//! The one dashboard snapshot model.
//!
//! Two layers share this module:
//!
//! * [`StageModelV1`] / [`MetaModelV1`]: the closed, typed per-emission model that the
//!   parent encodes into the IPC frame and the child decodes. It is converted once, with an
//!   explicit match, from the closed `gaze-inspection` projection. Payload availability is
//!   carried as a typed field; it is never inferred from payload byte length.
//! * [`SnapshotV1`]: the browser wire model written by the snapshot and follow routes and
//!   consumed by `assets/app.js`. Its keys are covered by the `snapshot_contract` tests.
//!
//! No type here can hold payload bytes, so the metadata streams cannot carry them.

use gaze_inspection::{
    OwnerRawProjectionV1, OwnerRestoredProjectionV1, ProviderVisibleProjectionV1,
};
use gaze_types::inspection::{
    AttestationTraceV1, CaptureDomainsV1, CoarseDurationBucketV1, ContentBlockIndexV1,
    DecisionEngineCodeV1, DecisionOutcomeCodeV1, EndpointSelectionCodeV1, InspectionDeliveryCodeV1,
    InspectionDropCodeV1, InspectionEmissionIdV1, InspectionEpochV1, InspectionErrorCodeV1,
    InspectionMeasurementV1, InspectionMetadataVersionV1, InspectionOperationalStatusV1,
    InspectionQueueSnapshotV1, InspectionSequenceV1, InspectionStageDomainV1,
    InspectionSurfaceRefV1, JsonShapeSummaryV1, LogicalInspectionIdV1, PiiClassCodeV1,
    PortSelectionCodeV1, ProjectionAvailabilityV1, ProjectionOmissionReasonV1,
    ProviderProfileCodeV1, RouteCodeV1, SseDeltaKindV1, SseEntryOrdinalV1, SseEventKindV1,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The single omission-reason key table shared by the IPC model, the browser wire, and
/// the public view model. `assets/app.js` `OMISSION_LABELS` is tested against it key for key.
#[must_use]
pub(crate) const fn omission_reason_key(reason: ProjectionOmissionReasonV1) -> &'static str {
    match reason {
        ProjectionOmissionReasonV1::NotCapturedByPolicy => "NotCapturedByPolicy",
        ProjectionOmissionReasonV1::NotApplicable => "NotApplicable",
        ProjectionOmissionReasonV1::UnsupportedFormat => "UnsupportedFormat",
        ProjectionOmissionReasonV1::MalformedFailClosed => "MalformedFailClosed",
        ProjectionOmissionReasonV1::LimitExceeded => "LimitExceeded",
        ProjectionOmissionReasonV1::SignedOrEncryptedSurface => "SignedOrEncryptedSurface",
        ProjectionOmissionReasonV1::Purging => "Purging",
        ProjectionOmissionReasonV1::Disabled => "Disabled",
        ProjectionOmissionReasonV1::QueueClosed => "QueueClosed",
        ProjectionOmissionReasonV1::ProjectionFailedClosed => "ProjectionFailedClosed",
        ProjectionOmissionReasonV1::UnknownFuture => "UnknownFuture",
    }
}

/* ============================ IPC model ============================ */

/// Typed payload availability. Present means the producer captured the payload, even when
/// the captured body is zero bytes long.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PayloadAvailabilityV1 {
    Present,
    Omitted(ProjectionOmissionReasonV1),
}

impl PayloadAvailabilityV1 {
    pub(crate) fn of<T>(payload: &ProjectionAvailabilityV1<T>) -> Self {
        match payload {
            ProjectionAvailabilityV1::Present(_) => Self::Present,
            ProjectionAvailabilityV1::Omitted(reason) => Self::Omitted(*reason),
        }
    }
}

/// Typed mirror of one SSE timeline entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SseEntryModelV1 {
    ordinal: SseEntryOrdinalV1,
    event_kind: SseEventKindV1,
    delta_kind: ProjectionAvailabilityV1<SseDeltaKindV1>,
    content_block: ProjectionAvailabilityV1<ContentBlockIndexV1>,
}

/// Typed mirror of one PII-class count.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PiiCountModelV1 {
    class: PiiClassCodeV1,
    detected: u32,
    pseudonymized: u32,
}

/// Typed mirror of one decision-trace entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionModelV1 {
    engine: DecisionEngineCodeV1,
    outcome: DecisionOutcomeCodeV1,
    surface: InspectionSurfaceRefV1,
    pii_class: ProjectionAvailabilityV1<PiiClassCodeV1>,
    detected: u32,
    pseudonymized: u32,
}

/// Closed per-emission stage model carried over IPC. It has no payload byte field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StageModelV1 {
    payload: PayloadAvailabilityV1,
    measurement: ProjectionAvailabilityV1<InspectionMeasurementV1>,
    json_shape: ProjectionAvailabilityV1<JsonShapeSummaryV1>,
    sse_timeline: ProjectionAvailabilityV1<Vec<SseEntryModelV1>>,
    pii_summary: ProjectionAvailabilityV1<Vec<PiiCountModelV1>>,
    decision_trace: ProjectionAvailabilityV1<Vec<DecisionModelV1>>,
    attestation: ProjectionAvailabilityV1<AttestationTraceV1>,
}

/// Mirrors a bounded gaze-types collection through its own serde contract. A mismatch is
/// impossible for the pinned types; if it ever happens the component fails closed.
fn mirror_entries<S, M>(entries: &[S]) -> Option<Vec<M>>
where
    S: Serialize,
    M: DeserializeOwned,
{
    entries
        .iter()
        .map(|entry| {
            serde_json::to_value(entry)
                .ok()
                .and_then(|value| serde_json::from_value(value).ok())
        })
        .collect()
}

fn mirror_availability<T, M>(
    availability: &ProjectionAvailabilityV1<T>,
    convert: impl FnOnce(&T) -> Option<M>,
) -> ProjectionAvailabilityV1<M> {
    match availability {
        ProjectionAvailabilityV1::Present(value) => match convert(value) {
            Some(model) => ProjectionAvailabilityV1::Present(model),
            None => ProjectionAvailabilityV1::Omitted(
                ProjectionOmissionReasonV1::ProjectionFailedClosed,
            ),
        },
        ProjectionAvailabilityV1::Omitted(reason) => ProjectionAvailabilityV1::Omitted(*reason),
    }
}

macro_rules! stage_model_from {
    ($name:ident, $projection:ty) => {
        /// Explicit conversion from one closed domain projection.
        pub(crate) fn $name(projection: &$projection) -> Self {
            Self {
                payload: PayloadAvailabilityV1::of(projection.payload()),
                measurement: projection.measurement().clone(),
                json_shape: projection.json_shape().clone(),
                sse_timeline: mirror_availability(projection.sse_timeline(), |value| {
                    mirror_entries(value.entries())
                }),
                pii_summary: mirror_availability(projection.pii_summary(), |value| {
                    mirror_entries(value.entries())
                }),
                decision_trace: mirror_availability(projection.decision_trace(), |value| {
                    mirror_entries(value.entries())
                }),
                attestation: projection.attestation().clone(),
            }
        }
    };
}

impl StageModelV1 {
    stage_model_from!(from_owner_raw, OwnerRawProjectionV1);
    stage_model_from!(from_provider_visible, ProviderVisibleProjectionV1);
    stage_model_from!(from_owner_restored, OwnerRestoredProjectionV1);

    /// A wholly omitted projection: payload and every component carry the exact reason.
    pub(crate) const fn omitted(reason: ProjectionOmissionReasonV1) -> Self {
        Self {
            payload: PayloadAvailabilityV1::Omitted(reason),
            measurement: ProjectionAvailabilityV1::Omitted(reason),
            json_shape: ProjectionAvailabilityV1::Omitted(reason),
            sse_timeline: ProjectionAvailabilityV1::Omitted(reason),
            pii_summary: ProjectionAvailabilityV1::Omitted(reason),
            decision_trace: ProjectionAvailabilityV1::Omitted(reason),
            attestation: ProjectionAvailabilityV1::Omitted(reason),
        }
    }

    pub(crate) const fn payload(&self) -> PayloadAvailabilityV1 {
        self.payload
    }
}

/// Typed mirror of the hostile-sink-safe event metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MetaModelV1 {
    version: InspectionMetadataVersionV1,
    pub(crate) logical_id: LogicalInspectionIdV1,
    pub(crate) emission_id: InspectionEmissionIdV1,
    pub(crate) sequence: InspectionSequenceV1,
    pub(crate) epoch: InspectionEpochV1,
    pub(crate) stage_domain: InspectionStageDomainV1,
    route: RouteCodeV1,
    profile: ProviderProfileCodeV1,
    endpoint: EndpointSelectionCodeV1,
    port: PortSelectionCodeV1,
    status: InspectionOperationalStatusV1,
    error: InspectionErrorCodeV1,
    drop: InspectionDropCodeV1,
    delivery: InspectionDeliveryCodeV1,
    duration: CoarseDurationBucketV1,
    queue: InspectionQueueSnapshotV1,
}

/* ============================ browser wire ============================ */

/// Closed capture tier reported by the parent from the immutable startup descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) enum CaptureTierV1 {
    /// The parent has not reported the descriptor yet.
    NotReported,
    ProviderVisible,
    OwnerRaw,
    OwnerRestored,
    Both,
}

impl CaptureTierV1 {
    /// Only the four dashboard acceptance combinations are representable.
    pub(crate) const fn from_domains(domains: CaptureDomainsV1) -> Option<Self> {
        match domains {
            CaptureDomainsV1::ProviderVisible => Some(Self::ProviderVisible),
            CaptureDomainsV1::OwnerRawAndProviderVisible => Some(Self::OwnerRaw),
            CaptureDomainsV1::ProviderVisibleAndOwnerRestored => Some(Self::OwnerRestored),
            CaptureDomainsV1::All => Some(Self::Both),
            CaptureDomainsV1::MetadataOnly
            | CaptureDomainsV1::OwnerRaw
            | CaptureDomainsV1::OwnerRestored
            | CaptureDomainsV1::OwnerRawAndOwnerRestored => None,
        }
    }
}

/// Converts a closed gaze-types code (snake_case on its own wire) to the browser's
/// PascalCase table key. The browser label tables are tested against these keys.
pub(crate) fn browser_code<T: Serialize>(value: &T) -> String {
    let Ok(serde_json::Value::String(snake)) = serde_json::to_value(value) else {
        return "UnknownFuture".to_owned();
    };
    snake
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect()
}

/// Browser availability: a value container only when present, an exact reason otherwise.
#[derive(Debug, Serialize)]
#[serde(tag = "state")]
pub(crate) enum BrowserAvailabilityV1<T> {
    Present { value: T },
    Omitted { reason: &'static str },
}

fn browser_availability<T, U>(
    availability: &ProjectionAvailabilityV1<T>,
    convert: impl FnOnce(&T) -> U,
) -> BrowserAvailabilityV1<U> {
    match availability {
        ProjectionAvailabilityV1::Present(value) => BrowserAvailabilityV1::Present {
            value: convert(value),
        },
        ProjectionAvailabilityV1::Omitted(reason) => BrowserAvailabilityV1::Omitted {
            reason: omission_reason_key(*reason),
        },
    }
}

/// Exact payload-domain state. Reveal/show controls exist only for Concealed/Available.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub(crate) enum DomainStateV1 {
    /// Provider-visible payload captured and retained; it can be shown.
    Available,
    /// Owner payload captured and retained; only a server-leased reveal displays it.
    Concealed,
    /// No payload exists for this emission.
    Omitted { reason: &'static str },
}

impl DomainStateV1 {
    /// Owner payload becomes Concealed, provider payload becomes Available, and an omitted
    /// payload carries its exact reason.
    pub(crate) const fn derive(
        stage: InspectionStageDomainV1,
        payload: PayloadAvailabilityV1,
    ) -> Self {
        match (payload, stage) {
            (PayloadAvailabilityV1::Omitted(reason), _) => Self::Omitted {
                reason: omission_reason_key(reason),
            },
            (
                PayloadAvailabilityV1::Present,
                InspectionStageDomainV1::OwnerRequest
                | InspectionStageDomainV1::OwnerRestoredResponse,
            ) => Self::Concealed,
            (
                PayloadAvailabilityV1::Present,
                InspectionStageDomainV1::ProviderRequest
                | InspectionStageDomainV1::ProviderResponse,
            ) => Self::Available,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct MeasurementV1 {
    bytes: u64,
    chunks: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct PiiCountV1 {
    cls: String,
    detected: u32,
    pseudonymized: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonNodeV1 {
    ordinal: u32,
    #[serde(rename = "type")]
    type_code: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct StructureV1 {
    truncation: String,
    nodes: Vec<JsonNodeV1>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SseEntryV1 {
    ordinal: u32,
    kind: String,
    /// `null` when the delta kind is omitted for this entry.
    delta: Option<String>,
    /// `null` when the content-block index is omitted for this entry.
    block: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StreamV1 {
    entries: Vec<SseEntryV1>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecisionV1 {
    /// Position in the bounded trace.
    ordinal: usize,
    engine: String,
    outcome: String,
    /// `null` when the producer omitted the class.
    pii_class: Option<String>,
    detected: u32,
    pseudonymized: u32,
}

/// One stage of one logical event.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StageV1 {
    emission_id: u64,
    sequence: u64,
    domain_state: DomainStateV1,
    measurements: BrowserAvailabilityV1<MeasurementV1>,
    pii_summary: BrowserAvailabilityV1<Vec<PiiCountV1>>,
    structure: BrowserAvailabilityV1<StructureV1>,
    stream: BrowserAvailabilityV1<StreamV1>,
    decisions: BrowserAvailabilityV1<Vec<DecisionV1>>,
    attestation: BrowserAvailabilityV1<String>,
}

fn present_code<T: Serialize>(availability: &ProjectionAvailabilityV1<T>) -> Option<String> {
    match availability {
        ProjectionAvailabilityV1::Present(value) => Some(browser_code(value)),
        ProjectionAvailabilityV1::Omitted(_) => None,
    }
}

impl StageV1 {
    pub(crate) fn new(meta: &MetaModelV1, model: &StageModelV1) -> Self {
        Self {
            emission_id: meta.emission_id.get(),
            sequence: meta.sequence.get(),
            domain_state: DomainStateV1::derive(meta.stage_domain, model.payload),
            measurements: browser_availability(&model.measurement, |value| MeasurementV1 {
                bytes: value.byte_count(),
                chunks: value.chunk_count(),
            }),
            pii_summary: browser_availability(&model.pii_summary, |entries| {
                entries
                    .iter()
                    .map(|entry| PiiCountV1 {
                        cls: browser_code(&entry.class),
                        detected: entry.detected,
                        pseudonymized: entry.pseudonymized,
                    })
                    .collect()
            }),
            structure: browser_availability(&model.json_shape, |shape| StructureV1 {
                truncation: browser_code(&shape.truncation()),
                nodes: shape
                    .nodes()
                    .iter()
                    .map(|node| JsonNodeV1 {
                        ordinal: node.ordinal().get(),
                        type_code: browser_code(&node.type_code()),
                    })
                    .collect(),
            }),
            stream: browser_availability(&model.sse_timeline, |entries| StreamV1 {
                entries: entries
                    .iter()
                    .map(|entry| SseEntryV1 {
                        ordinal: entry.ordinal.get(),
                        kind: browser_code(&entry.event_kind),
                        delta: present_code(&entry.delta_kind),
                        block: match entry.content_block {
                            ProjectionAvailabilityV1::Present(index) => Some(index.get()),
                            ProjectionAvailabilityV1::Omitted(_) => None,
                        },
                    })
                    .collect(),
            }),
            decisions: browser_availability(&model.decision_trace, |entries| {
                entries
                    .iter()
                    .enumerate()
                    .map(|(ordinal, entry)| DecisionV1 {
                        ordinal,
                        engine: browser_code(&entry.engine),
                        outcome: browser_code(&entry.outcome),
                        pii_class: present_code(&entry.pii_class),
                        detected: entry.detected,
                        pseudonymized: entry.pseudonymized,
                    })
                    .collect()
            }),
            attestation: browser_availability(&model.attestation, browser_code),
        }
    }
}

/// Per-stage emissions of one logical event. Absent stages are absent keys.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagesV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) owner_request: Option<StageV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider_request: Option<StageV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider_response: Option<StageV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) owner_restored_response: Option<StageV1>,
}

impl StagesV1 {
    pub(crate) fn insert(&mut self, stage: InspectionStageDomainV1, value: StageV1) {
        let slot = match stage {
            InspectionStageDomainV1::OwnerRequest => &mut self.owner_request,
            InspectionStageDomainV1::ProviderRequest => &mut self.provider_request,
            InspectionStageDomainV1::ProviderResponse => &mut self.provider_response,
            InspectionStageDomainV1::OwnerRestoredResponse => &mut self.owner_restored_response,
        };
        *slot = Some(value);
    }
}

/// One logical event. Route/profile/endpoint/port/status/duration come from the
/// newest emission's safe metadata.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventV1 {
    logical_id: u64,
    epoch: u64,
    ttl_seconds: u64,
    route: String,
    profile: String,
    endpoint: String,
    port: String,
    status: String,
    duration: String,
    stages: StagesV1,
}

impl EventV1 {
    pub(crate) fn new(newest: &MetaModelV1, ttl_seconds: u64, stages: StagesV1) -> Self {
        Self {
            logical_id: newest.logical_id.get(),
            epoch: newest.epoch.get(),
            ttl_seconds,
            route: browser_code(&newest.route),
            profile: browser_code(&newest.profile),
            endpoint: browser_code(&newest.endpoint),
            port: browser_code(&newest.port),
            status: browser_code(&newest.status),
            duration: browser_code(&newest.duration),
            stages,
        }
    }
}

/// Saturating safe counters: child store outcomes plus parent ingress drops.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CountersV1 {
    pub(crate) expired: u64,
    pub(crate) capacity_evicted: u64,
    pub(crate) duplicate_idempotent: u64,
    pub(crate) duplicate_conflict: u64,
    pub(crate) stale_epoch: u64,
    pub(crate) oversize: u64,
    pub(crate) ingress_full: u64,
    pub(crate) ingress_closed: u64,
}

/// Accepted limitation: queue telemetry is not measured.
#[derive(Debug, Serialize)]
pub(crate) struct QueueTelemetryV1 {
    state: &'static str,
    reason: &'static str,
}

const QUEUE_UNMEASURED: QueueTelemetryV1 = QueueTelemetryV1 {
    state: "Unavailable",
    reason: "NotMeasured",
};

/// Closed child-runtime state.
#[derive(Debug, Serialize)]
#[serde(tag = "lifecycle", rename_all_fields = "camelCase")]
pub(crate) enum RuntimeV1 {
    Running {
        epoch: u64,
        capture_tier: CaptureTierV1,
        retained: u64,
        capacity: u64,
        ttl_seconds: u64,
        queue_telemetry: QueueTelemetryV1,
    },
    Disabled {
        disabled_code: &'static str,
    },
}

/// Store-side retention facts for the runtime header.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetentionFactsV1 {
    pub(crate) epoch: u64,
    pub(crate) retained: u64,
    pub(crate) capacity: u64,
    pub(crate) ttl_seconds: u64,
}

/// The snapshot/follow body.
#[derive(Debug, Serialize)]
pub(crate) struct SnapshotV1 {
    version: u8,
    runtime: RuntimeV1,
    counters: CountersV1,
    events: Vec<EventV1>,
}

impl SnapshotV1 {
    pub(crate) fn running(
        facts: RetentionFactsV1,
        capture_tier: CaptureTierV1,
        counters: CountersV1,
        events: Vec<EventV1>,
    ) -> Self {
        Self {
            version: 1,
            runtime: RuntimeV1::Running {
                epoch: facts.epoch,
                capture_tier,
                retained: facts.retained,
                capacity: facts.capacity,
                ttl_seconds: facts.ttl_seconds,
                queue_telemetry: QUEUE_UNMEASURED,
            },
            counters,
            events,
        }
    }

    /// Fail-closed body: no events, a closed disabled code.
    pub(crate) fn failed_closed() -> Self {
        Self {
            version: 1,
            runtime: RuntimeV1::Disabled {
                disabled_code: "ProjectionFailedClosed",
            },
            counters: CountersV1::default(),
            events: Vec::new(),
        }
    }

    /// The one encoder for the snapshot and follow routes.
    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| {
            serde_json::to_vec(&Self::failed_closed()).unwrap_or_else(|_| {
                br#"{"version":1,"runtime":{"lifecycle":"Disabled","disabledCode":"ProjectionFailedClosed"},"events":[]}"#.to_vec()
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ALL_REASONS;

    #[test]
    fn browser_codes_are_pascal_case_variant_names() {
        assert_eq!(
            browser_code(&ProviderProfileCodeV1::OpenAiCompatible),
            "OpenAiCompatible"
        );
        assert_eq!(
            browser_code(&EndpointSelectionCodeV1::OsAssignedLoopback),
            "OsAssignedLoopback"
        );
        assert_eq!(
            browser_code(&CoarseDurationBucketV1::OneHundredMillisecondsToOneSecond),
            "OneHundredMillisecondsToOneSecond"
        );
        for reason in ALL_REASONS {
            assert_eq!(browser_code(&reason), omission_reason_key(reason));
        }
    }

    #[test]
    fn omitted_stage_model_never_carries_a_value_container() {
        for reason in ALL_REASONS {
            let meta: MetaModelV1 = serde_json::from_value(serde_json::json!({
                "version": "v1", "logical_id": 1, "emission_id": 2, "sequence": 0, "epoch": 0,
                "stage_domain": "owner_request", "route": "anthropic_messages",
                "profile": "open_ai_compatible", "endpoint": "os_assigned_loopback",
                "port": "os_assigned", "status": "accepted", "error": "none", "drop": "none",
                "delivery": "pending", "duration": "not_measured",
                "queue": { "admitted": 0, "dropped": 0, "delivered": 0 }
            }))
            .unwrap();
            let stage =
                serde_json::to_value(StageV1::new(&meta, &StageModelV1::omitted(reason))).unwrap();
            for key in [
                "measurements",
                "piiSummary",
                "structure",
                "stream",
                "decisions",
                "attestation",
            ] {
                assert_eq!(
                    stage[key],
                    serde_json::json!({ "state": "Omitted", "reason": omission_reason_key(reason) })
                );
            }
            assert_eq!(
                stage["domainState"],
                serde_json::json!({ "kind": "Omitted", "reason": omission_reason_key(reason) })
            );
        }
    }

    #[test]
    fn failed_closed_snapshot_is_disabled_with_no_events() {
        let value: serde_json::Value =
            serde_json::from_slice(&SnapshotV1::failed_closed().encode()).unwrap();
        assert_eq!(value["runtime"]["lifecycle"], "Disabled");
        assert_eq!(value["runtime"]["disabledCode"], "ProjectionFailedClosed");
        assert_eq!(value["events"], serde_json::json!([]));
    }
}
