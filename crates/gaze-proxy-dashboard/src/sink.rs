use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;

use gaze_inspection::{InspectionEventV1, InspectionSink, InspectionSinkErrorV1};

const PENDING: u8 = 0;
const RUNNING: u8 = 1;
const PURGING: u8 = 2;
const DISABLED: u8 = 3;

/// Saturating safe ingress counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IngressCounters {
    /// Events accepted into bounded Track B ingress.
    pub accepted: u64,
    /// Events dropped because ingress was full.
    pub full: u64,
    /// Events rejected while pending, purging, or disabled.
    pub closed: u64,
}

pub(crate) struct Admission {
    state: AtomicU8,
    accepted: AtomicU64,
    full: AtomicU64,
    closed: AtomicU64,
}

impl Admission {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(PENDING),
            accepted: AtomicU64::new(0),
            full: AtomicU64::new(0),
            closed: AtomicU64::new(0),
        }
    }

    pub(crate) fn activate(&self) -> bool {
        self.state
            .compare_exchange(PENDING, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn close_for_purge(&self) -> bool {
        self.state
            .compare_exchange(RUNNING, PURGING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn reopen_after_purge(&self) -> bool {
        self.state
            .compare_exchange(PURGING, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn disable(&self) {
        self.state.store(DISABLED, Ordering::Release);
    }

    pub(crate) fn is_running(&self) -> bool {
        self.state.load(Ordering::Acquire) == RUNNING
    }

    pub(crate) fn snapshot(&self) -> IngressCounters {
        IngressCounters {
            accepted: self.accepted.load(Ordering::Relaxed),
            full: self.full.load(Ordering::Relaxed),
            closed: self.closed.load(Ordering::Relaxed),
        }
    }
}

/// Bounded nonblocking inspection sink. Its request-path operation is one try-send.
pub struct DashboardInspectionSink {
    sender: SyncSender<InspectionEventV1>,
    admission: Arc<Admission>,
}

impl DashboardInspectionSink {
    pub(crate) fn channel(
        capacity: usize,
    ) -> (Arc<Self>, Receiver<InspectionEventV1>, Arc<Admission>) {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let admission = Arc::new(Admission::new());
        (
            Arc::new(Self {
                sender,
                admission: admission.clone(),
            }),
            receiver,
            admission,
        )
    }

    /// Returns safe ingress counters. Queue occupancy is deliberately not inferred.
    #[must_use]
    pub fn counters(&self) -> IngressCounters {
        self.admission.snapshot()
    }
}

impl InspectionSink for DashboardInspectionSink {
    fn try_emit(&self, event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
        if !self.admission.is_running() {
            self.admission.closed.fetch_add(1, Ordering::Relaxed);
            return Err(InspectionSinkErrorV1::Closed);
        }
        match self.sender.try_send(event) {
            Ok(()) => {
                self.admission.accepted.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(TrySendError::Full(_event)) => {
                self.admission.full.fetch_add(1, Ordering::Relaxed);
                Err(InspectionSinkErrorV1::Rejected)
            }
            Err(TrySendError::Disconnected(_event)) => {
                self.admission.disable();
                self.admission.closed.fetch_add(1, Ordering::Relaxed);
                Err(InspectionSinkErrorV1::Closed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_pending_until_activation_and_fatal_disable_wins() {
        let (_sink, _receiver, admission) = DashboardInspectionSink::channel(1);
        assert!(!admission.is_running());
        assert!(admission.activate());
        assert!(admission.close_for_purge());
        admission.disable();
        assert!(!admission.reopen_after_purge());
        assert!(!admission.is_running());
    }
}
