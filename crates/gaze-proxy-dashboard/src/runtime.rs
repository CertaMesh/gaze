use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gaze_inspection::BoundActivatedInspectionConsumerV1;

use crate::collector::WriterHandle;
use crate::sink::Admission;
use crate::supervisor::{PairingDelivery, SpawnedDashboardChild};
use crate::{DashboardError, DashboardErrorCode, DashboardStatus};

const CONTROL_QUEUE_CAPACITY: usize = 16;

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

struct Lifecycle {
    state: Mutex<DashboardLifecycle>,
    changed: Condvar,
    #[cfg(test)]
    wait_entered: Mutex<Option<Sender<()>>>,
}

impl Lifecycle {
    fn new() -> Self {
        Self {
            state: Mutex::new(DashboardLifecycle::Running(0)),
            changed: Condvar::new(),
            #[cfg(test)]
            wait_entered: Mutex::new(None),
        }
    }

    fn set(&self, value: DashboardLifecycle) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = value;
        self.changed.notify_all();
    }

    fn get(&self) -> DashboardLifecycle {
        *self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn wait_for_epoch(&self, minimum: u64) -> Result<u64, DashboardError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        #[cfg(test)]
        if let Some(entered) = self.wait_entered.lock().unwrap().take() {
            entered.send(()).unwrap();
        }
        let (state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |state| match state {
                DashboardLifecycle::Running(epoch) => *epoch < minimum,
                DashboardLifecycle::Purging { .. } => true,
                DashboardLifecycle::Disabled | DashboardLifecycle::Stopped => false,
            })
            .unwrap_or_else(|poison| poison.into_inner());
        match *state {
            DashboardLifecycle::Running(epoch) if epoch >= minimum => Ok(epoch),
            _ => Err(DashboardError::new(DashboardErrorCode::FatalDisabled)),
        }
    }
}

enum RuntimeCommand {
    Purge(Sender<Result<(), DashboardError>>),
    Rotate(Box<dyn PairingDelivery>, Sender<Result<(), DashboardError>>),
    Shutdown(Sender<Result<(), DashboardError>>),
}

pub(crate) struct RuntimeParts {
    activated: BoundActivatedInspectionConsumerV1,
    admission: Arc<Admission>,
    writer: WriterHandle,
    child: SpawnedDashboardChild,
}

impl RuntimeParts {
    pub(crate) fn new(
        activated: BoundActivatedInspectionConsumerV1,
        admission: Arc<Admission>,
        writer: WriterHandle,
        child: SpawnedDashboardChild,
    ) -> Self {
        Self {
            activated,
            admission,
            writer,
            child,
        }
    }
}

/// Host control for purge, rotation, shutdown, and sanitized status.
#[derive(Clone)]
pub struct DashboardControl {
    commands: SyncSender<RuntimeCommand>,
    lifecycle: Arc<Lifecycle>,
}

impl DashboardControl {
    /// Performs a serialized reusable purge. Track B ingress closes and drains before begin-purge;
    /// child state is zeroized while the matching guard is held; only then is it completed.
    pub fn purge(&self) -> Result<(), DashboardError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        send_command(&self.commands, RuntimeCommand::Purge(reply_tx))?;
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
        send_command(&self.commands, RuntimeCommand::Rotate(delivery, reply_tx))?;
        reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?
    }

    /// Permanently disables capture, zeroizes state, terminates, and reaps the child.
    pub fn shutdown(&self) -> Result<(), DashboardError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        send_command(&self.commands, RuntimeCommand::Shutdown(reply_tx))?;
        reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?
    }

    /// Returns the sanitized status.
    #[must_use]
    pub fn status(&self) -> DashboardStatus {
        match self.lifecycle.get() {
            DashboardLifecycle::Running(_) => DashboardStatus::Active,
            DashboardLifecycle::Purging { .. } => DashboardStatus::Purging,
            DashboardLifecycle::Disabled => {
                DashboardStatus::Disabled(DashboardErrorCode::FatalDisabled)
            }
            DashboardLifecycle::Stopped => DashboardStatus::Stopped,
        }
    }

    /// Returns the serialized lifecycle.
    #[must_use]
    pub fn lifecycle(&self) -> DashboardLifecycle {
        self.lifecycle.get()
    }

    /// Waits for an acknowledged running epoch, failing on disable, stop, or timeout.
    ///
    /// Returns `FatalDisabled` on disable, stop, or expiry of the five-second wait.
    /// A timeout does not disable the dashboard: `status()` may still be `Active`.
    /// Check `lifecycle()` to distinguish terminal states from an unacknowledged epoch.
    ///
    /// A returned epoch is a snapshot: subsequent browser requests may start another purge.
    /// Use after completing HTTP requests to await their purge notifications without polling.
    pub fn wait_for_epoch(&self, minimum: u64) -> Result<u64, DashboardError> {
        self.lifecycle.wait_for_epoch(minimum)
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
        let (commands, command_rx) = mpsc::sync_channel(CONTROL_QUEUE_CAPACITY);
        let lifecycle = Arc::new(Lifecycle::new());
        let thread_lifecycle = lifecycle.clone();
        let supervisor = thread::Builder::new()
            .name("gaze-dashboard-supervisor".to_owned())
            .spawn(move || runtime_loop(parts, command_rx, &thread_lifecycle))
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;
        Ok(Self {
            control: DashboardControl {
                commands,
                lifecycle,
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
    lifecycle: &Lifecycle,
) {
    loop {
        if parts.writer.is_faulted() || parts.child.has_exited() {
            disable_and_reap(&mut parts, lifecycle);
            break;
        }
        if parts.child.take_browser_purge_request() && purge(&mut parts, lifecycle).is_err() {
            disable_and_reap(&mut parts, lifecycle);
            break;
        }
        match commands.recv_timeout(Duration::from_millis(10)) {
            Ok(RuntimeCommand::Purge(reply)) => {
                let result = purge(&mut parts, lifecycle);
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    disable_and_reap(&mut parts, lifecycle);
                    break;
                }
            }
            Ok(RuntimeCommand::Rotate(mut delivery, reply)) => {
                let result = rotate(&mut parts, lifecycle, delivery.as_mut());
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    disable_and_reap(&mut parts, lifecycle);
                    break;
                }
            }
            Ok(RuntimeCommand::Shutdown(reply)) => {
                disable_and_reap(&mut parts, lifecycle);
                let _ = reply.send(Ok(()));
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                disable_and_reap(&mut parts, lifecycle);
                break;
            }
        }
    }
}

fn purge(parts: &mut RuntimeParts, lifecycle: &Lifecycle) -> Result<(), DashboardError> {
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
    lifecycle.set(DashboardLifecycle::Purging { from, to: next });
    parts.child.purge_and_zeroize(next)?;
    let completed = guard
        .complete()
        .map_err(|_| DashboardError::new(DashboardErrorCode::PurgeFailed))?;
    if completed.get() != next || !parts.admission.reopen_after_purge() {
        return Err(DashboardError::new(DashboardErrorCode::PurgeFailed));
    }
    lifecycle.set(DashboardLifecycle::Running(next));
    Ok(())
}

fn rotate(
    parts: &mut RuntimeParts,
    lifecycle: &Lifecycle,
    delivery: &mut dyn PairingDelivery,
) -> Result<(), DashboardError> {
    purge(parts, lifecycle)?;
    parts.child.rotate_pairing(delivery)
}

fn disable_and_reap(parts: &mut RuntimeParts, lifecycle: &Lifecycle) {
    let _ = parts.admission.disable();
    let _ = parts.activated.disable();
    let _ = parts.writer.stop_and_join();
    lifecycle.set(DashboardLifecycle::Disabled);
    let _ = parts.child.shutdown_terminate_reap();
    lifecycle.set(DashboardLifecycle::Stopped);
}

fn send_command(
    commands: &SyncSender<RuntimeCommand>,
    command: RuntimeCommand,
) -> Result<(), DashboardError> {
    commands.try_send(command).map_err(|error| match error {
        TrySendError::Full(_) => DashboardError::new(DashboardErrorCode::ControlQueueFull),
        TrySendError::Disconnected(_) => DashboardError::new(DashboardErrorCode::FatalDisabled),
    })
}

fn lifecycle_epoch(lifecycle: &Lifecycle) -> u64 {
    match lifecycle.get() {
        DashboardLifecycle::Running(epoch) => epoch,
        DashboardLifecycle::Purging { to, .. } => to,
        DashboardLifecycle::Disabled | DashboardLifecycle::Stopped => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_ack_waits_for_running_and_rejects_terminal_states() {
        for terminal in [
            None,
            Some(DashboardLifecycle::Disabled),
            Some(DashboardLifecycle::Stopped),
        ] {
            let lifecycle = Arc::new(Lifecycle::new());
            lifecycle.set(DashboardLifecycle::Purging { from: 0, to: 1 });
            let (entered_tx, entered_rx) = mpsc::channel();
            *lifecycle.wait_entered.lock().unwrap() = Some(entered_tx);
            let worker = lifecycle.clone();
            let waiting = thread::spawn(move || worker.wait_for_epoch(1));
            entered_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("waiter entered under lifecycle lock");
            lifecycle.set(terminal.unwrap_or(DashboardLifecycle::Running(1)));
            let result = waiting.join().unwrap();
            if terminal.is_some() {
                assert_eq!(
                    result.unwrap_err().code(),
                    DashboardErrorCode::FatalDisabled
                );
            } else {
                assert_eq!(result.unwrap(), 1);
            }
        }
    }

    #[test]
    fn epoch_ack_rejects_disabled_even_when_minimum_is_zero() {
        let lifecycle = Lifecycle::new();
        lifecycle.set(DashboardLifecycle::Disabled);
        assert_eq!(
            lifecycle.wait_for_epoch(0).unwrap_err().code(),
            DashboardErrorCode::FatalDisabled
        );
    }

    #[test]
    fn runtime_parts_constructor_requires_bound_activation_proof() {
        let _: fn(
            BoundActivatedInspectionConsumerV1,
            Arc<Admission>,
            WriterHandle,
            SpawnedDashboardChild,
        ) -> RuntimeParts = RuntimeParts::new;
    }

    #[test]
    fn serialized_control_queue_accepts_exactly_sixteen_pending_commands() {
        let (commands, _receiver) = mpsc::sync_channel(CONTROL_QUEUE_CAPACITY);
        for _ in 0..CONTROL_QUEUE_CAPACITY {
            let (reply, _reply_rx) = mpsc::channel();
            assert!(commands.try_send(RuntimeCommand::Purge(reply)).is_ok());
        }
        let (reply, _reply_rx) = mpsc::channel();
        assert!(matches!(
            commands.try_send(RuntimeCommand::Purge(reply)),
            Err(TrySendError::Full(_))
        ));
    }
}
