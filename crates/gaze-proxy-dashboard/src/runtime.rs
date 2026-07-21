use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gaze_inspection::ActivatedInspectionConsumerV1;

use crate::collector::WriterHandle;
use crate::sink::Admission;
use crate::supervisor::{PairingDelivery, SpawnedDashboardChild};
use crate::{DashboardError, DashboardErrorCode, DashboardStatus};

/// Serialized runtime lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardLifecycle {
    /// Admission is open for the exact installed epoch.
    Running(u64),
    /// Admission is closed while matching registration-bound purge is in flight.
    Purging {
        /// Previous accepted epoch.
        from: u64,
        /// Runtime-selected next epoch.
        to: u64,
    },
    /// Fatal one-way disable; this state cannot resurrect.
    Disabled,
    /// Child termination and reap completed.
    Stopped,
}

enum RuntimeCommand {
    Purge(Sender<Result<(), DashboardError>>),
    Rotate(Box<dyn PairingDelivery>, Sender<Result<(), DashboardError>>),
    Shutdown(Sender<Result<(), DashboardError>>),
}

pub(crate) struct RuntimeParts {
    pub(crate) activated: ActivatedInspectionConsumerV1,
    pub(crate) admission: Arc<Admission>,
    pub(crate) writer: WriterHandle,
    pub(crate) child: SpawnedDashboardChild,
}

/// Host control for purge, rotation, shutdown, and sanitized status.
#[derive(Clone)]
pub struct DashboardControl {
    commands: Sender<RuntimeCommand>,
    lifecycle: Arc<Mutex<DashboardLifecycle>>,
    status: Arc<Mutex<DashboardStatus>>,
}

impl DashboardControl {
    /// Performs a serialized reusable purge. Track B ingress closes and drains before begin-purge;
    /// child state is zeroized while the matching guard is held; only then is it completed.
    pub fn purge(&self) -> Result<(), DashboardError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.commands
            .send(RuntimeCommand::Purge(reply_tx))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
        reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?
    }

    /// Rotates the launch secret through the same canonical acknowledged delivery protocol.
    pub fn rotate_pairing_secret(
        &self,
        delivery: Box<dyn PairingDelivery>,
    ) -> Result<(), DashboardError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.commands
            .send(RuntimeCommand::Rotate(delivery, reply_tx))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
        reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?
    }

    /// Permanently disables capture, zeroizes state, terminates, and reaps the child.
    pub fn shutdown(&self) -> Result<(), DashboardError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.commands
            .send(RuntimeCommand::Shutdown(reply_tx))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
        reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?
    }

    /// Returns the sanitized status.
    #[must_use]
    pub fn status(&self) -> DashboardStatus {
        *self
            .status
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Returns the serialized lifecycle.
    #[must_use]
    pub fn lifecycle(&self) -> DashboardLifecycle {
        *self
            .lifecycle
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// Activated dashboard handle. It exposes no sink, descriptor, epoch authority, or inspection
/// registration object.
pub struct DashboardLaunch {
    control: DashboardControl,
    authority: std::net::SocketAddrV4,
    supervisor: Option<JoinHandle<()>>,
}

impl DashboardLaunch {
    pub(crate) fn start(
        parts: RuntimeParts,
        authority: std::net::SocketAddrV4,
    ) -> Result<Self, DashboardError> {
        let (commands, command_rx) = mpsc::channel();
        let lifecycle = Arc::new(Mutex::new(DashboardLifecycle::Running(0)));
        let status = Arc::new(Mutex::new(DashboardStatus::Active));
        let thread_lifecycle = lifecycle.clone();
        let thread_status = status.clone();
        let supervisor = thread::Builder::new()
            .name("gaze-dashboard-supervisor".to_owned())
            .spawn(move || runtime_loop(parts, command_rx, &thread_lifecycle, &thread_status))
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;
        Ok(Self {
            control: DashboardControl {
                commands,
                lifecycle,
                status,
            },
            authority,
            supervisor: Some(supervisor),
        })
    }

    /// Returns a cloneable host control. It contains no inspection sink.
    #[must_use]
    pub fn control(&self) -> DashboardControl {
        self.control.clone()
    }

    /// Returns the safe literal listener authority for intentional operator display.
    #[must_use]
    pub const fn authority(&self) -> std::net::SocketAddrV4 {
        self.authority
    }
}

impl Drop for DashboardLaunch {
    fn drop(&mut self) {
        let _ = self.control.shutdown();
        if let Some(supervisor) = self.supervisor.take() {
            let _ = supervisor.join();
        }
    }
}

fn runtime_loop(
    mut parts: RuntimeParts,
    commands: Receiver<RuntimeCommand>,
    lifecycle: &Mutex<DashboardLifecycle>,
    status: &Mutex<DashboardStatus>,
) {
    loop {
        if parts.writer.is_faulted() || parts.child.has_exited() {
            disable_and_reap(&mut parts, lifecycle, status);
            break;
        }
        if parts.child.take_browser_purge_request() && purge(&mut parts, lifecycle, status).is_err()
        {
            disable_and_reap(&mut parts, lifecycle, status);
            break;
        }
        match commands.recv_timeout(Duration::from_millis(10)) {
            Ok(RuntimeCommand::Purge(reply)) => {
                let result = purge(&mut parts, lifecycle, status);
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    disable_and_reap(&mut parts, lifecycle, status);
                    break;
                }
            }
            Ok(RuntimeCommand::Rotate(mut delivery, reply)) => {
                let result = rotate(&mut parts, lifecycle, status, delivery.as_mut());
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    disable_and_reap(&mut parts, lifecycle, status);
                    break;
                }
            }
            Ok(RuntimeCommand::Shutdown(reply)) => {
                disable_and_reap(&mut parts, lifecycle, status);
                let _ = reply.send(Ok(()));
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                disable_and_reap(&mut parts, lifecycle, status);
                break;
            }
        }
    }
}

fn purge(
    parts: &mut RuntimeParts,
    lifecycle: &Mutex<DashboardLifecycle>,
    status: &Mutex<DashboardStatus>,
) -> Result<(), DashboardError> {
    if !parts.admission.close_for_purge() {
        return Err(DashboardError::new(DashboardErrorCode::PurgeFailed));
    }
    parts.writer.drain()?;
    let guard = parts
        .activated
        .begin_purge()
        .map_err(|_| DashboardError::new(DashboardErrorCode::PurgeFailed))?;
    let next = guard.next_epoch().get();
    let from = lifecycle_epoch(lifecycle);
    *lifecycle
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) =
        DashboardLifecycle::Purging { from, to: next };
    *status.lock().unwrap_or_else(|poison| poison.into_inner()) = DashboardStatus::Purging;
    parts.child.purge_and_zeroize(next)?;
    let completed = guard
        .complete()
        .map_err(|_| DashboardError::new(DashboardErrorCode::PurgeFailed))?;
    if completed.get() != next || !parts.admission.reopen_after_purge() {
        return Err(DashboardError::new(DashboardErrorCode::PurgeFailed));
    }
    *lifecycle
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = DashboardLifecycle::Running(next);
    *status.lock().unwrap_or_else(|poison| poison.into_inner()) = DashboardStatus::Active;
    Ok(())
}

fn rotate(
    parts: &mut RuntimeParts,
    lifecycle: &Mutex<DashboardLifecycle>,
    status: &Mutex<DashboardStatus>,
    delivery: &mut dyn PairingDelivery,
) -> Result<(), DashboardError> {
    purge(parts, lifecycle, status)?;
    parts.child.rotate_pairing(delivery)
}

fn disable_and_reap(
    parts: &mut RuntimeParts,
    lifecycle: &Mutex<DashboardLifecycle>,
    status: &Mutex<DashboardStatus>,
) {
    parts.admission.disable();
    let _ = parts.writer.stop_and_join();
    let _ = parts.activated.disable();
    *lifecycle
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = DashboardLifecycle::Disabled;
    *status.lock().unwrap_or_else(|poison| poison.into_inner()) =
        DashboardStatus::Disabled(DashboardErrorCode::FatalDisabled);
    let _ = parts.child.shutdown_terminate_reap();
    *lifecycle
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = DashboardLifecycle::Stopped;
    *status.lock().unwrap_or_else(|poison| poison.into_inner()) = DashboardStatus::Stopped;
}

fn lifecycle_epoch(lifecycle: &Mutex<DashboardLifecycle>) -> u64 {
    match *lifecycle
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
    {
        DashboardLifecycle::Running(epoch) => epoch,
        DashboardLifecycle::Purging { to, .. } => to,
        DashboardLifecycle::Disabled | DashboardLifecycle::Stopped => 0,
    }
}
