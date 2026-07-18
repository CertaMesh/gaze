//! Provider-neutral, bounded inspection delivery for Gaze.
//!
//! Sensitive bytes can be observed only through intentionally named callbacks. Calling such a
//! callback, or delivering a payload-bearing event to an [`InspectionSink`], is trusted
//! declassification: bytes deliberately copied by trusted sink code cannot be revoked. The
//! metadata-only path never constructs content-derived projections.

#![forbid(unsafe_code)]

use gaze_types::inspection::{
    AttestationTraceV1, DashboardCaptureDescriptorV1, DecisionTraceV1, InspectionDomainV1,
    InspectionDropCodeV1, InspectionEmissionIdV1, InspectionEpochV1, InspectionEventMetaV1,
    InspectionMeasurementV1, InspectionSafeFieldsV1, InspectionSequenceV1, InspectionStageDomainV1,
    JsonShapeSummaryV1, LogicalInspectionIdV1, PiiSummaryV1, ProjectionAvailabilityV1,
    ProjectionOmissionReasonV1, SseTimelineMetaV1,
};
use std::cell::Cell;
use std::collections::VecDeque;
use std::fmt;
use std::marker::PhantomData;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use zeroize::Zeroize;

const MAX_QUEUE_ITEMS_V1: usize = 4_096;
const MAX_QUEUE_BYTES_V1: usize = 64 * 1024 * 1024;

static NEXT_REGISTRATION_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_LOGICAL_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_EMISSION_ID: AtomicU64 = AtomicU64::new(1);

struct SensitiveBytes {
    bytes: Vec<u8>,
    #[cfg(test)]
    zeroize_probe: Option<Arc<AtomicBool>>,
}

impl SensitiveBytes {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            #[cfg(test)]
            zeroize_probe: None,
        }
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn with_declassified_bytes<R, F>(&self, callback: F) -> R
    where
        F: for<'a> FnOnce(&'a [u8]) -> R,
    {
        callback(&self.bytes)
    }

    #[cfg(test)]
    fn with_zeroize_probe(bytes: Vec<u8>, probe: Arc<AtomicBool>) -> Self {
        Self {
            bytes,
            zeroize_probe: Some(probe),
        }
    }
}

impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        self.bytes.zeroize();
        #[cfg(test)]
        if let Some(probe) = &self.zeroize_probe {
            probe.store(self.bytes.iter().all(|byte| *byte == 0), Ordering::SeqCst);
        }
    }
}

macro_rules! sensitive_payload {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        pub struct $name {
            bytes: SensitiveBytes,
        }

        impl $name {
            /// Captures owned bytes into a zeroizing, non-formatting domain wrapper.
            #[must_use]
            pub fn capture(bytes: Vec<u8>) -> Self {
                Self {
                    bytes: SensitiveBytes::new(bytes),
                }
            }

            /// Intentionally declassifies the bytes to one non-consuming trusted callback.
            ///
            /// The higher-ranked callback lifetime prevents a borrow from escaping. Trusted code
            /// can still deliberately copy the bytes; such a copy is outside runtime revocation.
            pub fn with_declassified_bytes<R, F>(&self, callback: F) -> R
            where
                F: for<'a> FnOnce(&'a [u8]) -> R,
            {
                self.bytes.with_declassified_bytes(callback)
            }

            fn sensitive_len(&self) -> usize {
                self.bytes.len()
            }

            #[cfg(test)]
            fn capture_with_probe(bytes: Vec<u8>, probe: Arc<AtomicBool>) -> Self {
                Self {
                    bytes: SensitiveBytes::with_zeroize_probe(bytes, probe),
                }
            }
        }
    };
}

sensitive_payload!(
    OwnerRawPayloadV1,
    "Zeroizing owner-request bytes. This type has no formatting, serialization, cloning, equality, hashing, dereferencing, or implicit-conversion surface."
);
sensitive_payload!(
    ProviderVisiblePayloadV1,
    "Zeroizing pseudonymized provider-visible bytes. This type has no formatting, serialization, cloning, equality, hashing, dereferencing, or implicit-conversion surface."
);
sensitive_payload!(
    OwnerRestoredPayloadV1,
    "Zeroizing owner-restored bytes. This type has no formatting, serialization, cloning, equality, hashing, dereferencing, or implicit-conversion surface."
);

macro_rules! domain_projection {
    ($name:ident, $payload:ident, $description:literal) => {
        #[doc = $description]
        pub struct $name {
            payload: ProjectionAvailabilityV1<$payload>,
            measurement: ProjectionAvailabilityV1<InspectionMeasurementV1>,
            json_shape: ProjectionAvailabilityV1<JsonShapeSummaryV1>,
            sse_timeline: ProjectionAvailabilityV1<SseTimelineMetaV1>,
            pii_summary: ProjectionAvailabilityV1<PiiSummaryV1>,
            decision_trace: ProjectionAvailabilityV1<DecisionTraceV1>,
            attestation: ProjectionAvailabilityV1<AttestationTraceV1>,
        }

        impl $name {
            /// Creates one total authorized projection. Every absent component has a reason.
            #[allow(clippy::too_many_arguments)]
            #[must_use]
            pub fn new(
                payload: ProjectionAvailabilityV1<$payload>,
                measurement: ProjectionAvailabilityV1<InspectionMeasurementV1>,
                json_shape: ProjectionAvailabilityV1<JsonShapeSummaryV1>,
                sse_timeline: ProjectionAvailabilityV1<SseTimelineMetaV1>,
                pii_summary: ProjectionAvailabilityV1<PiiSummaryV1>,
                decision_trace: ProjectionAvailabilityV1<DecisionTraceV1>,
                attestation: ProjectionAvailabilityV1<AttestationTraceV1>,
            ) -> Self {
                Self {
                    payload,
                    measurement,
                    json_shape,
                    sse_timeline,
                    pii_summary,
                    decision_trace,
                    attestation,
                }
            }

            /// Returns the typed payload availability.
            #[must_use]
            pub const fn payload(&self) -> &ProjectionAvailabilityV1<$payload> {
                &self.payload
            }

            /// Returns the authorized aggregate measurement availability.
            #[must_use]
            pub const fn measurement(&self) -> &ProjectionAvailabilityV1<InspectionMeasurementV1> {
                &self.measurement
            }

            /// Returns the authorized JSON-shape availability.
            #[must_use]
            pub const fn json_shape(&self) -> &ProjectionAvailabilityV1<JsonShapeSummaryV1> {
                &self.json_shape
            }

            /// Returns the authorized SSE-timeline availability.
            #[must_use]
            pub const fn sse_timeline(&self) -> &ProjectionAvailabilityV1<SseTimelineMetaV1> {
                &self.sse_timeline
            }

            /// Returns the authorized PII-summary availability.
            #[must_use]
            pub const fn pii_summary(&self) -> &ProjectionAvailabilityV1<PiiSummaryV1> {
                &self.pii_summary
            }

            /// Returns the authorized decision-trace availability.
            #[must_use]
            pub const fn decision_trace(&self) -> &ProjectionAvailabilityV1<DecisionTraceV1> {
                &self.decision_trace
            }

            /// Returns the authorized attestation availability.
            #[must_use]
            pub const fn attestation(&self) -> &ProjectionAvailabilityV1<AttestationTraceV1> {
                &self.attestation
            }

            fn sensitive_len(&self) -> usize {
                let payload = match &self.payload {
                    ProjectionAvailabilityV1::Present(payload) => payload.sensitive_len(),
                    ProjectionAvailabilityV1::Omitted(_) => 0,
                };
                payload
                    .saturating_add(projected_slice_bytes(
                        &self.json_shape,
                        |value| value.nodes().len(),
                        std::mem::size_of::<gaze_types::inspection::JsonShapeNodeV1>(),
                    ))
                    .saturating_add(projected_slice_bytes(
                        &self.sse_timeline,
                        |value| value.entries().len(),
                        std::mem::size_of::<gaze_types::inspection::SseTimelineEntryV1>(),
                    ))
                    .saturating_add(projected_slice_bytes(
                        &self.pii_summary,
                        |value| value.entries().len(),
                        std::mem::size_of::<gaze_types::inspection::PiiCountV1>(),
                    ))
                    .saturating_add(projected_slice_bytes(
                        &self.decision_trace,
                        |value| value.entries().len(),
                        std::mem::size_of::<gaze_types::inspection::DecisionTraceEntryV1>(),
                    ))
            }
        }
    };
}

fn projected_slice_bytes<T, F>(
    availability: &ProjectionAvailabilityV1<T>,
    len: F,
    element_size: usize,
) -> usize
where
    F: FnOnce(&T) -> usize,
{
    match availability {
        ProjectionAvailabilityV1::Present(value) => len(value).saturating_mul(element_size),
        ProjectionAvailabilityV1::Omitted(_) => 0,
    }
}

domain_projection!(
    OwnerRawProjectionV1,
    OwnerRawPayloadV1,
    "Domain-authorized owner-request projection."
);
domain_projection!(
    ProviderVisibleProjectionV1,
    ProviderVisiblePayloadV1,
    "Domain-authorized provider-visible projection."
);
domain_projection!(
    OwnerRestoredProjectionV1,
    OwnerRestoredPayloadV1,
    "Domain-authorized owner-restored projection."
);

enum InspectionEventBodyV1 {
    OwnerRequest(ProjectionAvailabilityV1<OwnerRawProjectionV1>),
    ProviderRequest(ProjectionAvailabilityV1<ProviderVisibleProjectionV1>),
    ProviderResponse(ProjectionAvailabilityV1<ProviderVisibleProjectionV1>),
    OwnerRestoredResponse(ProjectionAvailabilityV1<OwnerRestoredProjectionV1>),
}

impl InspectionEventBodyV1 {
    fn stage_domain(&self) -> InspectionStageDomainV1 {
        match self {
            Self::OwnerRequest(_) => InspectionStageDomainV1::OwnerRequest,
            Self::ProviderRequest(_) => InspectionStageDomainV1::ProviderRequest,
            Self::ProviderResponse(_) => InspectionStageDomainV1::ProviderResponse,
            Self::OwnerRestoredResponse(_) => InspectionStageDomainV1::OwnerRestoredResponse,
        }
    }

    fn sensitive_len(&self) -> usize {
        match self {
            Self::OwnerRequest(ProjectionAvailabilityV1::Present(value)) => value.sensitive_len(),
            Self::ProviderRequest(ProjectionAvailabilityV1::Present(value))
            | Self::ProviderResponse(ProjectionAvailabilityV1::Present(value)) => {
                value.sensitive_len()
            }
            Self::OwnerRestoredResponse(ProjectionAvailabilityV1::Present(value)) => {
                value.sensitive_len()
            }
            Self::OwnerRequest(ProjectionAvailabilityV1::Omitted(_))
            | Self::ProviderRequest(ProjectionAvailabilityV1::Omitted(_))
            | Self::ProviderResponse(ProjectionAvailabilityV1::Omitted(_))
            | Self::OwnerRestoredResponse(ProjectionAvailabilityV1::Omitted(_)) => 0,
        }
    }
}

/// Content-free event kind for routing and hostile-sink assertions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionEventKindV1 {
    OwnerRequest,
    ProviderRequest,
    ProviderResponse,
    OwnerRestoredResponse,
    Omitted {
        stage_domain: InspectionStageDomainV1,
        reason: ProjectionOmissionReasonV1,
    },
}

/// Borrowed typed event view. It never erases a payload to generic bytes/domain pairs.
pub enum InspectionEventViewV1<'a> {
    OwnerRequest(&'a OwnerRawProjectionV1),
    ProviderRequest(&'a ProviderVisibleProjectionV1),
    ProviderResponse(&'a ProviderVisibleProjectionV1),
    OwnerRestoredResponse(&'a OwnerRestoredProjectionV1),
    Omitted {
        stage_domain: InspectionStageDomainV1,
        reason: ProjectionOmissionReasonV1,
    },
}

/// Non-serializable, non-formatting inspection event delivered to a trusted sink.
pub struct InspectionEventV1 {
    meta: InspectionEventMetaV1,
    body: InspectionEventBodyV1,
}

impl InspectionEventV1 {
    /// Returns hostile-sink-safe metadata.
    #[must_use]
    pub const fn meta(&self) -> &InspectionEventMetaV1 {
        &self.meta
    }

    /// Returns a content-free event kind.
    #[must_use]
    pub fn kind(&self) -> InspectionEventKindV1 {
        match &self.body {
            InspectionEventBodyV1::OwnerRequest(ProjectionAvailabilityV1::Present(_)) => {
                InspectionEventKindV1::OwnerRequest
            }
            InspectionEventBodyV1::ProviderRequest(ProjectionAvailabilityV1::Present(_)) => {
                InspectionEventKindV1::ProviderRequest
            }
            InspectionEventBodyV1::ProviderResponse(ProjectionAvailabilityV1::Present(_)) => {
                InspectionEventKindV1::ProviderResponse
            }
            InspectionEventBodyV1::OwnerRestoredResponse(ProjectionAvailabilityV1::Present(_)) => {
                InspectionEventKindV1::OwnerRestoredResponse
            }
            InspectionEventBodyV1::OwnerRequest(ProjectionAvailabilityV1::Omitted(reason))
            | InspectionEventBodyV1::ProviderRequest(ProjectionAvailabilityV1::Omitted(reason))
            | InspectionEventBodyV1::ProviderResponse(ProjectionAvailabilityV1::Omitted(reason))
            | InspectionEventBodyV1::OwnerRestoredResponse(ProjectionAvailabilityV1::Omitted(
                reason,
            )) => InspectionEventKindV1::Omitted {
                stage_domain: self.body.stage_domain(),
                reason: *reason,
            },
        }
    }

    /// Returns a typed projection view. Sensitive payload bytes remain behind their callback.
    #[must_use]
    pub fn view(&self) -> InspectionEventViewV1<'_> {
        match &self.body {
            InspectionEventBodyV1::OwnerRequest(ProjectionAvailabilityV1::Present(value)) => {
                InspectionEventViewV1::OwnerRequest(value)
            }
            InspectionEventBodyV1::ProviderRequest(ProjectionAvailabilityV1::Present(value)) => {
                InspectionEventViewV1::ProviderRequest(value)
            }
            InspectionEventBodyV1::ProviderResponse(ProjectionAvailabilityV1::Present(value)) => {
                InspectionEventViewV1::ProviderResponse(value)
            }
            InspectionEventBodyV1::OwnerRestoredResponse(ProjectionAvailabilityV1::Present(
                value,
            )) => InspectionEventViewV1::OwnerRestoredResponse(value),
            InspectionEventBodyV1::OwnerRequest(ProjectionAvailabilityV1::Omitted(reason))
            | InspectionEventBodyV1::ProviderRequest(ProjectionAvailabilityV1::Omitted(reason))
            | InspectionEventBodyV1::ProviderResponse(ProjectionAvailabilityV1::Omitted(reason))
            | InspectionEventBodyV1::OwnerRestoredResponse(ProjectionAvailabilityV1::Omitted(
                reason,
            )) => InspectionEventViewV1::Omitted {
                stage_domain: self.body.stage_domain(),
                reason: *reason,
            },
        }
    }
}

/// Closed, sanitized sink rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionSinkErrorV1 {
    Rejected,
    Closed,
    FailedClosed,
}

/// Object-safe trusted inspection delivery boundary.
pub trait InspectionSink: Send + Sync + 'static {
    /// Receives one fully constructed event on the dispatcher thread, never the producer path.
    fn try_emit(&self, event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1>;
}

/// Sink that deliberately drops every event without revealing payload bytes.
#[derive(Default)]
pub struct NoopInspectionSinkV1;

impl InspectionSink for NoopInspectionSinkV1 {
    fn try_emit(&self, _event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
        Ok(())
    }
}

/// Bounded queue configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InspectionQueueLimitsV1 {
    max_items: usize,
    max_bytes: usize,
}

impl InspectionQueueLimitsV1 {
    /// Creates limits inside the reviewed hard ceilings.
    pub const fn new(max_items: usize, max_bytes: usize) -> Result<Self, InspectionLimitErrorV1> {
        if max_items == 0 || max_items > MAX_QUEUE_ITEMS_V1 {
            return Err(InspectionLimitErrorV1::InvalidItemLimit);
        }
        if max_bytes == 0 || max_bytes > MAX_QUEUE_BYTES_V1 {
            return Err(InspectionLimitErrorV1::InvalidByteLimit);
        }
        Ok(Self {
            max_items,
            max_bytes,
        })
    }
}

/// Closed queue-limit validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionLimitErrorV1 {
    InvalidItemLimit,
    InvalidByteLimit,
}

impl fmt::Display for InspectionLimitErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidItemLimit => "invalid_inspection_item_limit",
            Self::InvalidByteLimit => "invalid_inspection_byte_limit",
        })
    }
}

impl std::error::Error for InspectionLimitErrorV1 {}

/// Opaque one-shot producer half supplied before atomic installation.
#[must_use]
pub struct PendingInspectionProducerV1 {
    descriptor: DashboardCaptureDescriptorV1,
}

impl PendingInspectionProducerV1 {
    /// Creates the producer pending half for one immutable descriptor.
    pub const fn new(descriptor: DashboardCaptureDescriptorV1) -> Self {
        Self { descriptor }
    }
}

/// Opaque one-shot consumer half supplied after adopter acknowledgement.
#[must_use]
pub struct PendingInspectionConsumerV1 {
    descriptor: DashboardCaptureDescriptorV1,
    sink: Arc<dyn InspectionSink>,
    limits: InspectionQueueLimitsV1,
}

impl PendingInspectionConsumerV1 {
    /// Seals the exact descriptor, sink, and bounded queue configuration into a pending half.
    pub fn new(
        descriptor: DashboardCaptureDescriptorV1,
        sink: Arc<dyn InspectionSink>,
        limits: InspectionQueueLimitsV1,
    ) -> Self {
        Self {
            descriptor,
            sink,
            limits,
        }
    }
}

/// Closed installation failure without source text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionInstallErrorV1 {
    DescriptorMismatch,
    RegistrationIdentityExhausted,
    DispatcherUnavailable,
}

impl fmt::Display for InspectionInstallErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DescriptorMismatch => "inspection_descriptor_mismatch",
            Self::RegistrationIdentityExhausted => "inspection_registration_identity_exhausted",
            Self::DispatcherUnavailable => "inspection_dispatcher_unavailable",
        })
    }
}

impl std::error::Error for InspectionInstallErrorV1 {}

struct RegistrationIdentity {
    _nonce: u64,
}

enum LifecycleStateV1 {
    Running(u64),
    Purging { from: u64, to: u64 },
    Disabled,
}

struct RuntimeInner {
    lifecycle: LifecycleStateV1,
    queue: VecDeque<QueuedInspectionEventV1>,
    closed: bool,
    sink_panics: u64,
    sink_rejections: u64,
}

struct QueueAccounting {
    items: AtomicUsize,
    bytes: AtomicUsize,
}

struct QueueReservation {
    accounting: Arc<QueueAccounting>,
    bytes: usize,
}

impl QueueReservation {
    fn new(accounting: Arc<QueueAccounting>, bytes: usize) -> Self {
        accounting.items.fetch_add(1, Ordering::AcqRel);
        accounting.bytes.fetch_add(bytes, Ordering::AcqRel);
        Self { accounting, bytes }
    }
}

impl Drop for QueueReservation {
    fn drop(&mut self) {
        self.accounting.items.fetch_sub(1, Ordering::AcqRel);
        self.accounting
            .bytes
            .fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

struct QueuedInspectionEventV1 {
    epoch: u64,
    event: InspectionEventV1,
    reservation: QueueReservation,
}

#[cfg(test)]
#[derive(Default)]
struct DispatchHooks {
    after_dequeue: Option<Arc<std::sync::Barrier>>,
    after_permit: Option<Arc<std::sync::Barrier>>,
}

struct RegistrationState {
    _identity: RegistrationIdentity,
    descriptor: DashboardCaptureDescriptorV1,
    limits: InspectionQueueLimitsV1,
    inner: Mutex<RuntimeInner>,
    changed: Condvar,
    accounting: Arc<QueueAccounting>,
    producer_open: AtomicBool,
    #[cfg(test)]
    hooks: Mutex<DispatchHooks>,
}

impl RegistrationState {
    fn try_inner(&self) -> Result<MutexGuard<'_, RuntimeInner>, InspectionDropCodeV1> {
        match self.inner.try_lock() {
            Ok(inner) => Ok(inner),
            Err(TryLockError::WouldBlock) => Err(InspectionDropCodeV1::QueueContended),
            Err(TryLockError::Poisoned(poisoned)) => {
                let mut inner = poisoned.into_inner();
                inner.lifecycle = LifecycleStateV1::Disabled;
                inner.closed = true;
                inner.queue.clear();
                Err(InspectionDropCodeV1::Disabled)
            }
        }
    }

    fn disable(&self) -> InspectionDisableOutcomeV1 {
        let mut inner = lock_without_recovery(&self.inner);
        if matches!(inner.lifecycle, LifecycleStateV1::Disabled) {
            return InspectionDisableOutcomeV1::AlreadyDisabled;
        }
        inner.lifecycle = LifecycleStateV1::Disabled;
        inner.closed = true;
        inner.queue.clear();
        self.changed.notify_all();
        InspectionDisableOutcomeV1::Disabled
    }
}

fn lock_without_recovery<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn next_bounded_id(counter: &AtomicU64) -> Option<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .ok()
}

/// Sole consuming atomic installation operation for the matched pending halves.
pub fn install_inspection_v1(
    producer: PendingInspectionProducerV1,
    consumer: PendingInspectionConsumerV1,
) -> Result<(InstalledInspectionProducerV1, ActivatedInspectionConsumerV1), InspectionInstallErrorV1>
{
    if producer.descriptor != consumer.descriptor {
        return Err(InspectionInstallErrorV1::DescriptorMismatch);
    }
    let registration_id = next_bounded_id(&NEXT_REGISTRATION_ID)
        .ok_or(InspectionInstallErrorV1::RegistrationIdentityExhausted)?;
    let state = Arc::new(RegistrationState {
        _identity: RegistrationIdentity {
            _nonce: registration_id,
        },
        descriptor: producer.descriptor,
        limits: consumer.limits,
        inner: Mutex::new(RuntimeInner {
            lifecycle: LifecycleStateV1::Running(0),
            queue: VecDeque::new(),
            closed: false,
            sink_panics: 0,
            sink_rejections: 0,
        }),
        changed: Condvar::new(),
        accounting: Arc::new(QueueAccounting {
            items: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
        }),
        producer_open: AtomicBool::new(true),
        #[cfg(test)]
        hooks: Mutex::new(DispatchHooks::default()),
    });
    let dispatcher_state = state.clone();
    let sink = consumer.sink;
    std::thread::Builder::new()
        .name("gaze-inspection".to_owned())
        .spawn(move || dispatcher_loop(dispatcher_state, sink))
        .map_err(|_| InspectionInstallErrorV1::DispatcherUnavailable)?;
    Ok((
        InstalledInspectionProducerV1 {
            state: state.clone(),
        },
        ActivatedInspectionConsumerV1 {
            state,
            _single_owner: PhantomData,
        },
    ))
}

fn dispatcher_loop(state: Arc<RegistrationState>, sink: Arc<dyn InspectionSink>) {
    loop {
        let queued = {
            let mut inner = lock_without_recovery(&state.inner);
            while inner.queue.is_empty() && !inner.closed {
                inner = state
                    .changed
                    .wait(inner)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            match inner.queue.pop_front() {
                Some(queued) => queued,
                None => return,
            }
        };

        #[cfg(test)]
        run_dispatch_hook(&state.hooks, |hooks| hooks.after_dequeue.clone());

        let permit = {
            let inner = lock_without_recovery(&state.inner);
            matches!(
                inner.lifecycle,
                LifecycleStateV1::Running(epoch) if epoch == queued.epoch && !inner.closed
            )
        };
        if !permit {
            drop(queued);
            continue;
        }

        #[cfg(test)]
        run_dispatch_hook(&state.hooks, |hooks| hooks.after_permit.clone());

        let QueuedInspectionEventV1 {
            event, reservation, ..
        } = queued;
        let delivered = catch_unwind(AssertUnwindSafe(|| sink.try_emit(event)));
        drop(reservation);
        let mut inner = lock_without_recovery(&state.inner);
        match delivered {
            Ok(Ok(())) => {}
            Ok(Err(_)) => inner.sink_rejections = inner.sink_rejections.saturating_add(1),
            Err(_) => inner.sink_panics = inner.sink_panics.saturating_add(1),
        }
    }
}

#[cfg(test)]
fn run_dispatch_hook<F>(hooks: &Mutex<DispatchHooks>, select: F)
where
    F: FnOnce(&DispatchHooks) -> Option<Arc<std::sync::Barrier>>,
{
    let barrier = select(&lock_without_recovery(hooks));
    if let Some(barrier) = barrier {
        barrier.wait();
        barrier.wait();
    }
}

/// Matched installed producer capability. It exposes no sink, epoch, control, or identity.
#[must_use]
pub struct InstalledInspectionProducerV1 {
    state: Arc<RegistrationState>,
}

impl InstalledInspectionProducerV1 {
    /// Mints one registration-bound logical emitter with a fresh content-independent ID.
    pub fn begin_logical(
        &self,
    ) -> Result<InspectionLogicalEmitterV1, InspectionBeginLogicalErrorV1> {
        if !self.state.producer_open.load(Ordering::Acquire) {
            return Err(InspectionBeginLogicalErrorV1::Closed);
        }
        let inner = self.state.try_inner().map_err(|drop| match drop {
            InspectionDropCodeV1::QueueContended => InspectionBeginLogicalErrorV1::Contended,
            _ => InspectionBeginLogicalErrorV1::Disabled,
        })?;
        match inner.lifecycle {
            LifecycleStateV1::Running(_) if !inner.closed => {}
            LifecycleStateV1::Purging { .. } => return Err(InspectionBeginLogicalErrorV1::Purging),
            LifecycleStateV1::Disabled | LifecycleStateV1::Running(_) => {
                return Err(InspectionBeginLogicalErrorV1::Disabled)
            }
        }
        drop(inner);
        let logical_id = next_bounded_id(&NEXT_LOGICAL_ID)
            .map(LogicalInspectionIdV1::new)
            .ok_or(InspectionBeginLogicalErrorV1::IdOverflow)?;
        Ok(InspectionLogicalEmitterV1 {
            state: self.state.clone(),
            logical_id,
            sequence: InspectionSequenceV1::new(0),
            exhausted: false,
        })
    }
}

impl Drop for InstalledInspectionProducerV1 {
    fn drop(&mut self) {
        self.state.producer_open.store(false, Ordering::Release);
        self.state.changed.notify_all();
    }
}

/// Closed logical-emitter creation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionBeginLogicalErrorV1 {
    Contended,
    Purging,
    Disabled,
    Closed,
    IdOverflow,
}

/// Closed bounded admission result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionAdmissionOutcomeV1 {
    Accepted {
        emission_id: InspectionEmissionIdV1,
        sequence: InspectionSequenceV1,
    },
    Dropped(InspectionDropCodeV1),
}

/// Registration-bound per-logical producer cursor.
#[must_use]
pub struct InspectionLogicalEmitterV1 {
    state: Arc<RegistrationState>,
    logical_id: LogicalInspectionIdV1,
    sequence: InspectionSequenceV1,
    exhausted: bool,
}

impl InspectionLogicalEmitterV1 {
    /// Emits OwnerRaw only when the immutable startup descriptor authorizes it.
    pub fn try_emit_owner_request<F>(
        &mut self,
        safe: InspectionSafeFieldsV1,
        build: F,
    ) -> InspectionAdmissionOutcomeV1
    where
        F: FnOnce() -> ProjectionAvailabilityV1<OwnerRawProjectionV1>,
    {
        self.try_emit(
            InspectionStageDomainV1::OwnerRequest,
            safe,
            build,
            InspectionEventBodyV1::OwnerRequest,
        )
    }

    /// Emits ProviderVisible request only when the immutable descriptor authorizes it.
    pub fn try_emit_provider_request<F>(
        &mut self,
        safe: InspectionSafeFieldsV1,
        build: F,
    ) -> InspectionAdmissionOutcomeV1
    where
        F: FnOnce() -> ProjectionAvailabilityV1<ProviderVisibleProjectionV1>,
    {
        self.try_emit(
            InspectionStageDomainV1::ProviderRequest,
            safe,
            build,
            InspectionEventBodyV1::ProviderRequest,
        )
    }

    /// Emits ProviderVisible response only when the immutable descriptor authorizes it.
    pub fn try_emit_provider_response<F>(
        &mut self,
        safe: InspectionSafeFieldsV1,
        build: F,
    ) -> InspectionAdmissionOutcomeV1
    where
        F: FnOnce() -> ProjectionAvailabilityV1<ProviderVisibleProjectionV1>,
    {
        self.try_emit(
            InspectionStageDomainV1::ProviderResponse,
            safe,
            build,
            InspectionEventBodyV1::ProviderResponse,
        )
    }

    /// Emits OwnerRestored only when the immutable startup descriptor authorizes it.
    pub fn try_emit_owner_restored_response<F>(
        &mut self,
        safe: InspectionSafeFieldsV1,
        build: F,
    ) -> InspectionAdmissionOutcomeV1
    where
        F: FnOnce() -> ProjectionAvailabilityV1<OwnerRestoredProjectionV1>,
    {
        self.try_emit(
            InspectionStageDomainV1::OwnerRestoredResponse,
            safe,
            build,
            InspectionEventBodyV1::OwnerRestoredResponse,
        )
    }

    fn try_emit<T, F, W>(
        &mut self,
        stage_domain: InspectionStageDomainV1,
        safe: InspectionSafeFieldsV1,
        build: F,
        wrap: W,
    ) -> InspectionAdmissionOutcomeV1
    where
        F: FnOnce() -> ProjectionAvailabilityV1<T>,
        W: FnOnce(ProjectionAvailabilityV1<T>) -> InspectionEventBodyV1,
    {
        if self.exhausted {
            return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::SequenceOverflow);
        }
        let next_sequence = match self.sequence.checked_next() {
            Ok(sequence) => sequence,
            Err(_) => {
                self.exhausted = true;
                return InspectionAdmissionOutcomeV1::Dropped(
                    InspectionDropCodeV1::SequenceOverflow,
                );
            }
        };
        let (epoch, selected) = match self.pre_projection(stage_domain.domain()) {
            Ok(result) => result,
            Err(drop) => return InspectionAdmissionOutcomeV1::Dropped(drop),
        };
        let projection = if selected {
            match catch_unwind(AssertUnwindSafe(build)) {
                Ok(projection) => projection,
                Err(_) => {
                    return InspectionAdmissionOutcomeV1::Dropped(
                        InspectionDropCodeV1::PartialEvent,
                    )
                }
            }
        } else {
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotCapturedByPolicy)
        };
        self.final_admission(epoch, next_sequence, safe, wrap(projection))
    }

    fn pre_projection(
        &self,
        domain: InspectionDomainV1,
    ) -> Result<(u64, bool), InspectionDropCodeV1> {
        if !self.state.producer_open.load(Ordering::Acquire) {
            return Err(InspectionDropCodeV1::QueueClosed);
        }
        let inner = self.state.try_inner()?;
        match inner.lifecycle {
            LifecycleStateV1::Running(epoch) if !inner.closed => {
                Ok((epoch, self.state.descriptor.captures(domain)))
            }
            LifecycleStateV1::Running(_) => Err(InspectionDropCodeV1::QueueClosed),
            LifecycleStateV1::Purging { .. } => Err(InspectionDropCodeV1::Purging),
            LifecycleStateV1::Disabled => Err(InspectionDropCodeV1::Disabled),
        }
    }

    fn final_admission(
        &mut self,
        expected_epoch: u64,
        sequence: InspectionSequenceV1,
        safe: InspectionSafeFieldsV1,
        body: InspectionEventBodyV1,
    ) -> InspectionAdmissionOutcomeV1 {
        let sensitive_len = body.sensitive_len();
        if sensitive_len > self.state.limits.max_bytes {
            return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueByteLimit);
        }
        if !self.state.producer_open.load(Ordering::Acquire) {
            return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueClosed);
        }
        let mut inner = match self.state.try_inner() {
            Ok(inner) => inner,
            Err(drop) => return InspectionAdmissionOutcomeV1::Dropped(drop),
        };
        match inner.lifecycle {
            LifecycleStateV1::Running(epoch) if epoch == expected_epoch && !inner.closed => {}
            LifecycleStateV1::Running(_) if inner.closed => {
                return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueClosed)
            }
            LifecycleStateV1::Running(_) => {
                return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::StaleEpoch)
            }
            LifecycleStateV1::Purging { .. } => {
                return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Purging)
            }
            LifecycleStateV1::Disabled => {
                return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Disabled)
            }
        }
        let current_items = self.state.accounting.items.load(Ordering::Acquire);
        let current_bytes = self.state.accounting.bytes.load(Ordering::Acquire);
        if current_items >= self.state.limits.max_items {
            return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueFull);
        }
        if current_bytes
            .checked_add(sensitive_len)
            .is_none_or(|sum| sum > self.state.limits.max_bytes)
        {
            return InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueByteLimit);
        }
        let emission_id = match next_bounded_id(&NEXT_EMISSION_ID) {
            Some(id) => InspectionEmissionIdV1::new(id),
            None => {
                self.exhausted = true;
                return InspectionAdmissionOutcomeV1::Dropped(
                    InspectionDropCodeV1::SequenceOverflow,
                );
            }
        };
        let meta = InspectionEventMetaV1::new(
            self.logical_id,
            emission_id,
            sequence,
            InspectionEpochV1::new(expected_epoch),
            body.stage_domain(),
            safe,
        );
        let event = InspectionEventV1 { meta, body };
        let reservation = QueueReservation::new(self.state.accounting.clone(), sensitive_len);
        inner.queue.push_back(QueuedInspectionEventV1 {
            epoch: expected_epoch,
            event,
            reservation,
        });
        self.sequence = sequence;
        drop(inner);
        self.state.changed.notify_one();
        InspectionAdmissionOutcomeV1::Accepted {
            emission_id,
            sequence,
        }
    }
}

/// Sole post-install registration-bound consumer authority.
#[must_use]
pub struct ActivatedInspectionConsumerV1 {
    state: Arc<RegistrationState>,
    _single_owner: PhantomData<Cell<()>>,
}

impl ActivatedInspectionConsumerV1 {
    /// Begins a reusable purge and returns its self-bound completion guard.
    pub fn begin_purge(&mut self) -> Result<InspectionPurgeGuardV1, InspectionPurgeErrorV1> {
        let mut inner = lock_without_recovery(&self.state.inner);
        let (from, to) = match inner.lifecycle {
            LifecycleStateV1::Running(from) => match from.checked_add(1) {
                Some(to) => (from, to),
                None => {
                    inner.lifecycle = LifecycleStateV1::Disabled;
                    inner.closed = true;
                    inner.queue.clear();
                    self.state.changed.notify_all();
                    return Err(InspectionPurgeErrorV1::EpochOverflow);
                }
            },
            LifecycleStateV1::Purging { .. } => return Err(InspectionPurgeErrorV1::AlreadyPurging),
            LifecycleStateV1::Disabled => return Err(InspectionPurgeErrorV1::Disabled),
        };
        inner.lifecycle = LifecycleStateV1::Purging { from, to };
        inner.queue.clear();
        drop(inner);
        self.state.changed.notify_all();
        Ok(InspectionPurgeGuardV1 {
            state: self.state.clone(),
            from,
            to,
            completed: false,
        })
    }

    /// Performs the idempotent one-way disable transition and drains queued events.
    pub fn disable(&mut self) -> InspectionDisableOutcomeV1 {
        self.state.disable()
    }
}

impl Drop for ActivatedInspectionConsumerV1 {
    fn drop(&mut self) {
        self.state.disable();
    }
}

/// Registration-bound purge completion guard.
#[must_use]
pub struct InspectionPurgeGuardV1 {
    state: Arc<RegistrationState>,
    from: u64,
    to: u64,
    completed: bool,
}

impl InspectionPurgeGuardV1 {
    /// Returns the runtime-selected next epoch; callers cannot choose it.
    #[must_use]
    pub const fn next_epoch(&self) -> InspectionEpochV1 {
        InspectionEpochV1::new(self.to)
    }

    /// Consumes the matching guard and reopens only its own registration.
    pub fn complete(mut self) -> Result<InspectionEpochV1, InspectionPurgeErrorV1> {
        {
            let mut inner = lock_without_recovery(&self.state.inner);
            match inner.lifecycle {
                LifecycleStateV1::Purging { from, to } if from == self.from && to == self.to => {
                    inner.lifecycle = LifecycleStateV1::Running(to);
                }
                LifecycleStateV1::Disabled => return Err(InspectionPurgeErrorV1::Disabled),
                LifecycleStateV1::Running(_) | LifecycleStateV1::Purging { .. } => {
                    return Err(InspectionPurgeErrorV1::MismatchedGuard)
                }
            }
        }
        self.completed = true;
        self.state.changed.notify_all();
        Ok(InspectionEpochV1::new(self.to))
    }
}

impl Drop for InspectionPurgeGuardV1 {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let mut inner = lock_without_recovery(&self.state.inner);
        if matches!(
            inner.lifecycle,
            LifecycleStateV1::Purging { from, to } if from == self.from && to == self.to
        ) {
            inner.lifecycle = LifecycleStateV1::Disabled;
            inner.closed = true;
            inner.queue.clear();
            self.state.changed.notify_all();
        }
    }
}

/// Closed purge lifecycle failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionPurgeErrorV1 {
    AlreadyPurging,
    Disabled,
    EpochOverflow,
    MismatchedGuard,
}

/// Closed idempotent disable result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionDisableOutcomeV1 {
    Disabled,
    AlreadyDisabled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_types::inspection::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Condvar, Mutex};
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<
            Vec<(
                InspectionSequenceV1,
                InspectionEventKindV1,
                InspectionEmissionIdV1,
                LogicalInspectionIdV1,
            )>,
        >,
        changed: Condvar,
    }

    impl RecordingSink {
        fn wait_for(&self, count: usize) {
            let guard = self.events.lock().unwrap();
            let (guard, timeout) = self
                .changed
                .wait_timeout_while(guard, Duration::from_secs(2), |events| events.len() < count)
                .unwrap();
            assert!(!timeout.timed_out(), "inspection delivery timed out");
            assert!(guard.len() >= count);
        }
    }

    impl InspectionSink for RecordingSink {
        fn try_emit(&self, event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
            self.events.lock().unwrap().push((
                event.meta().sequence(),
                event.kind(),
                event.meta().emission_id(),
                event.meta().logical_id(),
            ));
            self.changed.notify_all();
            Ok(())
        }
    }

    fn safe_fields() -> InspectionSafeFieldsV1 {
        InspectionSafeFieldsV1::new(
            RouteCodeV1::AnthropicMessages,
            ProviderProfileCodeV1::AnthropicMessagesDirect,
            EndpointSelectionCodeV1::FixedHttpsOrigin,
            PortSelectionCodeV1::NotApplicable,
            InspectionOperationalStatusV1::Accepted,
            InspectionErrorCodeV1::None,
            InspectionDropCodeV1::None,
            InspectionDeliveryCodeV1::Pending,
            CoarseDurationBucketV1::NotMeasured,
            InspectionQueueSnapshotV1::default(),
        )
    }

    fn provider_projection(bytes: &[u8]) -> ProviderVisibleProjectionV1 {
        ProviderVisibleProjectionV1::new(
            ProjectionAvailabilityV1::Present(ProviderVisiblePayloadV1::capture(bytes.to_vec())),
            ProjectionAvailabilityV1::Present(InspectionMeasurementV1::new(bytes.len() as u64, 1)),
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::UnsupportedFormat),
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
            ProjectionAvailabilityV1::Present(PiiSummaryV1::empty()),
            ProjectionAvailabilityV1::Present(DecisionTraceV1::empty()),
            ProjectionAvailabilityV1::Present(AttestationTraceV1::NotApplicable),
        )
    }

    fn owner_raw_projection(bytes: &[u8]) -> OwnerRawProjectionV1 {
        OwnerRawProjectionV1::new(
            ProjectionAvailabilityV1::Present(OwnerRawPayloadV1::capture(bytes.to_vec())),
            ProjectionAvailabilityV1::Present(InspectionMeasurementV1::new(bytes.len() as u64, 1)),
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::UnsupportedFormat),
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
            ProjectionAvailabilityV1::Present(PiiSummaryV1::empty()),
            ProjectionAvailabilityV1::Present(DecisionTraceV1::empty()),
            ProjectionAvailabilityV1::Present(AttestationTraceV1::NotApplicable),
        )
    }

    fn owner_restored_projection(bytes: &[u8]) -> OwnerRestoredProjectionV1 {
        OwnerRestoredProjectionV1::new(
            ProjectionAvailabilityV1::Present(OwnerRestoredPayloadV1::capture(bytes.to_vec())),
            ProjectionAvailabilityV1::Present(InspectionMeasurementV1::new(bytes.len() as u64, 1)),
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::UnsupportedFormat),
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
            ProjectionAvailabilityV1::Present(PiiSummaryV1::empty()),
            ProjectionAvailabilityV1::Present(DecisionTraceV1::empty()),
            ProjectionAvailabilityV1::Present(AttestationTraceV1::NotApplicable),
        )
    }

    fn begin_logical_for_test(
        producer: &InstalledInspectionProducerV1,
    ) -> InspectionLogicalEmitterV1 {
        const DEADLINE: Duration = Duration::from_millis(100);
        let started = Instant::now();
        loop {
            match producer.begin_logical() {
                Ok(logical) => return logical,
                Err(InspectionBeginLogicalErrorV1::Contended) if started.elapsed() < DEADLINE => {
                    std::thread::yield_now();
                }
                Err(InspectionBeginLogicalErrorV1::Contended) => {
                    panic!(
                        "begin_logical remained contended beyond the {DEADLINE:?} test setup deadline"
                    )
                }
                Err(error) => panic!("begin_logical failed during test setup: {error:?}"),
            }
        }
    }

    fn admit_for_test<F>(mut attempt: F) -> InspectionAdmissionOutcomeV1
    where
        F: FnMut() -> InspectionAdmissionOutcomeV1,
    {
        const DEADLINE: Duration = Duration::from_millis(100);
        let started = Instant::now();
        loop {
            match attempt() {
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueContended)
                    if started.elapsed() < DEADLINE =>
                {
                    std::thread::yield_now();
                }
                InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueContended) => {
                    panic!(
                        "inspection admission remained contended beyond the {DEADLINE:?} test deadline"
                    )
                }
                outcome => return outcome,
            }
        }
    }

    impl InspectionLogicalEmitterV1 {
        fn try_emit_owner_request_for_test<F>(
            &mut self,
            safe: InspectionSafeFieldsV1,
            mut build: F,
        ) -> InspectionAdmissionOutcomeV1
        where
            F: FnMut() -> ProjectionAvailabilityV1<OwnerRawProjectionV1>,
        {
            admit_for_test(|| self.try_emit_owner_request(safe, &mut build))
        }

        fn try_emit_provider_request_for_test<F>(
            &mut self,
            safe: InspectionSafeFieldsV1,
            mut build: F,
        ) -> InspectionAdmissionOutcomeV1
        where
            F: FnMut() -> ProjectionAvailabilityV1<ProviderVisibleProjectionV1>,
        {
            admit_for_test(|| self.try_emit_provider_request(safe, &mut build))
        }

        fn try_emit_provider_response_for_test<F>(
            &mut self,
            safe: InspectionSafeFieldsV1,
            mut build: F,
        ) -> InspectionAdmissionOutcomeV1
        where
            F: FnMut() -> ProjectionAvailabilityV1<ProviderVisibleProjectionV1>,
        {
            admit_for_test(|| self.try_emit_provider_response(safe, &mut build))
        }

        fn try_emit_owner_restored_response_for_test<F>(
            &mut self,
            safe: InspectionSafeFieldsV1,
            mut build: F,
        ) -> InspectionAdmissionOutcomeV1
        where
            F: FnMut() -> ProjectionAvailabilityV1<OwnerRestoredProjectionV1>,
        {
            admit_for_test(|| self.try_emit_owner_restored_response(safe, &mut build))
        }
    }

    fn wait_for_accounting(state: &RegistrationState, items: usize, bytes: usize) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let observed = (
                state.accounting.items.load(Ordering::Acquire),
                state.accounting.bytes.load(Ordering::Acquire),
            );
            if observed == (items, bytes) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "queue accounting did not converge"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[derive(Default)]
    struct BlockingSink {
        entered: Mutex<bool>,
        entered_changed: Condvar,
        released: Mutex<bool>,
        released_changed: Condvar,
    }

    impl BlockingSink {
        fn wait_until_entered(&self) {
            let guard = self.entered.lock().unwrap();
            let (guard, timeout) = self
                .entered_changed
                .wait_timeout_while(guard, Duration::from_secs(2), |entered| !*entered)
                .unwrap();
            assert!(!timeout.timed_out());
            assert!(*guard);
        }

        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.released_changed.notify_all();
        }
    }

    impl InspectionSink for BlockingSink {
        fn try_emit(&self, _event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
            *self.entered.lock().unwrap() = true;
            self.entered_changed.notify_all();
            let guard = self.released.lock().unwrap();
            drop(
                self.released_changed
                    .wait_while(guard, |released| !*released)
                    .unwrap(),
            );
            Ok(())
        }
    }

    struct PanicSink;

    impl InspectionSink for PanicSink {
        fn try_emit(&self, _event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
            panic!("synthetic inspection sink panic")
        }
    }

    #[derive(Default)]
    struct ReentrantSink {
        emitter: Mutex<Option<InspectionLogicalEmitterV1>>,
        calls: Mutex<usize>,
        changed: Condvar,
    }

    impl ReentrantSink {
        fn wait_for(&self, count: usize) {
            let guard = self.calls.lock().unwrap();
            let (guard, timeout) = self
                .changed
                .wait_timeout_while(guard, Duration::from_secs(2), |calls| *calls < count)
                .unwrap();
            assert!(!timeout.timed_out());
            assert!(*guard >= count);
        }
    }

    impl InspectionSink for ReentrantSink {
        fn try_emit(&self, _event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
            let should_reenter = {
                let mut calls = self.calls.lock().unwrap();
                let should_reenter = *calls == 0;
                *calls += 1;
                self.changed.notify_all();
                should_reenter
            };
            if should_reenter {
                let outcome = self
                    .emitter
                    .lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .try_emit_provider_response_for_test(safe_fields(), || {
                        ProjectionAvailabilityV1::Present(provider_projection(b"<Email_2>"))
                    });
                assert!(matches!(
                    outcome,
                    InspectionAdmissionOutcomeV1::Accepted { .. }
                ));
            }
            Ok(())
        }
    }

    #[test]
    fn atomic_install_rejects_descriptor_mismatch() {
        let producer = PendingInspectionProducerV1::new(DashboardCaptureDescriptorV1::new(
            CaptureDomainsV1::ProviderVisible,
        ));
        let consumer = PendingInspectionConsumerV1::new(
            DashboardCaptureDescriptorV1::new(CaptureDomainsV1::MetadataOnly),
            Arc::new(RecordingSink::default()),
            InspectionQueueLimitsV1::new(4, 1024).unwrap(),
        );
        assert!(matches!(
            install_inspection_v1(producer, consumer),
            Err(InspectionInstallErrorV1::DescriptorMismatch)
        ));
    }

    #[test]
    fn producer_and_consumer_capabilities_have_the_required_thread_traits() {
        fn require_send<T: Send>() {}
        fn require_send_sync<T: Send + Sync>() {}
        require_send_sync::<InstalledInspectionProducerV1>();
        require_send::<ActivatedInspectionConsumerV1>();
    }

    #[test]
    fn metadata_only_skips_projection_before_allocation() {
        let sink = Arc::new(RecordingSink::default());
        let producer = PendingInspectionProducerV1::new(DashboardCaptureDescriptorV1::new(
            CaptureDomainsV1::MetadataOnly,
        ));
        let consumer = PendingInspectionConsumerV1::new(
            DashboardCaptureDescriptorV1::new(CaptureDomainsV1::MetadataOnly),
            sink.clone(),
            InspectionQueueLimitsV1::new(4, 1024).unwrap(),
        );
        let (producer, _consumer) = install_inspection_v1(producer, consumer).unwrap();
        let mut logical = begin_logical_for_test(&producer);
        let projected = AtomicUsize::new(0);
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                projected.fetch_add(1, Ordering::SeqCst);
                ProjectionAvailabilityV1::Present(provider_projection(b"alice@example.invalid"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        sink.wait_for(1);
        assert_eq!(projected.load(Ordering::SeqCst), 0);
        assert_eq!(
            sink.events.lock().unwrap()[0].1,
            InspectionEventKindV1::Omitted {
                stage_domain: InspectionStageDomainV1::ProviderRequest,
                reason: ProjectionOmissionReasonV1::NotCapturedByPolicy,
            }
        );
    }

    #[test]
    fn every_startup_combination_selects_projection_before_callback() {
        for domains in CaptureDomainsV1::ALL {
            let descriptor = DashboardCaptureDescriptorV1::new(domains);
            let sink = Arc::new(RecordingSink::default());
            let (producer, _consumer) = install_inspection_v1(
                PendingInspectionProducerV1::new(descriptor),
                PendingInspectionConsumerV1::new(
                    descriptor,
                    sink.clone(),
                    InspectionQueueLimitsV1::new(8, 16 * 1024).unwrap(),
                ),
            )
            .unwrap();
            let mut logical = begin_logical_for_test(&producer);
            let raw_calls = AtomicUsize::new(0);
            let provider_calls = AtomicUsize::new(0);
            let restored_calls = AtomicUsize::new(0);
            let owner_request = logical.try_emit_owner_request_for_test(safe_fields(), || {
                raw_calls.fetch_add(1, Ordering::SeqCst);
                ProjectionAvailabilityV1::Present(owner_raw_projection(b"alice@example.invalid"))
            });
            assert!(matches!(
                owner_request,
                InspectionAdmissionOutcomeV1::Accepted { .. }
            ));
            let provider_request =
                logical.try_emit_provider_request_for_test(safe_fields(), || {
                    provider_calls.fetch_add(1, Ordering::SeqCst);
                    ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
                });
            assert!(matches!(
                provider_request,
                InspectionAdmissionOutcomeV1::Accepted { .. }
            ));
            let owner_restored_response =
                logical.try_emit_owner_restored_response_for_test(safe_fields(), || {
                    restored_calls.fetch_add(1, Ordering::SeqCst);
                    ProjectionAvailabilityV1::Present(owner_restored_projection(
                        b"alice@example.invalid",
                    ))
                });
            assert!(matches!(
                owner_restored_response,
                InspectionAdmissionOutcomeV1::Accepted { .. }
            ));
            sink.wait_for(3);
            assert_eq!(
                raw_calls.load(Ordering::SeqCst) > 0,
                domains.captures(InspectionDomainV1::OwnerRaw)
            );
            assert_eq!(
                provider_calls.load(Ordering::SeqCst) > 0,
                domains.captures(InspectionDomainV1::ProviderVisible)
            );
            assert_eq!(
                restored_calls.load(Ordering::SeqCst) > 0,
                domains.captures(InspectionDomainV1::OwnerRestored)
            );
        }
    }

    #[test]
    fn sequences_are_strictly_monotonic_per_logical_id() {
        let sink = Arc::new(RecordingSink::default());
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink.clone(),
                InspectionQueueLimitsV1::new(8, 4096).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        for _ in 0..3 {
            let outcome = logical.try_emit_provider_response_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            });
            assert!(matches!(
                outcome,
                InspectionAdmissionOutcomeV1::Accepted { .. }
            ));
        }
        sink.wait_for(3);
        let sequences: Vec<_> = sink
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|(sequence, ..)| sequence.get())
            .collect();
        assert_eq!(sequences, [1, 2, 3]);
        let events = sink.events.lock().unwrap();
        assert!(events.windows(2).all(|pair| pair[0].2 != pair[1].2));
        assert!(events.iter().all(|event| event.3 == events[0].3));
    }

    #[test]
    fn equal_descriptor_installations_remain_isolated() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let sink_a = Arc::new(RecordingSink::default());
        let sink_b = Arc::new(RecordingSink::default());
        let (producer_a, mut consumer_a) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink_a.clone(),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let (producer_b, _consumer_b) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink_b.clone(),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let mut logical_a = begin_logical_for_test(&producer_a);
        let mut logical_b = begin_logical_for_test(&producer_b);
        let guard = consumer_a.begin_purge().unwrap();
        assert!(matches!(
            logical_a.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Purging)
        ));
        assert!(matches!(
            logical_b.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        sink_b.wait_for(1);
        guard.complete().unwrap();
        assert!(sink_a.events.lock().unwrap().is_empty());
    }

    #[test]
    fn sequence_overflow_fails_closed_before_projection() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                Arc::new(RecordingSink::default()),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        logical.sequence = InspectionSequenceV1::new(u64::MAX);
        let calls = AtomicUsize::new(0);
        assert_eq!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::SequenceOverflow)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn projection_panic_is_caught_and_later_emission_can_continue() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let sink = Arc::new(RecordingSink::default());
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink.clone(),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        assert_eq!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                panic!("synthetic projection panic")
            }),
            InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::PartialEvent)
        );
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        sink.wait_for(1);
        assert_eq!(sink.events.lock().unwrap()[0].0.get(), 1);
    }

    #[test]
    fn begin_logical_returns_contended_while_registration_lock_is_held() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                Arc::new(RecordingSink::default()),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let locked = lock_without_recovery(&producer.state.inner);
        assert!(matches!(
            producer.begin_logical(),
            Err(InspectionBeginLogicalErrorV1::Contended)
        ));
        drop(locked);
    }

    #[test]
    fn queue_contention_drops_before_projection_without_waiting() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                Arc::new(RecordingSink::default()),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        let locked = lock_without_recovery(&producer.state.inner);
        let calls = AtomicUsize::new(0);
        assert_eq!(
            logical.try_emit_provider_request(safe_fields(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueContended)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(locked);
    }

    #[test]
    fn byte_limit_drop_zeroizes_partial_projection_and_conserves_accounting() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                Arc::new(RecordingSink::default()),
                InspectionQueueLimitsV1::new(4, 8).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        let zeroized = Arc::new(AtomicBool::new(false));
        assert_eq!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(ProviderVisibleProjectionV1::new(
                    ProjectionAvailabilityV1::Present(
                        ProviderVisiblePayloadV1::capture_with_probe(
                            b"<Email_123>".to_vec(),
                            zeroized.clone(),
                        ),
                    ),
                    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
                    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
                    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
                    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
                    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
                    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
                ))
            }),
            InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueByteLimit)
        );
        assert!(zeroized.load(Ordering::SeqCst));
        assert_eq!(producer.state.accounting.items.load(Ordering::Acquire), 0);
        assert_eq!(producer.state.accounting.bytes.load(Ordering::Acquire), 0);
    }

    #[test]
    fn queue_full_and_blocked_sink_never_wait_on_producer_admission() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let sink = Arc::new(BlockingSink::default());
        let (producer, mut consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink.clone(),
                InspectionQueueLimitsV1::new(2, 4096).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        sink.wait_until_entered();
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_2>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        assert_eq!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_3>"))
            }),
            InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueFull)
        );
        assert_eq!(consumer.disable(), InspectionDisableOutcomeV1::Disabled);
        assert_eq!(
            consumer.disable(),
            InspectionDisableOutcomeV1::AlreadyDisabled
        );
        sink.release();
        wait_for_accounting(&producer.state, 0, 0);
    }

    #[test]
    fn purge_drain_and_payload_drop_zeroize_and_release_one_reservation() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let sink = Arc::new(BlockingSink::default());
        let (producer, mut consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink.clone(),
                InspectionQueueLimitsV1::new(3, 4096).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        sink.wait_until_entered();
        let zeroized = Arc::new(AtomicBool::new(false));
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(ProviderVisibleProjectionV1::new(
                    ProjectionAvailabilityV1::Present(
                        ProviderVisiblePayloadV1::capture_with_probe(
                            b"<Email_2>".to_vec(),
                            zeroized.clone(),
                        ),
                    ),
                    ProjectionAvailabilityV1::Present(InspectionMeasurementV1::new(9, 1)),
                    ProjectionAvailabilityV1::Omitted(
                        ProjectionOmissionReasonV1::UnsupportedFormat,
                    ),
                    ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
                    ProjectionAvailabilityV1::Present(PiiSummaryV1::empty()),
                    ProjectionAvailabilityV1::Present(DecisionTraceV1::empty()),
                    ProjectionAvailabilityV1::Present(AttestationTraceV1::NotApplicable),
                ))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        let guard = consumer.begin_purge().unwrap();
        assert!(zeroized.load(Ordering::SeqCst));
        assert_eq!(producer.state.accounting.items.load(Ordering::Acquire), 1);
        sink.release();
        wait_for_accounting(&producer.state, 0, 0);
        guard.complete().unwrap();
    }

    #[test]
    fn every_sensitive_domain_zeroizes_on_drop() {
        let raw = Arc::new(AtomicBool::new(false));
        let provider = Arc::new(AtomicBool::new(false));
        let restored = Arc::new(AtomicBool::new(false));
        drop(OwnerRawPayloadV1::capture_with_probe(
            b"alice@example.invalid".to_vec(),
            raw.clone(),
        ));
        drop(ProviderVisiblePayloadV1::capture_with_probe(
            b"<Email_1>".to_vec(),
            provider.clone(),
        ));
        drop(OwnerRestoredPayloadV1::capture_with_probe(
            b"alice@example.invalid".to_vec(),
            restored.clone(),
        ));
        assert!(raw.load(Ordering::SeqCst));
        assert!(provider.load(Ordering::SeqCst));
        assert!(restored.load(Ordering::SeqCst));
    }

    #[test]
    fn dropped_purge_guard_disables_without_implicit_resume() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, mut consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                Arc::new(RecordingSink::default()),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        drop(consumer.begin_purge().unwrap());
        let calls = AtomicUsize::new(0);
        assert_eq!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::Disabled)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            consumer.begin_purge(),
            Err(InspectionPurgeErrorV1::Disabled)
        ));
    }

    #[test]
    fn epoch_overflow_disables_and_drains_fail_closed() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, mut consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                Arc::new(RecordingSink::default()),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        lock_without_recovery(&producer.state.inner).lifecycle =
            LifecycleStateV1::Running(u64::MAX);
        assert!(matches!(
            consumer.begin_purge(),
            Err(InspectionPurgeErrorV1::EpochOverflow)
        ));
        assert_eq!(
            consumer.disable(),
            InspectionDisableOutcomeV1::AlreadyDisabled
        );
    }

    #[test]
    fn dropping_installed_producer_closes_existing_logical_emitters() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                Arc::new(RecordingSink::default()),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        drop(producer);
        let calls = AtomicUsize::new(0);
        assert_eq!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Dropped(InspectionDropCodeV1::QueueClosed)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn post_dequeue_fence_drops_when_purge_wins_before_permit() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let sink = Arc::new(RecordingSink::default());
        let (producer, mut consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink.clone(),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let barrier = Arc::new(Barrier::new(2));
        lock_without_recovery(&producer.state.hooks).after_dequeue = Some(barrier.clone());
        let mut logical = begin_logical_for_test(&producer);
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        barrier.wait();
        let guard = consumer.begin_purge().unwrap();
        barrier.wait();
        wait_for_accounting(&producer.state, 0, 0);
        assert!(sink.events.lock().unwrap().is_empty());
        guard.complete().unwrap();
    }

    #[test]
    fn synchronized_permit_is_the_delivery_linearization_point() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let sink = Arc::new(RecordingSink::default());
        let (producer, mut consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink.clone(),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let barrier = Arc::new(Barrier::new(2));
        lock_without_recovery(&producer.state.hooks).after_permit = Some(barrier.clone());
        let mut logical = begin_logical_for_test(&producer);
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        barrier.wait();
        let guard = consumer.begin_purge().unwrap();
        barrier.wait();
        sink.wait_for(1);
        guard.complete().unwrap();
    }

    #[test]
    fn sink_panic_is_contained_to_dispatcher_and_releases_accounting() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                Arc::new(PanicSink),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        wait_for_accounting(&producer.state, 0, 0);
        assert_eq!(lock_without_recovery(&producer.state.inner).sink_panics, 1);
    }

    #[test]
    fn sink_reentry_never_runs_under_the_queue_lock() {
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::ProviderVisible);
        let sink = Arc::new(ReentrantSink::default());
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink.clone(),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        *sink.emitter.lock().unwrap() = Some(begin_logical_for_test(&producer));
        let mut logical = begin_logical_for_test(&producer);
        assert!(matches!(
            logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(b"<Email_1>"))
            }),
            InspectionAdmissionOutcomeV1::Accepted { .. }
        ));
        sink.wait_for(2);
        wait_for_accounting(&producer.state, 0, 0);
    }

    #[test]
    fn metadata_only_serialization_is_content_invariant_after_id_normalization() {
        #[derive(Default)]
        struct MetadataSink {
            values: Mutex<Vec<serde_json::Value>>,
            changed: Condvar,
        }
        impl InspectionSink for MetadataSink {
            fn try_emit(&self, event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
                assert!(matches!(
                    event.view(),
                    InspectionEventViewV1::Omitted {
                        reason: ProjectionOmissionReasonV1::NotCapturedByPolicy,
                        ..
                    }
                ));
                self.values
                    .lock()
                    .unwrap()
                    .push(serde_json::to_value(event.meta()).unwrap());
                self.changed.notify_all();
                Ok(())
            }
        }
        let sink = Arc::new(MetadataSink::default());
        let descriptor = DashboardCaptureDescriptorV1::new(CaptureDomainsV1::MetadataOnly);
        let (producer, _consumer) = install_inspection_v1(
            PendingInspectionProducerV1::new(descriptor),
            PendingInspectionConsumerV1::new(
                descriptor,
                sink.clone(),
                InspectionQueueLimitsV1::new(4, 1024).unwrap(),
            ),
        )
        .unwrap();
        let mut logical = begin_logical_for_test(&producer);
        for content in [
            b"a".as_slice(),
            b"alice@example.invalid and more".as_slice(),
        ] {
            let outcome = logical.try_emit_provider_request_for_test(safe_fields(), || {
                ProjectionAvailabilityV1::Present(provider_projection(content))
            });
            assert!(matches!(
                outcome,
                InspectionAdmissionOutcomeV1::Accepted { .. }
            ));
        }
        let guard = sink.values.lock().unwrap();
        let (mut guard, timeout) = sink
            .changed
            .wait_timeout_while(guard, Duration::from_secs(2), |values| values.len() < 2)
            .unwrap();
        assert!(!timeout.timed_out());
        for value in &mut *guard {
            let object = value.as_object_mut().unwrap();
            object.remove("logical_id");
            object.remove("emission_id");
            object.remove("sequence");
        }
        assert_eq!(guard[0], guard[1]);
    }
}
