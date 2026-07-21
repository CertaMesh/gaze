use std::io::{self, Read, Write};
use std::net::SocketAddrV4;
use std::os::unix::net::UnixStream;
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gaze_inspection::{
    ActivatedInspectionConsumerV1, InspectionQueueLimitsV1, PendingInspectionConsumerV1,
};
use gaze_types::inspection::DashboardCaptureDescriptorV1;
use zeroize::Zeroize;

use crate::collector::WriterHandle;
use crate::runtime::{DashboardLaunch, RuntimeParts};
use crate::sink::{Admission, DashboardInspectionSink};
use crate::{
    DashboardError, DashboardErrorCode, DashboardStartupConfig, DeliveredAckV1, PairingEnvelopeV1,
};

const CHILD_PURGE_REQUEST: u8 = 0x20;
const PARENT_PURGE: u8 = 0x10;
const PARENT_ROTATE: u8 = 0x11;
const PARENT_SHUTDOWN: u8 = 0x12;
const CHILD_PURGED: u8 = 0x90;
const CHILD_STOPPED: u8 = 0x92;

/// Intentional secure launch-secret delivery boundary.
pub trait PairingDelivery: Send + 'static {
    /// Delivers the safe authority and canonical 43-byte token. Implementations must use only a
    /// controlling terminal or a reviewed local acknowledged handle.
    fn deliver(&mut self, authority: SocketAddrV4, token: &[u8]) -> io::Result<()>;
}

impl<F> PairingDelivery for F
where
    F: FnMut(SocketAddrV4, &[u8]) -> io::Result<()> + Send + 'static,
{
    fn deliver(&mut self, authority: SocketAddrV4, token: &[u8]) -> io::Result<()> {
        self(authority, token)
    }
}

/// Killable child plus validated local parent endpoints.
pub struct SpawnedDashboardChild {
    child: Option<Child>,
    control: UnixStream,
    inspection: Option<UnixStream>,
    paired_authority: Option<SocketAddrV4>,
}

impl SpawnedDashboardChild {
    /// Wraps a just-spawned child and connected local Unix sockets.
    pub fn new(
        child: Child,
        control: UnixStream,
        inspection: UnixStream,
    ) -> Result<Self, DashboardError> {
        control
            .peer_addr()
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        inspection
            .peer_addr()
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        control
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        control
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        Ok(Self {
            child: Some(child),
            control,
            inspection: Some(inspection),
            paired_authority: None,
        })
    }

    pub(crate) fn take_inspection(&mut self) -> Result<UnixStream, DashboardError> {
        self.inspection
            .take()
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::ActivationFailed))
    }

    pub(crate) fn has_exited(&mut self) -> bool {
        self.child
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten())
            .is_some()
    }

    pub(crate) fn take_browser_purge_request(&mut self) -> bool {
        let _ = self.control.set_nonblocking(true);
        let mut byte = [0_u8; 1];
        let result =
            matches!(self.control.read(&mut byte), Ok(1) if byte[0] == CHILD_PURGE_REQUEST);
        let _ = self.control.set_nonblocking(false);
        result
    }

    pub(crate) fn purge_and_zeroize(&mut self, next_epoch: u64) -> Result<(), DashboardError> {
        self.control
            .write_all(&[PARENT_PURGE])
            .and_then(|()| self.control.write_all(&next_epoch.to_be_bytes()))
            .and_then(|()| self.control.flush())
            .map_err(|_| DashboardError::new(DashboardErrorCode::PurgeFailed))?;
        let mut ack = [0_u8; 9];
        self.control
            .read_exact(&mut ack)
            .map_err(|_| DashboardError::new(DashboardErrorCode::PurgeFailed))?;
        if ack[0] != CHILD_PURGED || u64::from_be_bytes(ack[1..9].try_into().unwrap()) != next_epoch
        {
            return Err(DashboardError::new(DashboardErrorCode::PurgeFailed));
        }
        Ok(())
    }

    pub(crate) fn rotate_pairing(
        &mut self,
        delivery: &mut dyn PairingDelivery,
    ) -> Result<(), DashboardError> {
        self.control
            .write_all(&[PARENT_ROTATE])
            .and_then(|()| self.control.flush())
            .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
        let expected = self
            .paired_authority
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::PairingFailed))?;
        acknowledge_pairing(&mut self.control, delivery, |authority| {
            authority == expected
        })
        .map(|_| ())
    }

    pub(crate) fn shutdown_terminate_reap(&mut self) -> Result<(), DashboardError> {
        let _ = self.control.write_all(&[PARENT_SHUTDOWN]);
        let _ = self.control.flush();
        let mut ack = [0_u8; 1];
        let cooperative = self.control.read_exact(&mut ack).is_ok() && ack[0] == CHILD_STOPPED;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let Some(child) = self.child.as_mut() else {
                return Ok(());
            };
            if child
                .try_wait()
                .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?
                .is_some()
            {
                let _ = self.child.take();
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            child
                .wait()
                .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
        }
        if cooperative {
            Ok(())
        } else {
            Err(DashboardError::new(DashboardErrorCode::FatalDisabled))
        }
    }
}

impl Drop for SpawnedDashboardChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Pairing supervisor. It creates no pending inspection consumer before delivery acknowledgement.
pub struct DashboardSupervisor;

impl DashboardSupervisor {
    /// Completes child readiness and canonical nonce-bound delivery acknowledgement.
    pub fn prepare(
        config: DashboardStartupConfig,
        mut child: SpawnedDashboardChild,
        mut delivery: impl PairingDelivery,
    ) -> Result<PairedDashboard, DashboardError> {
        let DashboardStartupConfig::Enabled {
            acceptance,
            bind,
            retention: _,
            clients: _,
            ipc,
        } = config
        else {
            let _ = child.shutdown_terminate_reap();
            return Err(DashboardError::new(
                DashboardErrorCode::DisabledByConfiguration,
            ));
        };
        let authority = acknowledge_pairing(&mut child.control, &mut delivery, |authority| {
            bind.matches_authority(authority)
        })?;
        child.paired_authority = Some(authority);
        Ok(PairedDashboard {
            config,
            descriptor: acceptance.descriptor(),
            authority,
            child: Some(child),
            ipc,
        })
    }
}

fn acknowledge_pairing(
    control: &mut UnixStream,
    delivery: &mut dyn PairingDelivery,
    authority_matches: impl FnOnce(SocketAddrV4) -> bool,
) -> Result<SocketAddrV4, DashboardError> {
    let envelope = PairingEnvelopeV1::read_exact(control)?;
    reject_immediate_trailing(control)?;
    let authority = envelope.authority();
    if !authority_matches(authority) {
        return Err(DashboardError::new(DashboardErrorCode::PairingFailed));
    }
    let nonce = envelope.nonce();
    let secret = envelope.secret();
    let mut token = secret.canonical_token();
    delivery
        .deliver(authority, token.as_ref())
        .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
    token.zeroize();
    DeliveredAckV1::delivered(nonce)
        .write_to(control)
        .and_then(|()| control.flush())
        .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
    Ok(authority)
}

fn reject_immediate_trailing(control: &mut UnixStream) -> Result<(), DashboardError> {
    control
        .set_nonblocking(true)
        .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
    let mut trailing = [0_u8; 1];
    let read = control.read(&mut trailing);
    control
        .set_nonblocking(false)
        .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
    match read {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
        _ => Err(DashboardError::new(DashboardErrorCode::PairingFailed)),
    }
}

/// Acknowledged one-shot pairing state. It exposes no sink.
pub struct PairedDashboard {
    config: DashboardStartupConfig,
    descriptor: DashboardCaptureDescriptorV1,
    authority: SocketAddrV4,
    child: Option<SpawnedDashboardChild>,
    ipc: crate::IpcLimits,
}

impl PairedDashboard {
    /// Consumes acknowledged pairing and creates exactly one pending consumer half.
    pub fn into_pending_activation(
        mut self,
    ) -> Result<
        (
            PendingDashboardActivation,
            PendingInspectionConsumerV1,
            DashboardCaptureDescriptorV1,
        ),
        DashboardError,
    > {
        let child = self
            .child
            .take()
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::ConsumerUnavailable))?;
        let (sink, receiver, admission) =
            DashboardInspectionSink::channel(self.ipc.ingress_items());
        let queue_limits =
            InspectionQueueLimitsV1::new(self.ipc.ingress_items(), self.ipc.frame_bytes())
                .map_err(|_| DashboardError::new(DashboardErrorCode::ConsumerUnavailable))?;
        let consumer = PendingInspectionConsumerV1::new(
            self.descriptor,
            Arc::clone(&sink) as Arc<dyn gaze_inspection::InspectionSink>,
            queue_limits,
        );
        Ok((
            PendingDashboardActivation {
                authority: self.authority,
                child: Some(child),
                receiver: Some(receiver),
                admission,
                frame_cap: self.ipc.frame_bytes(),
                _config: self.config,
            },
            consumer,
            self.descriptor,
        ))
    }
}

/// One-shot activation half retained by Track B while master atomically installs inspection.
pub struct PendingDashboardActivation {
    authority: SocketAddrV4,
    child: Option<SpawnedDashboardChild>,
    receiver: Option<Receiver<gaze_inspection::InspectionEventV1>>,
    admission: Arc<Admission>,
    frame_cap: usize,
    _config: DashboardStartupConfig,
}

impl PendingDashboardActivation {
    /// Commits only an activated provider-neutral consumer handle from the atomic installation.
    pub fn commit(
        mut self,
        activated: ActivatedInspectionConsumerV1,
    ) -> Result<DashboardLaunch, DashboardError> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::ActivationFailed))?;
        let receiver = self
            .receiver
            .take()
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::ActivationFailed))?;
        let inspection = child.take_inspection()?;
        let writer = WriterHandle::spawn(receiver, inspection, self.frame_cap)?;
        let admission = self.admission.clone();
        let launch = DashboardLaunch::start(
            RuntimeParts {
                activated,
                admission: admission.clone(),
                writer,
                child,
            },
            self.authority,
        )?;
        if !admission.activate() {
            let _ = launch.control().shutdown();
            return Err(DashboardError::new(DashboardErrorCode::ActivationFailed));
        }
        Ok(launch)
    }
}
