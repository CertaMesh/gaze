use std::io::{self, Read, Write};
use std::net::SocketAddrV4;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gaze_inspection::{
    ActivatedInspectionConsumerV1, InspectionConsumerBindingV1, InspectionQueueLimitsV1,
    PendingInspectionConsumerV1,
};
use gaze_types::inspection::DashboardCaptureDescriptorV1;
use zeroize::Zeroize;

use crate::collector::WriterHandle;
use crate::ipc::reject_immediate_trailing;
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
const CONTROL_SOCKET_ENV: &str = "GAZE_DASHBOARD_CONTROL_SOCKET_V1";
const INSPECTION_SOCKET_ENV: &str = "GAZE_DASHBOARD_INSPECTION_SOCKET_V1";

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
    socket_dir: Option<PathBuf>,
}

impl SpawnedDashboardChild {
    /// Creates both private channels, spawns the exact child, and accepts only that launch.
    ///
    /// Arbitrary child/channel assembly is intentionally not part of the public surface:
    /// ```compile_fail
    /// # use gaze_proxy_dashboard::SpawnedDashboardChild;
    /// # fn loose(child: std::process::Child, a: std::os::unix::net::UnixStream,
    /// #          b: std::os::unix::net::UnixStream) {
    /// let _ = SpawnedDashboardChild::new(child, a, b);
    /// # }
    /// ```
    pub fn spawn(mut command: Command) -> Result<Self, DashboardError> {
        let socket_dir = create_socket_dir()?;
        let control_path = socket_dir.join("control.sock");
        let inspection_path = socket_dir.join("inspection.sock");
        let control_listener = match UnixListener::bind(&control_path) {
            Ok(listener) => listener,
            Err(_) => {
                cleanup_socket_dir(&socket_dir);
                return Err(DashboardError::new(
                    DashboardErrorCode::InvalidInheritedHandle,
                ));
            }
        };
        let inspection_listener = match UnixListener::bind(&inspection_path) {
            Ok(listener) => listener,
            Err(_) => {
                cleanup_socket_dir(&socket_dir);
                return Err(DashboardError::new(
                    DashboardErrorCode::InvalidInheritedHandle,
                ));
            }
        };
        if control_listener.set_nonblocking(true).is_err()
            || inspection_listener.set_nonblocking(true).is_err()
        {
            cleanup_socket_dir(&socket_dir);
            return Err(DashboardError::new(
                DashboardErrorCode::InvalidInheritedHandle,
            ));
        }
        command.env(CONTROL_SOCKET_ENV, &control_path);
        command.env(INSPECTION_SOCKET_ENV, &inspection_path);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                cleanup_socket_dir(&socket_dir);
                return Err(DashboardError::new(
                    DashboardErrorCode::InvalidInheritedHandle,
                ));
            }
        };
        let expected_pid = child.id();
        let accepted = (|| {
            let control = accept_owned_channel(&control_listener, &mut child, expected_pid)?;
            let inspection = accept_owned_channel(&inspection_listener, &mut child, expected_pid)?;
            Ok::<_, DashboardError>((control, inspection))
        })();
        let (control, inspection) = match accepted {
            Ok(channels) => channels,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                cleanup_socket_dir(&socket_dir);
                return Err(error);
            }
        };
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
            socket_dir: Some(socket_dir),
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
        if let Some(socket_dir) = self.socket_dir.take() {
            cleanup_socket_dir(&socket_dir);
        }
    }
}

fn cleanup_socket_dir(socket_dir: &std::path::Path) {
    let _ = std::fs::remove_file(socket_dir.join("control.sock"));
    let _ = std::fs::remove_file(socket_dir.join("inspection.sock"));
    let _ = std::fs::remove_dir(socket_dir);
}

fn create_socket_dir() -> Result<PathBuf, DashboardError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut suffix = String::with_capacity(32);
    for byte in random {
        suffix.push(char::from(HEX[(byte >> 4) as usize]));
        suffix.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    let path = std::env::temp_dir().join(format!("gaze-dashboard-{suffix}"));
    std::fs::create_dir(&path)
        .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
    Ok(path)
}

fn accept_owned_channel(
    listener: &UnixListener,
    child: &mut Child,
    expected_pid: u32,
) -> Result<UnixStream, DashboardError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                validate_peer_child(&stream, expected_pid)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if child.try_wait().ok().flatten().is_some() || Instant::now() >= deadline {
                    return Err(DashboardError::new(
                        DashboardErrorCode::InvalidInheritedHandle,
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => {
                return Err(DashboardError::new(
                    DashboardErrorCode::InvalidInheritedHandle,
                ));
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn validate_peer_child(stream: &UnixStream, expected_pid: u32) -> Result<(), DashboardError> {
    let credentials = rustix::net::sockopt::socket_peercred(stream)
        .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
    if credentials.pid.as_raw_pid() as u32 != expected_pid {
        return Err(DashboardError::new(
            DashboardErrorCode::InvalidInheritedHandle,
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn validate_peer_child(_stream: &UnixStream, _expected_pid: u32) -> Result<(), DashboardError> {
    Ok(())
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
        let (consumer, binding) = PendingInspectionConsumerV1::new_with_binding(
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
                binding,
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
    binding: InspectionConsumerBindingV1,
    frame_cap: usize,
    _config: DashboardStartupConfig,
}

impl PendingDashboardActivation {
    /// Commits only when the raw activated candidate matches this dashboard's retained one-shot
    /// inspection binding proof. Binding precedes every socket, writer, runtime, and admission
    /// side effect.
    pub fn commit(
        mut self,
        activated: ActivatedInspectionConsumerV1,
    ) -> Result<DashboardLaunch, DashboardError> {
        let bound = self
            .binding
            .bind(activated)
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;
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
            RuntimeParts::new(bound, admission.clone(), writer, child),
            self.authority,
        )?;
        if !admission.activate() {
            let _ = launch.control().shutdown();
            return Err(DashboardError::new(DashboardErrorCode::ActivationFailed));
        }
        Ok(launch)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn peer_pid_mismatch_is_rejected() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        let wrong = std::process::id().saturating_add(1);
        let error = validate_peer_child(&stream, wrong).unwrap_err();
        assert_eq!(error.code(), DashboardErrorCode::InvalidInheritedHandle);
    }
}
