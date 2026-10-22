//! Browser fixtures produced by the production snapshot encoder.
//!
//! `browser-tests/encoder-fixtures.json` is the snapshot body the child would write for
//! each named fixture: closed gaze-inspection projections are converted by the real
//! `StageModelV1` conversion, admitted into the real `EventStore`, and encoded by the
//! real snapshot/follow encoder. The Playwright fixture server serves these bodies
//! verbatim, so the browser contract runs against the exact wire the child emits.
//!
//! The JSON is about 750 KB (the 10k-entry SSE fixture dominates), so it is generated,
//! not committed. Only its SHA-256 is committed, in `encoder-fixtures.sha256`:
//!
//! * this test renders the fixtures, writes the JSON next to the harness, and requires the
//!   rendered digest to equal the committed one (an unreviewed wire change fails here);
//! * `browser-tests/fixtures.mjs` refuses to serve a JSON whose digest differs.
//!
//! Prepare the harness with `cargo test -p gaze-proxy-dashboard browser_fixtures`.
//! After an intentional wire change, re-bless the digest with
//! `GAZE_DASHBOARD_BLESS_FIXTURES=1 cargo test -p gaze-proxy-dashboard browser_fixtures`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gaze_inspection::{
    OwnerRawPayloadV1, OwnerRawProjectionV1, OwnerRestoredPayloadV1, OwnerRestoredProjectionV1,
    ProviderVisiblePayloadV1, ProviderVisibleProjectionV1,
};
use gaze_types::inspection::{
    AttestationTraceV1, CoarseDurationBucketV1, ContentBlockIndexV1, DecisionEngineCodeV1,
    DecisionOutcomeCodeV1, DecisionTraceEntryV1, DecisionTraceV1, EndpointSelectionCodeV1,
    InspectionDeliveryCodeV1, InspectionDropCodeV1, InspectionEmissionIdV1, InspectionEpochV1,
    InspectionErrorCodeV1, InspectionEventMetaV1, InspectionMeasurementV1,
    InspectionOperationalStatusV1, InspectionQueueSnapshotV1, InspectionSafeFieldsV1,
    InspectionSequenceV1, InspectionStageDomainV1, InspectionSurfaceRefV1, JsonNodeOrdinalV1,
    JsonShapeNodeV1, JsonShapeSummaryV1, JsonShapeTruncationCodeV1, JsonTypeCodeV1,
    LogicalInspectionIdV1, PiiClassCodeV1, PiiCountV1, PiiSummaryV1, PortSelectionCodeV1,
    ProjectionAvailabilityV1, ProjectionOmissionReasonV1, ProviderProfileCodeV1, RouteCodeV1,
    SseDeltaKindV1, SseEntryOrdinalV1, SseEventKindV1, SseTimelineEntryV1, SseTimelineMetaV1,
};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::ipc::{DecodedInspectionFrame, RuntimeStatusFrameV1};
use crate::snapshot::{CaptureTierV1, PayloadAvailabilityV1, SnapshotV1, StageModelV1};
use crate::store::{AdmissionOutcome, EventStore};
use crate::RetentionLimits;

use InspectionStageDomainV1::{
    OwnerRequest, OwnerRestoredResponse, ProviderRequest, ProviderResponse,
};
use ProjectionAvailabilityV1::{Omitted, Present};
use ProjectionOmissionReasonV1 as Reason;

/// Captured fixture payload bytes. They must never reach the metadata stream.
const FIXTURE_PAYLOAD: &[u8] = b"FIXTURE-PAYLOAD-SENTINEL-3T6";

/// One emission's closed projection.
struct StageSpec {
    stage: InspectionStageDomainV1,
    emission: u64,
    /// `None` when the whole projection is omitted with `omitted`.
    projection: Option<ComponentSpec>,
    omitted: Reason,
}

struct ComponentSpec {
    payload: ProjectionAvailabilityV1<()>,
    measurement: ProjectionAvailabilityV1<InspectionMeasurementV1>,
    json_shape: ProjectionAvailabilityV1<JsonShapeSummaryV1>,
    sse_timeline: ProjectionAvailabilityV1<SseTimelineMetaV1>,
    pii_summary: ProjectionAvailabilityV1<PiiSummaryV1>,
    decision_trace: ProjectionAvailabilityV1<DecisionTraceV1>,
    attestation: ProjectionAvailabilityV1<AttestationTraceV1>,
}

const NA: Reason = Reason::NotApplicable;

fn structure(
    types: &[JsonTypeCodeV1],
    truncation: JsonShapeTruncationCodeV1,
) -> JsonShapeSummaryV1 {
    let nodes = types
        .iter()
        .enumerate()
        .map(|(ordinal, type_code)| {
            JsonShapeNodeV1::new(JsonNodeOrdinalV1::new(ordinal as u32), *type_code)
        })
        .collect();
    JsonShapeSummaryV1::try_new(nodes, truncation).unwrap()
}

fn small_structure() -> JsonShapeSummaryV1 {
    structure(
        &[
            JsonTypeCodeV1::Object,
            JsonTypeCodeV1::String,
            JsonTypeCodeV1::Array,
            JsonTypeCodeV1::Number,
            JsonTypeCodeV1::Boolean,
            JsonTypeCodeV1::Null,
        ],
        JsonShapeTruncationCodeV1::Complete,
    )
}

fn sse(
    ordinal: u32,
    kind: SseEventKindV1,
    delta: Option<SseDeltaKindV1>,
    block: Option<u32>,
) -> SseTimelineEntryV1 {
    SseTimelineEntryV1::new(
        SseEntryOrdinalV1::new(ordinal),
        kind,
        delta.map_or(Omitted(NA), Present),
        block.map_or(Omitted(NA), |block| {
            Present(ContentBlockIndexV1::new(block))
        }),
    )
}

fn small_stream() -> SseTimelineMetaV1 {
    use SseDeltaKindV1 as D;
    use SseEventKindV1 as K;
    let rows = [
        (K::MessageStart, None, None),
        (K::ContentBlockStart, None, Some(0)),
        (K::ContentBlockDelta, Some(D::Text), Some(0)),
        (K::ContentBlockDelta, Some(D::Thinking), Some(0)),
        (K::ContentBlockDelta, Some(D::Signature), Some(0)),
        (K::ContentBlockStop, None, Some(0)),
        (K::ContentBlockStart, None, Some(1)),
        (K::ContentBlockDelta, Some(D::InputJson), Some(1)),
        (K::ContentBlockStop, None, Some(1)),
        (K::MessageDelta, None, None),
        (K::MessageStop, None, None),
    ];
    SseTimelineMetaV1::try_new(
        rows.iter()
            .enumerate()
            .map(|(ordinal, (kind, delta, block))| sse(ordinal as u32, *kind, *delta, *block))
            .collect(),
    )
    .unwrap()
}

fn long_stream() -> SseTimelineMetaV1 {
    use SseEventKindV1 as K;
    let mut entries = vec![sse(0, K::MessageStart, None, None)];
    for ordinal in 1..9_998_u32 {
        let block = Some(ordinal / 100);
        entries.push(match ordinal % 5 {
            1 => sse(ordinal, K::ContentBlockStart, None, block),
            2 | 3 => sse(
                ordinal,
                K::ContentBlockDelta,
                Some(SseDeltaKindV1::Text),
                block,
            ),
            4 => sse(ordinal, K::ContentBlockStop, None, block),
            _ => sse(ordinal, K::Ping, None, None),
        });
    }
    entries.push(sse(9_998, K::MessageDelta, None, None));
    entries.push(sse(9_999, K::MessageStop, None, None));
    SseTimelineMetaV1::try_new(entries).unwrap()
}

fn decision(
    engine: DecisionEngineCodeV1,
    outcome: DecisionOutcomeCodeV1,
    class: Option<(PiiClassCodeV1, u32)>,
) -> DecisionTraceEntryV1 {
    let (class, count) = class.map_or((Omitted(NA), 0), |(class, count)| (Present(class), count));
    DecisionTraceEntryV1::new(
        engine,
        outcome,
        InspectionSurfaceRefV1::WholeBody,
        class,
        count,
        count,
    )
}

fn full_decisions() -> DecisionTraceV1 {
    use DecisionEngineCodeV1 as E;
    use DecisionOutcomeCodeV1 as O;
    DecisionTraceV1::try_new(vec![
        decision(E::DeterministicRecognizer, O::RanClean, None),
        decision(
            E::Ner,
            O::RanFindings,
            Some((PiiClassCodeV1::PersonName, 2)),
        ),
        decision(E::NeuralSafetyNet, O::SkippedNotApplicable, None),
        decision(E::ResidualScan, O::UnavailableFailClosed, None),
    ])
    .unwrap()
}

fn pii() -> PiiSummaryV1 {
    PiiSummaryV1::try_new(vec![
        PiiCountV1::new(PiiClassCodeV1::PersonName, 2, 2),
        PiiCountV1::new(PiiClassCodeV1::Email, 1, 1),
    ])
    .unwrap()
}

/// A captured stage. Request stages carry the structure and decisions, response stages
/// the stream, and only the provider request carries the event's attestation. Every
/// other component is an exact NotApplicable omission.
fn captured(
    stage: InspectionStageDomainV1,
    emission: u64,
    attestation: AttestationTraceV1,
) -> StageSpec {
    let request = matches!(stage, OwnerRequest | ProviderRequest);
    let (bytes, chunks) = match stage {
        OwnerRequest => (1_500, 1),
        ProviderRequest => (1_421, 3),
        ProviderResponse => (5_230, 18),
        OwnerRestoredResponse => (5_300, 1),
    };
    StageSpec {
        stage,
        emission,
        projection: Some(ComponentSpec {
            payload: Present(()),
            measurement: Present(InspectionMeasurementV1::new(bytes, chunks)),
            json_shape: if request {
                Present(small_structure())
            } else {
                Omitted(NA)
            },
            sse_timeline: if request {
                Omitted(NA)
            } else {
                Present(small_stream())
            },
            pii_summary: Present(pii()),
            decision_trace: if request {
                Present(full_decisions())
            } else {
                Omitted(NA)
            },
            attestation: if stage == ProviderRequest {
                Present(attestation)
            } else {
                Omitted(NA)
            },
        }),
        omitted: NA,
    }
}

/// The producer omitted the payload and every component with one exact reason.
fn payload_omitted(stage: InspectionStageDomainV1, emission: u64, reason: Reason) -> StageSpec {
    StageSpec {
        stage,
        emission,
        projection: Some(ComponentSpec {
            payload: Omitted(reason),
            measurement: Omitted(reason),
            json_shape: Omitted(reason),
            sse_timeline: Omitted(reason),
            pii_summary: Omitted(reason),
            decision_trace: Omitted(reason),
            attestation: Omitted(reason),
        }),
        omitted: reason,
    }
}

/// The whole projection is omitted, as gaze-inspection emits for an uncaptured domain.
fn projection_omitted(stage: InspectionStageDomainV1, emission: u64, reason: Reason) -> StageSpec {
    StageSpec {
        stage,
        emission,
        projection: None,
        omitted: reason,
    }
}

macro_rules! convert {
    ($projection:ident, $payload:ident, $spec:expr, $from:ident) => {{
        let spec = $spec;
        let payload = match spec.payload {
            Present(()) => Present($payload::capture(FIXTURE_PAYLOAD.to_vec())),
            Omitted(reason) => Omitted(reason),
        };
        StageModelV1::$from(&$projection::new(
            payload,
            spec.measurement,
            spec.json_shape,
            spec.sse_timeline,
            spec.pii_summary,
            spec.decision_trace,
            spec.attestation,
        ))
    }};
}

fn stage_model(
    stage: InspectionStageDomainV1,
    spec: Option<ComponentSpec>,
    omitted: Reason,
) -> StageModelV1 {
    let Some(spec) = spec else {
        return StageModelV1::omitted(omitted);
    };
    match stage {
        OwnerRequest => convert!(
            OwnerRawProjectionV1,
            OwnerRawPayloadV1,
            spec,
            from_owner_raw
        ),
        ProviderRequest | ProviderResponse => convert!(
            ProviderVisibleProjectionV1,
            ProviderVisiblePayloadV1,
            spec,
            from_provider_visible
        ),
        OwnerRestoredResponse => convert!(
            OwnerRestoredProjectionV1,
            OwnerRestoredPayloadV1,
            spec,
            from_owner_restored
        ),
    }
}

struct EventSpec {
    logical: u64,
    route: RouteCodeV1,
    profile: ProviderProfileCodeV1,
    endpoint: EndpointSelectionCodeV1,
    port: PortSelectionCodeV1,
    status: InspectionOperationalStatusV1,
    duration: CoarseDurationBucketV1,
    stages: Vec<StageSpec>,
}

fn event(logical: u64, stages: Vec<StageSpec>) -> EventSpec {
    EventSpec {
        logical,
        route: RouteCodeV1::AnthropicMessages,
        profile: ProviderProfileCodeV1::AnthropicMessagesDirect,
        endpoint: EndpointSelectionCodeV1::OsAssignedLoopback,
        port: PortSelectionCodeV1::OsAssigned,
        status: InspectionOperationalStatusV1::Accepted,
        duration: CoarseDurationBucketV1::UnderOneHundredMilliseconds,
        stages,
    }
}

fn provider_event(logical: u64, attestation: AttestationTraceV1) -> EventSpec {
    event(
        logical,
        vec![
            captured(ProviderRequest, logical * 10 + 2, attestation),
            captured(ProviderResponse, logical * 10 + 3, attestation),
        ],
    )
}

fn owner_event(
    logical: u64,
    raw: bool,
    restored: bool,
    attestation: AttestationTraceV1,
) -> EventSpec {
    let owner = |stage, emission, captured_here: bool| {
        if captured_here {
            captured(stage, emission, attestation)
        } else {
            projection_omitted(stage, emission, Reason::NotCapturedByPolicy)
        }
    };
    event(
        logical,
        vec![
            owner(OwnerRequest, logical * 10 + 1, raw),
            captured(ProviderRequest, logical * 10 + 2, attestation),
            captured(ProviderResponse, logical * 10 + 3, attestation),
            owner(OwnerRestoredResponse, logical * 10 + 4, restored),
        ],
    )
}

fn with_stage(
    mut spec: EventSpec,
    stage: InspectionStageDomainV1,
    edit: impl FnOnce(&mut ComponentSpec),
) -> EventSpec {
    let target = spec
        .stages
        .iter_mut()
        .find(|candidate| candidate.stage == stage)
        .and_then(|candidate| candidate.projection.as_mut())
        .expect("captured stage");
    edit(target);
    spec
}

fn admit(store: &mut EventStore, spec: EventSpec, now: Instant) {
    for (sequence, stage) in spec.stages.into_iter().enumerate() {
        let safe = InspectionSafeFieldsV1::new(
            spec.route,
            spec.profile,
            spec.endpoint,
            spec.port,
            spec.status,
            InspectionErrorCodeV1::None,
            InspectionDropCodeV1::None,
            InspectionDeliveryCodeV1::Delivered,
            spec.duration,
            InspectionQueueSnapshotV1::new(0, 0, 0),
        );
        let sequence = InspectionSequenceV1::new(sequence as u64);
        let meta = InspectionEventMetaV1::new(
            LogicalInspectionIdV1::new(spec.logical),
            InspectionEmissionIdV1::new(stage.emission),
            sequence,
            InspectionEpochV1::new(1),
            stage.stage,
            safe,
        );
        let projection = stage_model(stage.stage, stage.projection, stage.omitted);
        let payload = match projection.payload() {
            PayloadAvailabilityV1::Present => FIXTURE_PAYLOAD.to_vec(),
            PayloadAvailabilityV1::Omitted(_) => Vec::new(),
        };
        let frame = DecodedInspectionFrame {
            logical_id: LogicalInspectionIdV1::new(spec.logical),
            emission_id: InspectionEmissionIdV1::new(stage.emission),
            sequence,
            epoch: InspectionEpochV1::new(1),
            stage_domain: stage.stage,
            meta: serde_json::from_value(serde_json::to_value(meta).unwrap()).unwrap(),
            projection,
            safe_wire_bytes: 256,
            payload: Zeroizing::new(payload),
        };
        assert_eq!(store.admit(frame, now), AdmissionOutcome::Inserted);
    }
}

struct Counters {
    full: u64,
    closed: u64,
    oversize: u64,
    evicted: u64,
    stale: u64,
    conflict: u64,
}

const ZERO: Counters = Counters {
    full: 0,
    closed: 0,
    oversize: 0,
    evicted: 0,
    stale: 0,
    conflict: 0,
};

const DROPS: Counters = Counters {
    full: 7,
    closed: 2,
    oversize: 1,
    evicted: 3,
    stale: 2,
    conflict: 1,
};

fn snapshot(tier: CaptureTierV1, counters: &Counters, events: Vec<EventSpec>) -> Value {
    let now = Instant::now();
    let limits = RetentionLimits::new(64, 64 * 1024 * 1024, Duration::from_secs(300)).unwrap();
    let mut store = EventStore::new(limits, InspectionEpochV1::new(1));
    for spec in events {
        admit(&mut store, spec, now);
    }
    let store_counters = store.counters_mut();
    store_counters.oversize = counters.oversize;
    store_counters.capacity_evicted = counters.evicted;
    store_counters.stale_epoch = counters.stale;
    store_counters.duplicate_conflict = counters.conflict;
    crate::child::fixture_snapshot(
        store,
        Some(RuntimeStatusFrameV1 {
            capture: tier,
            ingress_full: counters.full,
            ingress_closed: counters.closed,
        }),
        now,
    )
}

fn default_events() -> Vec<EventSpec> {
    use AttestationTraceV1 as A;
    let mut four = provider_event(4, A::NotApplicable);
    four.profile = ProviderProfileCodeV1::OpenAiCompatible;
    four.endpoint = EndpointSelectionCodeV1::DynamicallyDiscoveredLoopback;
    four.port = PortSelectionCodeV1::DynamicallyDiscovered;
    four.duration = CoarseDurationBucketV1::OneToTenSeconds;
    let mut three = provider_event(3, A::MissExpired);
    three.profile = ProviderProfileCodeV1::GeminiCompatible;
    three.endpoint = EndpointSelectionCodeV1::RevalidatedLoopback;
    three.port = PortSelectionCodeV1::ChangedAndRevalidated;
    three.duration = CoarseDurationBucketV1::OverTenSeconds;
    let mut two = with_stage(
        provider_event(2, A::UnavailableFailClosed),
        ProviderRequest,
        |spec| {
            use DecisionEngineCodeV1 as E;
            use DecisionOutcomeCodeV1 as O;
            spec.decision_trace = Present(
                DecisionTraceV1::try_new(vec![
                    decision(E::DeterministicRecognizer, O::RanClean, None),
                    decision(E::Ner, O::TimeoutFailClosed, None),
                    decision(E::ResidualScan, O::ErrorFailClosed, None),
                ])
                .unwrap(),
            );
        },
    );
    two.profile = ProviderProfileCodeV1::GenericLocalAnthropicCompatible;
    two.endpoint = EndpointSelectionCodeV1::ConfiguredHttpsOrigin;
    two.port = PortSelectionCodeV1::FixedConfigured;
    two.status = InspectionOperationalStatusV1::FailedClosed;
    let mut one = event(
        1,
        vec![projection_omitted(
            ProviderRequest,
            11,
            Reason::ProjectionFailedClosed,
        )],
    );
    one.route = RouteCodeV1::Rejected;
    one.profile = ProviderProfileCodeV1::OtherReviewed;
    one.endpoint = EndpointSelectionCodeV1::UnavailableFailClosed;
    one.port = PortSelectionCodeV1::UnavailableFailClosed;
    one.status = InspectionOperationalStatusV1::Omitted;
    one.duration = CoarseDurationBucketV1::NotMeasured;
    vec![provider_event(5, A::ExactHit), four, three, two, one]
}

/// Every named browser fixture the encoder can produce.
fn fixtures() -> Vec<(&'static str, Value)> {
    use AttestationTraceV1 as A;
    use CaptureTierV1 as T;
    let deep_types = (0..64)
        .map(|depth| {
            if depth % 2 == 0 {
                JsonTypeCodeV1::Object
            } else {
                JsonTypeCodeV1::Array
            }
        })
        .collect::<Vec<_>>();
    let mut wide_types = vec![JsonTypeCodeV1::Object];
    wide_types.extend(std::iter::repeat_n(JsonTypeCodeV1::String, 512));
    vec![
        (
            "fx-default",
            snapshot(T::ProviderVisible, &ZERO, default_events()),
        ),
        (
            "fx-owner-raw",
            snapshot(
                T::OwnerRaw,
                &ZERO,
                vec![
                    owner_event(7, true, false, A::MissNoRecord),
                    provider_event(6, A::MissEvicted),
                ],
            ),
        ),
        (
            "fx-owner-restored",
            snapshot(
                T::OwnerRestored,
                &ZERO,
                vec![
                    owner_event(9, false, true, A::InvalidatedPolicyOrRulepack),
                    provider_event(8, A::InvalidatedContentBytes),
                ],
            ),
        ),
        (
            "fx-owner-both",
            snapshot(
                T::Both,
                &DROPS,
                vec![
                    owner_event(12, true, true, A::AmbiguousFailClosed),
                    provider_event(11, A::InvalidatedSessionEpoch),
                ],
            ),
        ),
        (
            "fx-content-pv-omitted",
            snapshot(
                T::ProviderVisible,
                &ZERO,
                vec![event(
                    14,
                    vec![
                        captured(ProviderRequest, 301, A::ExactHit),
                        payload_omitted(ProviderResponse, 302, Reason::LimitExceeded),
                    ],
                )],
            ),
        ),
        (
            "fx-content-owner-omitted",
            snapshot(
                T::OwnerRaw,
                &ZERO,
                vec![event(
                    15,
                    vec![
                        payload_omitted(OwnerRequest, 311, Reason::SignedOrEncryptedSurface),
                        captured(ProviderRequest, 312, A::ExactHit),
                        captured(ProviderResponse, 313, A::ExactHit),
                    ],
                )],
            ),
        ),
        (
            "fx-hostile",
            snapshot(
                T::ProviderVisible,
                &ZERO,
                vec![provider_event(16, A::ExactHit)],
            ),
        ),
        (
            "fx-structure-deep",
            snapshot(
                T::ProviderVisible,
                &ZERO,
                vec![with_stage(
                    provider_event(20, A::ExactHit),
                    ProviderRequest,
                    |spec| {
                        spec.json_shape = Present(structure(
                            &deep_types,
                            JsonShapeTruncationCodeV1::DepthLimit,
                        ));
                    },
                )],
            ),
        ),
        (
            "fx-structure-wide",
            snapshot(
                T::ProviderVisible,
                &ZERO,
                vec![with_stage(
                    provider_event(21, A::ExactHit),
                    ProviderRequest,
                    |spec| {
                        spec.json_shape =
                            Present(structure(&wide_types, JsonShapeTruncationCodeV1::NodeLimit));
                    },
                )],
            ),
        ),
        (
            "fx-structure-malformed",
            snapshot(
                T::ProviderVisible,
                &ZERO,
                vec![with_stage(
                    provider_event(22, A::ExactHit),
                    ProviderRequest,
                    |spec| spec.json_shape = Omitted(Reason::MalformedFailClosed),
                )],
            ),
        ),
        (
            "fx-sse-10k",
            snapshot(
                T::ProviderVisible,
                &ZERO,
                vec![with_stage(
                    provider_event(23, A::ExactHit),
                    ProviderResponse,
                    |spec| spec.sse_timeline = Present(long_stream()),
                )],
            ),
        ),
        (
            "fx-drops",
            snapshot(T::ProviderVisible, &DROPS, default_events()),
        ),
        ("fx-zero", snapshot(T::ProviderVisible, &ZERO, Vec::new())),
        (
            "fx-disconnected",
            serde_json::from_slice(&SnapshotV1::failed_closed().encode()).unwrap(),
        ),
    ]
}

fn render() -> String {
    let fixtures = fixtures();
    let mut out = String::from("{\n");
    for (index, (name, value)) in fixtures.iter().enumerate() {
        out.push_str(&format!(
            "  {}: {}{}\n",
            serde_json::to_string(name).unwrap(),
            serde_json::to_string(value).unwrap(),
            if index + 1 == fixtures.len() { "" } else { "," }
        ));
    }
    out.push_str("}\n");
    out
}

fn harness_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("browser-tests")
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes the rendered fixtures for the browser harness and checks the committed digest.
#[test]
fn browser_fixtures_are_the_real_encoder_output() {
    let rendered = render();
    let digest = sha256_hex(rendered.as_bytes());
    let json_path = harness_dir().join("encoder-fixtures.json");
    let digest_path = harness_dir().join("encoder-fixtures.sha256");
    if std::fs::read_to_string(&json_path).ok().as_deref() != Some(rendered.as_str()) {
        // Best effort: on a read-only checkout the harness refuses the stale file by
        // digest, so a failed write can never let it serve something else.
        let _ = std::fs::write(&json_path, &rendered);
    }
    if std::env::var_os("GAZE_DASHBOARD_BLESS_FIXTURES").is_some() {
        std::fs::write(&digest_path, format!("{digest}\n")).unwrap();
    }
    let committed = std::fs::read_to_string(&digest_path).unwrap_or_default();
    assert_eq!(
        committed.trim(),
        digest,
        "the snapshot encoder's browser fixtures changed; review the wire change and \
         re-bless with GAZE_DASHBOARD_BLESS_FIXTURES=1"
    );
}

/// The harness loads the generated encoder output, verifies its digest, and builds no
/// snapshot body of its own.
#[test]
fn browser_harness_serves_only_encoder_fixtures() {
    let fixtures_mjs = std::fs::read_to_string(harness_dir().join("fixtures.mjs")).unwrap();
    for required in [
        "encoder-fixtures.json",
        "encoder-fixtures.sha256",
        "createHash(\"sha256\")",
    ] {
        assert!(
            fixtures_mjs.contains(required),
            "fixtures.mjs must load and verify {required}"
        );
    }
    // Wire keys only the encoder may produce never appear as hand-built literals.
    for wire_key in [
        "domainState",
        "captureTier",
        "measurements",
        "emissionId",
        "lifecycle: \"Running\"",
        "ingressFull",
    ] {
        assert!(
            !fixtures_mjs.contains(wire_key),
            "fixtures.mjs hand-builds encoder wire key {wire_key}"
        );
    }
    let names = fixtures()
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    for name in &names {
        assert!(
            fixtures_mjs.contains(&format!("\"{name}\"")),
            "fixtures.mjs must serve encoder fixture {name}"
        );
    }
}

#[test]
fn browser_fixture_metadata_never_carries_payload_bytes() {
    let rendered = render();
    assert!(!rendered.contains(std::str::from_utf8(FIXTURE_PAYLOAD).unwrap()));
}
