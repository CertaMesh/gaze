// Purge transitions are compiled and tested while production activation remains fail-closed.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;

use gaze_inspection::{InspectionEventV1, InspectionSink, InspectionSinkErrorV1};

const STATE_MASK: u64 = 0xff;
const GENERATION_STEP: u64 = 1 << 8;
const PENDING: u64 = 0;
const RUNNING: u64 = 1;
const PURGING: u64 = 2;
const DISABLED: u64 = 3;
const QUIESCENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

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
    state_and_generation: AtomicU64,
    in_flight: AtomicUsize,
    accepted: AtomicU64,
    full: AtomicU64,
    closed: AtomicU64,
}

impl Admission {
    fn new() -> Self {
        Self {
            state_and_generation: AtomicU64::new(PENDING),
            in_flight: AtomicUsize::new(0),
            accepted: AtomicU64::new(0),
            full: AtomicU64::new(0),
            closed: AtomicU64::new(0),
        }
    }

    pub(crate) fn activate(&self) -> bool {
        self.state_and_generation
            .compare_exchange(PENDING, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn close_for_purge(&self) -> bool {
        let mut observed = self.state_and_generation.load(Ordering::Acquire);
        loop {
            if observed & STATE_MASK != RUNNING {
                return false;
            }
            let next = observed.wrapping_add(GENERATION_STEP) & !STATE_MASK | PURGING;
            match self.state_and_generation.compare_exchange_weak(
                observed,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return self.wait_for_quiescence(),
                Err(actual) => observed = actual,
            }
        }
    }

    pub(crate) fn reopen_after_purge(&self) -> bool {
        let observed = self.state_and_generation.load(Ordering::Acquire);
        observed & STATE_MASK == PURGING
            && self
                .state_and_generation
                .compare_exchange(
                    observed,
                    observed & !STATE_MASK | RUNNING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
    }

    pub(crate) fn disable(&self) -> bool {
        let mut observed = self.state_and_generation.load(Ordering::Acquire);
        loop {
            if observed & STATE_MASK == DISABLED {
                return self.wait_for_quiescence();
            }
            let next = observed.wrapping_add(GENERATION_STEP) & !STATE_MASK | DISABLED;
            match self.state_and_generation.compare_exchange_weak(
                observed,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return self.wait_for_quiescence(),
                Err(actual) => observed = actual,
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn is_running(&self) -> bool {
        self.state_and_generation.load(Ordering::Acquire) & STATE_MASK == RUNNING
    }

    fn acquire(self: &Arc<Self>) -> Option<AdmissionPermit> {
        let observed = self.state_and_generation.load(Ordering::Acquire);
        if observed & STATE_MASK != RUNNING {
            return None;
        }
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        if self.state_and_generation.load(Ordering::Acquire) != observed {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(AdmissionPermit {
            admission: self.clone(),
        })
    }

    fn wait_for_quiescence(&self) -> bool {
        let deadline = std::time::Instant::now() + QUIESCENCE_TIMEOUT;
        while self.in_flight.load(Ordering::Acquire) != 0 {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::yield_now();
        }
        true
    }

    pub(crate) fn snapshot(&self) -> IngressCounters {
        IngressCounters {
            accepted: self.accepted.load(Ordering::Relaxed),
            full: self.full.load(Ordering::Relaxed),
            closed: self.closed.load(Ordering::Relaxed),
        }
    }
}

struct AdmissionPermit {
    admission: Arc<Admission>,
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        self.admission.in_flight.fetch_sub(1, Ordering::AcqRel);
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
        let Some(_permit) = self.admission.acquire() else {
            self.admission.closed.fetch_add(1, Ordering::Relaxed);
            return Err(InspectionSinkErrorV1::Closed);
        };
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
                let _ = self.admission.disable();
                self.admission.closed.fetch_add(1, Ordering::Relaxed);
                Err(InspectionSinkErrorV1::Closed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::time::Duration;

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

    #[test]
    fn purge_linearizes_before_any_post_close_send_permit() {
        let (_sink, _receiver, admission) = DashboardInspectionSink::channel(1);
        assert!(admission.activate());
        let barrier = Arc::new(Barrier::new(2));
        let held_admission = admission.clone();
        let held_barrier = barrier.clone();
        let (acquired_tx, acquired_rx) = mpsc::sync_channel(1);
        let holder = std::thread::spawn(move || {
            let permit = held_admission.acquire().expect("running permit");
            acquired_tx.send(()).unwrap();
            held_barrier.wait();
            drop(permit);
        });
        acquired_rx.recv().unwrap();
        let closed_admission = admission.clone();
        let (closed_tx, closed_rx) = mpsc::sync_channel(1);
        let closer = std::thread::spawn(move || {
            closed_tx.send(closed_admission.close_for_purge()).unwrap();
        });
        assert!(closed_rx.recv_timeout(Duration::from_millis(20)).is_err());
        barrier.wait();
        assert!(closed_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        holder.join().unwrap();
        closer.join().unwrap();
        assert!(admission.acquire().is_none());
    }
}
