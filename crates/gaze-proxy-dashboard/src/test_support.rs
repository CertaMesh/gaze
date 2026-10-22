//! Test-only producer harness: emits real `InspectionEventV1` values through the
//! installed gaze-inspection runtime so tests exercise the production IPC encoder,
//! decoder, store, and snapshot encoder end to end.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gaze_inspection::{
    install_inspection_v1, InspectionAdmissionOutcomeV1, InspectionEventV1,
    InspectionQueueLimitsV1, InspectionSink, InspectionSinkErrorV1, OwnerRawPayloadV1,
    OwnerRawProjectionV1, OwnerRestoredPayloadV1, OwnerRestoredProjectionV1,
    PendingInspectionConsumerV1, PendingInspectionProducerV1, ProviderVisiblePayloadV1,
    ProviderVisibleProjectionV1,
};
use gaze_types::inspection::{
    AttestationTraceV1, CaptureDomainsV1, CoarseDurationBucketV1, ContentBlockIndexV1,
    DashboardCaptureDescriptorV1, DecisionEngineCodeV1, DecisionOutcomeCodeV1,
    DecisionTraceEntryV1, DecisionTraceV1, EndpointSelectionCodeV1, InspectionDeliveryCodeV1,
    InspectionDropCodeV1, InspectionEpochV1, InspectionErrorCodeV1, InspectionMeasurementV1,
    InspectionOperationalStatusV1, InspectionQueueSnapshotV1, InspectionSafeFieldsV1,
    InspectionStageDomainV1, InspectionSurfaceRefV1, JsonNodeOrdinalV1, JsonShapeNodeV1,
    JsonShapeSummaryV1, JsonShapeTruncationCodeV1, JsonTypeCodeV1, PiiClassCodeV1, PiiCountV1,
    PiiSummaryV1, PortSelectionCodeV1, ProjectionAvailabilityV1, ProjectionOmissionReasonV1,
    ProviderProfileCodeV1, RouteCodeV1, SseDeltaKindV1, SseEntryOrdinalV1, SseEventKindV1,
    SseTimelineEntryV1, SseTimelineMetaV1,
};

use crate::ipc::{DecodedInspectionFrame, EncodedInspectionFrame};
use crate::store::{AdmissionOutcome, EventStore};
use crate::RetentionLimits;

/// Every closed omission reason, in declaration order.
pub(crate) const ALL_REASONS: [ProjectionOmissionReasonV1; 11] = [
    ProjectionOmissionReasonV1::NotCapturedByPolicy,
    ProjectionOmissionReasonV1::NotApplicable,
    ProjectionOmissionReasonV1::UnsupportedFormat,
    ProjectionOmissionReasonV1::MalformedFailClosed,
    ProjectionOmissionReasonV1::LimitExceeded,
    ProjectionOmissionReasonV1::SignedOrEncryptedSurface,
    ProjectionOmissionReasonV1::Purging,
    ProjectionOmissionReasonV1::Disabled,
    ProjectionOmissionReasonV1::QueueClosed,
    ProjectionOmissionReasonV1::ProjectionFailedClosed,
    ProjectionOmissionReasonV1::UnknownFuture,
];

/// Declaration position of each reason. The match is exhaustive, so a new upstream
/// variant fails to compile here until it is given a position and added to
/// [`ALL_REASONS`]; the test below then proves the list is complete and ordered.
const fn reason_position(reason: ProjectionOmissionReasonV1) -> usize {
    match reason {
        ProjectionOmissionReasonV1::NotCapturedByPolicy => 0,
        ProjectionOmissionReasonV1::NotApplicable => 1,
        ProjectionOmissionReasonV1::UnsupportedFormat => 2,
        ProjectionOmissionReasonV1::MalformedFailClosed => 3,
        ProjectionOmissionReasonV1::LimitExceeded => 4,
        ProjectionOmissionReasonV1::SignedOrEncryptedSurface => 5,
        ProjectionOmissionReasonV1::Purging => 6,
        ProjectionOmissionReasonV1::Disabled => 7,
        ProjectionOmissionReasonV1::QueueClosed => 8,
        ProjectionOmissionReasonV1::ProjectionFailedClosed => 9,
        ProjectionOmissionReasonV1::UnknownFuture => 10,
    }
}

#[test]
fn all_reasons_lists_every_closed_omission_reason_once() {
    for (index, reason) in ALL_REASONS.iter().enumerate() {
        assert_eq!(reason_position(*reason), index, "{reason:?}");
    }
}

/// Every stage, in pipeline order.
pub(crate) const ALL_STAGES: [InspectionStageDomainV1; 4] = [
    InspectionStageDomainV1::OwnerRequest,
    InspectionStageDomainV1::ProviderRequest,
    InspectionStageDomainV1::ProviderResponse,
    InspectionStageDomainV1::OwnerRestoredResponse,
];

/// Payload sentinels. They must never reach a metadata stream.
pub(crate) const OWNER_RAW_SENTINEL: &str =
    "OWNER-RAW-SENTINEL-7Q4 Dr. Schmidt alice@example.invalid";
pub(crate) const PROVIDER_SENTINEL: &str = "PSEUDO-PAYLOAD-SENTINEL-5B8 <Email_1>";
pub(crate) const OWNER_RESTORED_SENTINEL: &str = "OWNER-RESTORED-SENTINEL-9K2 +1-555-0142";

pub(crate) fn sentinel_for(stage: InspectionStageDomainV1) -> &'static str {
    match stage {
        InspectionStageDomainV1::OwnerRequest => OWNER_RAW_SENTINEL,
        InspectionStageDomainV1::ProviderRequest | InspectionStageDomainV1::ProviderResponse => {
            PROVIDER_SENTINEL
        }
        InspectionStageDomainV1::OwnerRestoredResponse => OWNER_RESTORED_SENTINEL,
    }
}

/// How one emitted stage projection is shaped.
#[derive(Clone, Copy, Debug)]
pub(crate) enum StageShape {
    /// Payload and every component are present.
    Present,
    /// Payload is present but zero bytes long (an empty body is still captured).
    PresentEmptyPayload,
    /// The projection is present, but its payload is omitted with this reason; the
    /// components are omitted with the same reason.
    PayloadOmitted(ProjectionOmissionReasonV1),
    /// The whole projection is omitted with this reason.
    ProjectionOmitted(ProjectionOmissionReasonV1),
}

pub(crate) fn safe_fields() -> InspectionSafeFieldsV1 {
    InspectionSafeFieldsV1::new(
        RouteCodeV1::AnthropicMessages,
        ProviderProfileCodeV1::OpenAiCompatible,
        EndpointSelectionCodeV1::OsAssignedLoopback,
        PortSelectionCodeV1::OsAssigned,
        InspectionOperationalStatusV1::Accepted,
        InspectionErrorCodeV1::None,
        InspectionDropCodeV1::None,
        InspectionDeliveryCodeV1::Pending,
        CoarseDurationBucketV1::UnderOneHundredMilliseconds,
        InspectionQueueSnapshotV1::new(3, 1, 2),
    )
}

/// Every projection component of one stage, in constructor order.
type ProjectionComponents = (
    ProjectionAvailabilityV1<InspectionMeasurementV1>,
    ProjectionAvailabilityV1<JsonShapeSummaryV1>,
    ProjectionAvailabilityV1<SseTimelineMetaV1>,
    ProjectionAvailabilityV1<PiiSummaryV1>,
    ProjectionAvailabilityV1<DecisionTraceV1>,
    ProjectionAvailabilityV1<AttestationTraceV1>,
);

fn present_components() -> ProjectionComponents {
    use ProjectionAvailabilityV1::{Omitted, Present};
    (
        Present(InspectionMeasurementV1::new(1_421, 3)),
        Present(
            JsonShapeSummaryV1::try_new(
                vec![
                    JsonShapeNodeV1::new(JsonNodeOrdinalV1::new(0), JsonTypeCodeV1::Object),
                    JsonShapeNodeV1::new(JsonNodeOrdinalV1::new(1), JsonTypeCodeV1::String),
                ],
                JsonShapeTruncationCodeV1::Complete,
            )
            .unwrap(),
        ),
        Present(
            SseTimelineMetaV1::try_new(vec![
                SseTimelineEntryV1::new(
                    SseEntryOrdinalV1::new(0),
                    SseEventKindV1::MessageStart,
                    Omitted(ProjectionOmissionReasonV1::NotApplicable),
                    Omitted(ProjectionOmissionReasonV1::NotApplicable),
                ),
                SseTimelineEntryV1::new(
                    SseEntryOrdinalV1::new(1),
                    SseEventKindV1::ContentBlockDelta,
                    Present(SseDeltaKindV1::InputJson),
                    Present(ContentBlockIndexV1::new(0)),
                ),
            ])
            .unwrap(),
        ),
        Present(PiiSummaryV1::try_new(vec![PiiCountV1::new(PiiClassCodeV1::Email, 1, 1)]).unwrap()),
        Present(
            DecisionTraceV1::try_new(vec![
                DecisionTraceEntryV1::new(
                    DecisionEngineCodeV1::DeterministicRecognizer,
                    DecisionOutcomeCodeV1::RanFindings,
                    InspectionSurfaceRefV1::WholeBody,
                    Present(PiiClassCodeV1::Email),
                    1,
                    1,
                ),
                DecisionTraceEntryV1::new(
                    DecisionEngineCodeV1::Ner,
                    DecisionOutcomeCodeV1::RanClean,
                    InspectionSurfaceRefV1::WholeBody,
                    Omitted(ProjectionOmissionReasonV1::NotApplicable),
                    0,
                    0,
                ),
            ])
            .unwrap(),
        ),
        Present(AttestationTraceV1::ExactHit),
    )
}

macro_rules! build_projection {
    ($projection:ident, $payload:ident, $shape:expr, $sentinel:expr) => {{
        use ProjectionAvailabilityV1::{Omitted, Present};
        match $shape {
            StageShape::Present => {
                let (m, j, s, p, d, a) = present_components();
                Present($projection::new(
                    Present($payload::capture($sentinel.as_bytes().to_vec())),
                    m,
                    j,
                    s,
                    p,
                    d,
                    a,
                ))
            }
            StageShape::PresentEmptyPayload => {
                let (m, j, s, p, d, a) = present_components();
                Present($projection::new(
                    Present($payload::capture(Vec::new())),
                    m,
                    j,
                    s,
                    p,
                    d,
                    a,
                ))
            }
            StageShape::PayloadOmitted(reason) => Present($projection::new(
                Omitted(reason),
                Omitted(reason),
                Omitted(reason),
                Omitted(reason),
                Omitted(reason),
                Omitted(reason),
                Omitted(reason),
            )),
            StageShape::ProjectionOmitted(reason) => Omitted(reason),
        }
    }};
}

struct ChannelSink(Mutex<Sender<InspectionEventV1>>);

impl InspectionSink for ChannelSink {
    fn try_emit(&self, event: InspectionEventV1) -> Result<(), InspectionSinkErrorV1> {
        self.0
            .lock()
            .unwrap()
            .send(event)
            .map_err(|_| InspectionSinkErrorV1::Closed)
    }
}

/// Emits one logical inspection with the given stages through a freshly installed runtime
/// and returns the delivered events in emission order.
pub(crate) fn emit_logical(
    domains: CaptureDomainsV1,
    stages: &[(InspectionStageDomainV1, StageShape)],
) -> Vec<InspectionEventV1> {
    let descriptor = DashboardCaptureDescriptorV1::new(domains);
    let (sender, receiver): (Sender<InspectionEventV1>, Receiver<InspectionEventV1>) =
        mpsc::channel();
    let consumer = PendingInspectionConsumerV1::new(
        descriptor,
        Arc::new(ChannelSink(Mutex::new(sender))),
        InspectionQueueLimitsV1::new(64, 1 << 20).unwrap(),
    );
    let (producer, _activated) =
        install_inspection_v1(PendingInspectionProducerV1::new(descriptor), consumer).unwrap();
    let mut logical = producer.begin_logical_blocking().unwrap();
    let mut events = Vec::new();
    for (stage, shape) in stages {
        let shape = *shape;
        let sentinel = sentinel_for(*stage);
        let outcome = match stage {
            InspectionStageDomainV1::OwnerRequest => logical
                .try_emit_owner_request(safe_fields(), || {
                    build_projection!(OwnerRawProjectionV1, OwnerRawPayloadV1, shape, sentinel)
                }),
            InspectionStageDomainV1::ProviderRequest => {
                logical.try_emit_provider_request(safe_fields(), || {
                    build_projection!(
                        ProviderVisibleProjectionV1,
                        ProviderVisiblePayloadV1,
                        shape,
                        sentinel
                    )
                })
            }
            InspectionStageDomainV1::ProviderResponse => {
                logical.try_emit_provider_response(safe_fields(), || {
                    build_projection!(
                        ProviderVisibleProjectionV1,
                        ProviderVisiblePayloadV1,
                        shape,
                        sentinel
                    )
                })
            }
            InspectionStageDomainV1::OwnerRestoredResponse => logical
                .try_emit_owner_restored_response(safe_fields(), || {
                    build_projection!(
                        OwnerRestoredProjectionV1,
                        OwnerRestoredPayloadV1,
                        shape,
                        sentinel
                    )
                }),
        };
        assert!(
            matches!(outcome, InspectionAdmissionOutcomeV1::Accepted { .. }),
            "fixture emission must be admitted: {outcome:?}"
        );
        events.push(receiver.recv_timeout(Duration::from_secs(10)).unwrap());
    }
    events
}

/// Round-trips events through the production IPC encoder and decoder into a store.
pub(crate) fn admit_all(store: &mut EventStore, events: &[InspectionEventV1], now: Instant) {
    for event in events {
        let encoded = EncodedInspectionFrame::from_event(event, 1 << 20).unwrap();
        let decoded = DecodedInspectionFrame::decode(encoded.as_bytes(), 1 << 20).unwrap();
        assert_eq!(store.admit(decoded, now), AdmissionOutcome::Inserted);
    }
}

pub(crate) fn fixture_store() -> EventStore {
    let limits = RetentionLimits::new(256, 16 * 1024 * 1024, Duration::from_secs(300)).unwrap();
    EventStore::new(limits, InspectionEpochV1::new(0))
}
