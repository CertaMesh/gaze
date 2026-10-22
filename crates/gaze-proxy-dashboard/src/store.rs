use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gaze_types::inspection::{
    InspectionEmissionIdV1, InspectionEpochV1, InspectionSequenceV1, InspectionStageDomainV1,
    LogicalInspectionIdV1,
};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::ipc::DecodedInspectionFrame;
use crate::snapshot::{EventV1, PayloadAvailabilityV1, RetentionFactsV1, StageV1, StagesV1};
use crate::RetentionLimits;

const SENSITIVE_ENVELOPE_HEADER_BYTES: usize = 11;
const RESPONSE_WRITE_OVERHEAD_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoreCounters {
    pub(crate) expired: u64,
    pub(crate) capacity_evicted: u64,
    pub(crate) duplicate_idempotent: u64,
    pub(crate) duplicate_conflict: u64,
    pub(crate) stale_epoch: u64,
    pub(crate) oversize: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionOutcome {
    Inserted,
    DuplicateIdempotent,
    DuplicateConflict,
    StaleEpoch,
    Oversize,
    SequenceConflict,
}

struct StoredEmission {
    frame: DecodedInspectionFrame,
    accounted_bytes: usize,
    insertion_generation: u64,
    deadline: Instant,
}

struct StoredLogical {
    emissions: Vec<InspectionEmissionIdV1>,
    last_sequence: InspectionSequenceV1,
    insertion_generation: u64,
    deadline: Instant,
}

pub(crate) struct EventStore {
    global: HashMap<InspectionEmissionIdV1, StoredEmission>,
    logical: HashMap<LogicalInspectionIdV1, StoredLogical>,
    oldest: VecDeque<(LogicalInspectionIdV1, u64, Instant)>,
    retained_bytes: usize,
    active_response_bytes: Arc<AtomicUsize>,
    next_generation: u64,
    accepted_epoch: InspectionEpochV1,
    limits: RetentionLimits,
    counters: StoreCounters,
}

impl EventStore {
    pub(crate) fn new(limits: RetentionLimits, epoch: InspectionEpochV1) -> Self {
        Self {
            global: HashMap::new(),
            logical: HashMap::new(),
            oldest: VecDeque::new(),
            retained_bytes: 0,
            active_response_bytes: Arc::new(AtomicUsize::new(0)),
            next_generation: 1,
            accepted_epoch: epoch,
            limits,
            counters: StoreCounters::default(),
        }
    }

    pub(crate) fn admit(
        &mut self,
        frame: DecodedInspectionFrame,
        now: Instant,
    ) -> AdmissionOutcome {
        self.expire(now);
        if frame.epoch != self.accepted_epoch {
            self.counters.stale_epoch = self.counters.stale_epoch.saturating_add(1);
            return AdmissionOutcome::StaleEpoch;
        }
        if let Some(existing) = self.global.get(&frame.emission_id) {
            if existing.frame.identical_to(&frame) {
                self.counters.duplicate_idempotent =
                    self.counters.duplicate_idempotent.saturating_add(1);
                return AdmissionOutcome::DuplicateIdempotent;
            }
            self.counters.duplicate_conflict = self.counters.duplicate_conflict.saturating_add(1);
            return AdmissionOutcome::DuplicateConflict;
        }
        let is_new_logical = !self.logical.contains_key(&frame.logical_id);
        let accounted_bytes = frame
            .retained_bytes()
            .saturating_add(std::mem::size_of::<StoredEmission>() + 128)
            .saturating_add(if is_new_logical {
                std::mem::size_of::<StoredLogical>() + 128
            } else {
                0
            });
        if accounted_bytes > self.limits.max_bytes() {
            self.counters.oversize = self.counters.oversize.saturating_add(1);
            return AdmissionOutcome::Oversize;
        }
        if let Some(logical) = self.logical.get(&frame.logical_id) {
            if frame.sequence <= logical.last_sequence {
                self.counters.duplicate_conflict =
                    self.counters.duplicate_conflict.saturating_add(1);
                return AdmissionOutcome::SequenceConflict;
            }
        }

        while self.global.len() >= self.limits.max_events()
            || (is_new_logical && self.logical.len() >= self.limits.max_events())
            || self
                .retained_bytes
                .checked_add(self.active_response_bytes.load(Ordering::Acquire))
                .and_then(|bytes| bytes.checked_add(accounted_bytes))
                .is_none_or(|bytes| bytes > self.limits.max_bytes())
        {
            if !self.evict_oldest() {
                self.counters.oversize = self.counters.oversize.saturating_add(1);
                return AdmissionOutcome::Oversize;
            }
        }

        let logical_id = frame.logical_id;
        let emission_id = frame.emission_id;
        let sequence = frame.sequence;
        let deadline = match now.checked_add(self.limits.ttl()) {
            Some(deadline) => deadline,
            None => {
                self.counters.oversize = self.counters.oversize.saturating_add(1);
                return AdmissionOutcome::Oversize;
            }
        };
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);

        self.retained_bytes += accounted_bytes;
        self.global.insert(
            emission_id,
            StoredEmission {
                frame,
                accounted_bytes,
                insertion_generation: generation,
                deadline,
            },
        );
        match self.logical.get_mut(&logical_id) {
            Some(logical) => {
                logical.emissions.push(emission_id);
                logical.last_sequence = sequence;
            }
            None => {
                self.logical.insert(
                    logical_id,
                    StoredLogical {
                        emissions: vec![emission_id],
                        last_sequence: sequence,
                        insertion_generation: generation,
                        deadline,
                    },
                );
                self.oldest.push_back((logical_id, generation, deadline));
            }
        }
        AdmissionOutcome::Inserted
    }

    pub(crate) fn begin_epoch(&mut self, epoch: InspectionEpochV1) {
        self.purge();
        self.accepted_epoch = epoch;
    }

    pub(crate) fn purge(&mut self) {
        self.global.clear();
        self.logical.clear();
        self.oldest.clear();
        self.retained_bytes = 0;
    }

    pub(crate) fn expire(&mut self, now: Instant) {
        while let Some((logical_id, generation, deadline)) = self.oldest.front().copied() {
            if now < deadline {
                break;
            }
            self.oldest.pop_front();
            let should_remove = self.logical.get(&logical_id).is_some_and(|logical| {
                logical.insertion_generation == generation && logical.deadline <= now
            });
            if should_remove {
                self.remove_logical(logical_id);
                self.counters.expired = self.counters.expired.saturating_add(1);
            }
        }
    }

    pub(crate) fn lease(
        &mut self,
        logical_id: LogicalInspectionIdV1,
        emission_id: InspectionEmissionIdV1,
        stage_domain: InspectionStageDomainV1,
        now: Instant,
        reveal_deadline: Instant,
        auth_generation: u64,
    ) -> Option<ResponseLease> {
        self.expire(now);
        let stored = self.global.get(&emission_id)?;
        // An omitted payload is never leasable, whatever bytes or route are presented.
        if stored.frame.logical_id != logical_id
            || stored.frame.stage_domain != stage_domain
            || stored.frame.projection.payload() != PayloadAvailabilityV1::Present
            || now >= stored.deadline
            || now >= reveal_deadline
        {
            return None;
        }
        Some(ResponseLease {
            auth_generation,
            epoch: self.accepted_epoch,
            logical_id,
            emission_id,
            stage_domain,
            insertion_generation: stored.insertion_generation,
            deadline: stored.deadline.min(reveal_deadline),
        })
    }

    pub(crate) fn lease_valid(
        &mut self,
        lease: &ResponseLease,
        auth_generation: u64,
        now: Instant,
    ) -> bool {
        self.expire(now);
        if lease.auth_generation != auth_generation
            || lease.epoch != self.accepted_epoch
            || now >= lease.deadline
        {
            return false;
        }
        self.global.get(&lease.emission_id).is_some_and(|stored| {
            stored.frame.logical_id == lease.logical_id
                && stored.frame.stage_domain == lease.stage_domain
                && stored.insertion_generation == lease.insertion_generation
        })
    }

    pub(crate) fn encode_payload_for_response(
        &mut self,
        lease: &ResponseLease,
        auth_generation: u64,
        now: Instant,
        domain_tag: u8,
        stage_tag: u8,
    ) -> Option<SensitiveResponseBuffer> {
        if !self.lease_valid(lease, auth_generation, now) {
            return None;
        }
        let stored = self.global.get(&lease.emission_id)?;
        let envelope_len = stored
            .frame
            .payload
            .len()
            .checked_add(SENSITIVE_ENVELOPE_HEADER_BYTES)?;
        let reservation = envelope_len
            .checked_add(std::mem::size_of::<ResponseLease>())?
            .checked_add(RESPONSE_WRITE_OVERHEAD_BYTES)?;
        let total = self
            .retained_bytes
            .checked_add(self.active_response_bytes.load(Ordering::Acquire))?
            .checked_add(reservation)?;
        if total > self.limits.max_bytes() {
            return None;
        }
        self.active_response_bytes
            .fetch_add(reservation, Ordering::AcqRel);
        let mut bytes = Zeroizing::new(Vec::with_capacity(envelope_len));
        bytes.extend_from_slice(b"GZPL");
        bytes.push(1);
        bytes.push(domain_tag);
        bytes.push(stage_tag);
        bytes.extend_from_slice(&(stored.frame.payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(stored.frame.payload.as_slice());
        Some(SensitiveResponseBuffer {
            bytes,
            reservation,
            accounting: self.active_response_bytes.clone(),
        })
    }

    #[cfg(test)]
    pub(crate) fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(crate) const fn epoch(&self) -> InspectionEpochV1 {
        self.accepted_epoch
    }

    #[cfg(test)]
    pub(crate) fn logical_len(&self) -> usize {
        self.logical.len()
    }

    /// Fixture seam: lets browser fixtures show non-zero store counters.
    #[cfg(test)]
    pub(crate) fn counters_mut(&mut self) -> &mut StoreCounters {
        &mut self.counters
    }

    pub(crate) const fn safe_counters(&self) -> StoreCounters {
        self.counters
    }

    /// Retention facts for the snapshot runtime header.
    pub(crate) fn retention_facts(&self) -> RetentionFactsV1 {
        RetentionFactsV1 {
            epoch: self.accepted_epoch.get(),
            retained: self.logical.len() as u64,
            capacity: self.limits.max_events() as u64,
            ttl_seconds: self.limits.ttl().as_secs(),
        }
    }

    /// Typed, payload-free snapshot events, newest logical first.
    pub(crate) fn snapshot_events(&self, now: Instant) -> Vec<EventV1> {
        let mut logical_ids = self.logical.keys().copied().collect::<Vec<_>>();
        logical_ids.sort_by_key(|id| std::cmp::Reverse(id.get()));
        logical_ids
            .into_iter()
            .filter_map(|logical_id| {
                let logical = self.logical.get(&logical_id)?;
                let mut stages = StagesV1::default();
                let mut newest: Option<&DecodedInspectionFrame> = None;
                for emission_id in &logical.emissions {
                    let Some(stored) = self.global.get(emission_id) else {
                        continue;
                    };
                    let frame = &stored.frame;
                    stages.insert(
                        frame.stage_domain,
                        StageV1::new(&frame.meta, &frame.projection),
                    );
                    if newest.is_none_or(|current| current.sequence < frame.sequence) {
                        newest = Some(frame);
                    }
                }
                let newest = newest?;
                let remaining = logical.deadline.saturating_duration_since(now);
                let ttl_seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
                Some(EventV1::new(&newest.meta, ttl_seconds, stages))
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn counters(&self) -> StoreCounters {
        self.counters
    }

    fn evict_oldest(&mut self) -> bool {
        while let Some((logical_id, generation, _)) = self.oldest.pop_front() {
            let current = self
                .logical
                .get(&logical_id)
                .is_some_and(|logical| logical.insertion_generation == generation);
            if current {
                self.remove_logical(logical_id);
                self.counters.capacity_evicted = self.counters.capacity_evicted.saturating_add(1);
                return true;
            }
        }
        false
    }

    fn remove_logical(&mut self, logical_id: LogicalInspectionIdV1) {
        let Some(logical) = self.logical.remove(&logical_id) else {
            return;
        };
        for emission_id in logical.emissions {
            if let Some(stored) = self.global.remove(&emission_id) {
                self.retained_bytes = self.retained_bytes.saturating_sub(stored.accounted_bytes);
            }
        }
    }
}

/// Non-cloneable exact response authorization lease.
pub(crate) struct ResponseLease {
    auth_generation: u64,
    epoch: InspectionEpochV1,
    logical_id: LogicalInspectionIdV1,
    emission_id: InspectionEmissionIdV1,
    stage_domain: InspectionStageDomainV1,
    insertion_generation: u64,
    deadline: Instant,
}

pub(crate) struct SensitiveResponseBuffer {
    bytes: Zeroizing<Vec<u8>>,
    reservation: usize,
    accounting: Arc<AtomicUsize>,
}

impl Drop for SensitiveResponseBuffer {
    fn drop(&mut self) {
        self.accounting
            .fetch_sub(self.reservation, Ordering::AcqRel);
    }
}

impl SensitiveResponseBuffer {
    pub(crate) fn as_slice(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    #[cfg(test)]
    pub(crate) const fn reservation(&self) -> usize {
        self.reservation
    }
}

pub(crate) struct RevealRegistry {
    permits: HashMap<
        (
            u64,
            InspectionEpochV1,
            LogicalInspectionIdV1,
            InspectionEmissionIdV1,
            InspectionStageDomainV1,
        ),
        Instant,
    >,
    window: Duration,
}

impl RevealRegistry {
    pub(crate) fn new(window: Duration) -> Self {
        Self {
            permits: HashMap::new(),
            window,
        }
    }

    pub(crate) fn authorize(
        &mut self,
        auth_generation: u64,
        epoch: InspectionEpochV1,
        logical_id: LogicalInspectionIdV1,
        emission_id: InspectionEmissionIdV1,
        stage_domain: InspectionStageDomainV1,
        now: Instant,
    ) -> Option<Instant> {
        let deadline = now.checked_add(self.window)?;
        self.permits.insert(
            (
                auth_generation,
                epoch,
                logical_id,
                emission_id,
                stage_domain,
            ),
            deadline,
        );
        Some(deadline)
    }

    pub(crate) fn consume(
        &mut self,
        key: (
            u64,
            InspectionEpochV1,
            LogicalInspectionIdV1,
            InspectionEmissionIdV1,
            InspectionStageDomainV1,
        ),
        now: Instant,
    ) -> Option<Instant> {
        self.permits.remove(&key).filter(|deadline| now < *deadline)
    }

    pub(crate) fn purge(&mut self) {
        self.permits.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        logical: u64,
        emission: u64,
        sequence: u64,
        epoch: u64,
        stage_domain: InspectionStageDomainV1,
        payload: &[u8],
    ) -> DecodedInspectionFrame {
        DecodedInspectionFrame::for_test(logical, emission, sequence, epoch, stage_domain, payload)
    }

    #[test]
    fn global_emission_identity_is_exact_and_conflicts_do_not_mutate() {
        let limits = RetentionLimits::new(4, 4096, Duration::from_secs(60)).expect("valid limits");
        let now = Instant::now();
        let mut store = EventStore::new(limits, InspectionEpochV1::new(7));
        assert_eq!(
            store.admit(
                frame(
                    1,
                    9,
                    0,
                    7,
                    InspectionStageDomainV1::ProviderRequest,
                    b"<Email_1>",
                ),
                now,
            ),
            AdmissionOutcome::Inserted
        );
        let retained = store.retained_bytes();
        assert_eq!(
            store.admit(
                frame(
                    1,
                    9,
                    0,
                    7,
                    InspectionStageDomainV1::ProviderRequest,
                    b"<Email_1>",
                ),
                now,
            ),
            AdmissionOutcome::DuplicateIdempotent
        );
        assert_eq!(
            store.admit(
                frame(
                    2,
                    9,
                    0,
                    7,
                    InspectionStageDomainV1::ProviderResponse,
                    b"alice@example.invalid",
                ),
                now,
            ),
            AdmissionOutcome::DuplicateConflict
        );
        assert_eq!(store.retained_bytes(), retained);
        assert_eq!(store.logical_len(), 1);
        assert_eq!(store.counters().duplicate_idempotent, 1);
        assert_eq!(store.counters().duplicate_conflict, 1);
    }

    #[test]
    fn ttl_is_monotonic_exact_and_access_does_not_refresh() {
        let limits = RetentionLimits::new(2, 4096, Duration::from_secs(1)).expect("valid limits");
        let now = Instant::now();
        let mut store = EventStore::new(limits, InspectionEpochV1::new(0));
        assert_eq!(
            store.admit(
                frame(
                    1,
                    1,
                    0,
                    0,
                    InspectionStageDomainV1::OwnerRequest,
                    b"Dr. Schmidt",
                ),
                now,
            ),
            AdmissionOutcome::Inserted
        );
        store.expire(now + Duration::from_millis(999));
        assert_eq!(store.logical_len(), 1);
        store.expire(now + Duration::from_secs(1));
        assert_eq!(store.logical_len(), 0);
        assert_eq!(store.retained_bytes(), 0);
    }

    #[test]
    fn purge_fences_epoch_and_invalidates_response_lease() {
        let limits = RetentionLimits::new(2, 4096, Duration::from_secs(60)).expect("valid limits");
        let now = Instant::now();
        let mut store = EventStore::new(limits, InspectionEpochV1::new(3));
        let logical = LogicalInspectionIdV1::new(1);
        let emission = InspectionEmissionIdV1::new(1);
        let stage = InspectionStageDomainV1::OwnerRestoredResponse;
        assert_eq!(
            store.admit(frame(1, 1, 0, 3, stage, b"+1-555-0142"), now,),
            AdmissionOutcome::Inserted
        );
        let lease = store
            .lease(
                logical,
                emission,
                stage,
                now,
                now + Duration::from_secs(30),
                4,
            )
            .expect("exact lease");
        assert!(store.lease_valid(&lease, 4, now));
        store.begin_epoch(InspectionEpochV1::new(4));
        assert!(!store.lease_valid(&lease, 4, now));
        assert_eq!(
            store.admit(
                frame(
                    2,
                    2,
                    0,
                    3,
                    InspectionStageDomainV1::ProviderResponse,
                    b"<Email_1>"
                ),
                now,
            ),
            AdmissionOutcome::StaleEpoch
        );
        assert_eq!(store.logical_len(), 0);
    }

    #[test]
    fn reveal_permit_is_exact_and_consumed_once() {
        let now = Instant::now();
        let mut reveals = RevealRegistry::new(Duration::from_secs(30));
        let key = (
            8,
            InspectionEpochV1::new(2),
            LogicalInspectionIdV1::new(3),
            InspectionEmissionIdV1::new(4),
            InspectionStageDomainV1::OwnerRequest,
        );
        assert!(reveals
            .authorize(key.0, key.1, key.2, key.3, key.4, now)
            .is_some());
        assert!(reveals.consume(key, now).is_some());
        assert!(reveals.consume(key, now).is_none());
    }

    #[test]
    fn sensitive_response_accounts_full_single_buffer_peak_at_exact_cap() {
        let limits = RetentionLimits::new(4, 16 * 1024, Duration::from_secs(60)).unwrap();
        let now = Instant::now();
        let mut store = EventStore::new(limits, InspectionEpochV1::new(1));
        let stage = InspectionStageDomainV1::ProviderResponse;
        assert_eq!(
            store.admit(frame(1, 1, 0, 1, stage, b"<Email_1>"), now),
            AdmissionOutcome::Inserted
        );
        let lease = store
            .lease(
                LogicalInspectionIdV1::new(1),
                InspectionEmissionIdV1::new(1),
                stage,
                now,
                now + Duration::from_secs(30),
                0,
            )
            .unwrap();
        let first = store
            .encode_payload_for_response(&lease, 0, now, 1, 3)
            .unwrap();
        assert_eq!(&first.as_slice()[0..7], b"GZPL\x01\x01\x03");
        assert_eq!(&first.as_slice()[11..], b"<Email_1>");
        let exact = store
            .retained_bytes
            .checked_add(first.reservation() * 2)
            .unwrap();
        store.limits = RetentionLimits::new(4, exact - 1, Duration::from_secs(60)).unwrap();
        assert!(store
            .encode_payload_for_response(&lease, 0, now, 1, 3)
            .is_none());
        store.limits = RetentionLimits::new(4, exact, Duration::from_secs(60)).unwrap();
        let second = store
            .encode_payload_for_response(&lease, 0, now, 1, 3)
            .unwrap();
        assert_eq!(
            store.active_response_bytes.load(Ordering::Acquire),
            first.reservation() * 2
        );
        drop(second);
        drop(first);
        assert_eq!(store.active_response_bytes.load(Ordering::Acquire), 0);
    }

    #[test]
    fn omitted_payloads_are_never_leasable_and_present_ones_are() {
        use crate::test_support::{admit_all, emit_logical, StageShape, ALL_STAGES};
        use gaze_types::inspection::{CaptureDomainsV1, ProjectionOmissionReasonV1};
        let now = Instant::now();
        for stage in ALL_STAGES {
            for (shape, leasable) in [
                (StageShape::Present, true),
                (StageShape::PresentEmptyPayload, true),
                (
                    StageShape::PayloadOmitted(ProjectionOmissionReasonV1::LimitExceeded),
                    false,
                ),
                (
                    StageShape::ProjectionOmitted(ProjectionOmissionReasonV1::NotCapturedByPolicy),
                    false,
                ),
            ] {
                let mut store = crate::test_support::fixture_store();
                admit_all(
                    &mut store,
                    &emit_logical(CaptureDomainsV1::All, &[(stage, shape)]),
                    now,
                );
                let (emission, stored) = store.global.iter().next().unwrap();
                let logical = stored.frame.logical_id;
                let lease = store.lease(
                    logical,
                    *emission,
                    stage,
                    now,
                    now + Duration::from_secs(30),
                    0,
                );
                assert_eq!(lease.is_some(), leasable, "{stage:?} {shape:?}");
            }
        }
    }
}
