use gaze_types::inspection::{
    AttestationTraceV1, DecisionEngineCodeV1, DecisionOutcomeCodeV1, EndpointSelectionCodeV1,
    PortSelectionCodeV1, ProjectionOmissionReasonV1, ProviderProfileCodeV1, RouteCodeV1,
    SseDeltaKindV1, SseEventKindV1,
};
use serde::Serialize;

/// Closed projection availability for safe view models.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum ProjectionAvailabilityVm {
    /// A startup-authorized typed projection is present.
    Present,
    /// The producer supplied a mandatory closed omission reason.
    Omitted(ProjectionOmissionReasonV1),
}

impl ProjectionAvailabilityVm {
    /// Exact caution label. It never guesses a more specific cause.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Present => "AVAILABLE",
            Self::Omitted(ProjectionOmissionReasonV1::NotCapturedByPolicy) => {
                "NOT CAPTURED BY POLICY"
            }
            Self::Omitted(ProjectionOmissionReasonV1::NotApplicable) => "NOT APPLICABLE",
            Self::Omitted(ProjectionOmissionReasonV1::UnsupportedFormat) => "UNSUPPORTED FORMAT",
            Self::Omitted(ProjectionOmissionReasonV1::MalformedFailClosed) => {
                "MALFORMED — FAIL-CLOSED"
            }
            Self::Omitted(ProjectionOmissionReasonV1::LimitExceeded) => "LIMIT EXCEEDED",
            Self::Omitted(ProjectionOmissionReasonV1::SignedOrEncryptedSurface) => {
                "SIGNED OR ENCRYPTED SURFACE"
            }
            Self::Omitted(ProjectionOmissionReasonV1::Purging) => "PURGING",
            Self::Omitted(ProjectionOmissionReasonV1::Disabled) => "DISABLED",
            Self::Omitted(ProjectionOmissionReasonV1::QueueClosed) => "QUEUE CLOSED",
            Self::Omitted(ProjectionOmissionReasonV1::ProjectionFailedClosed) => {
                "PROJECTION FAILED CLOSED"
            }
            Self::Omitted(ProjectionOmissionReasonV1::UnknownFuture) => "UNKNOWN",
        }
    }
}

/// Safe event-list summary. It has no payload, object key, URL, host, or numeric port field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EventSummaryVm {
    logical_id: u64,
    emission_id: u64,
    sequence: u64,
    route: RouteCodeV1,
    profile: ProviderProfileCodeV1,
    endpoint: EndpointSelectionCodeV1,
    port: PortSelectionCodeV1,
    projection: ProjectionAvailabilityVm,
}

impl EventSummaryVm {
    /// Creates a summary solely from closed values and opaque IDs.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        logical_id: u64,
        emission_id: u64,
        sequence: u64,
        route: RouteCodeV1,
        profile: ProviderProfileCodeV1,
        endpoint: EndpointSelectionCodeV1,
        port: PortSelectionCodeV1,
        projection: ProjectionAvailabilityVm,
    ) -> Self {
        Self {
            logical_id,
            emission_id,
            sequence,
            route,
            profile,
            endpoint,
            port,
            projection,
        }
    }

    /// Returns the category-only port label.
    #[must_use]
    pub const fn port_label(&self) -> &'static str {
        port_label(self.port)
    }
}

/// Safe aggregate traffic counts supplied only by an authorized domain projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TrafficCountsVm {
    bytes: u64,
    chunks: u32,
}

impl TrafficCountsVm {
    /// Creates domain-authorized aggregate counts.
    #[must_use]
    pub const fn new(bytes: u64, chunks: u32) -> Self {
        Self { bytes, chunks }
    }
}

/// Ordinal/type-only JSON-shape view.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SafeJsonShapeVm {
    availability: ProjectionAvailabilityVm,
    node_count: Option<u32>,
}

impl SafeJsonShapeVm {
    /// Creates an available safe shape count.
    #[must_use]
    pub const fn present(node_count: u32) -> Self {
        Self {
            availability: ProjectionAvailabilityVm::Present,
            node_count: Some(node_count),
        }
    }

    /// Creates an omitted shape with no synthetic zero/empty value.
    #[must_use]
    pub const fn omitted(reason: ProjectionOmissionReasonV1) -> Self {
        Self {
            availability: ProjectionAvailabilityVm::Omitted(reason),
            node_count: None,
        }
    }
}

/// One SSE row. Its contract deliberately has no byte-count or timing field.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SseTimelineEntryVm {
    ordinal: u32,
    kind: SseEventKindV1,
    delta: Option<SseDeltaKindV1>,
    content_block_index: Option<u32>,
}

impl SseTimelineEntryVm {
    /// Creates the exact Revision-60 SSE field set.
    #[must_use]
    pub const fn new(
        ordinal: u32,
        kind: SseEventKindV1,
        delta: Option<SseDeltaKindV1>,
        content_block_index: Option<u32>,
    ) -> Self {
        Self {
            ordinal,
            kind,
            delta,
            content_block_index,
        }
    }
}

/// Bounded SSE timeline or an exact closed omission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SseTimelineVm {
    availability: ProjectionAvailabilityVm,
    entries: Vec<SseTimelineEntryVm>,
}

impl SseTimelineVm {
    /// Creates a present bounded timeline.
    #[must_use]
    pub fn present(entries: Vec<SseTimelineEntryVm>) -> Self {
        Self {
            availability: ProjectionAvailabilityVm::Present,
            entries,
        }
    }

    /// Creates an omitted timeline with no synthetic empty value.
    #[must_use]
    pub fn omitted(reason: ProjectionOmissionReasonV1) -> Self {
        Self {
            availability: ProjectionAvailabilityVm::Omitted(reason),
            entries: Vec::new(),
        }
    }
}

/// Neutral/caution decision-trace row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DecisionTraceVm {
    engine: DecisionEngineCodeV1,
    outcome: DecisionOutcomeCodeV1,
}

impl DecisionTraceVm {
    /// Creates a trace row from closed producer values.
    #[must_use]
    pub const fn new(engine: DecisionEngineCodeV1, outcome: DecisionOutcomeCodeV1) -> Self {
        Self { engine, outcome }
    }

    /// Returns a neutral/caution outcome label; never a verified-clean claim.
    #[must_use]
    pub const fn outcome_label(self) -> &'static str {
        match self.outcome {
            DecisionOutcomeCodeV1::RanClean => "RAN — NO FINDINGS (not verified clean)",
            DecisionOutcomeCodeV1::RanFindings => "FINDINGS",
            DecisionOutcomeCodeV1::SkippedNotApplicable => "NOT APPLICABLE",
            DecisionOutcomeCodeV1::UnavailableFailClosed
            | DecisionOutcomeCodeV1::TimeoutFailClosed
            | DecisionOutcomeCodeV1::ErrorFailClosed => "FAIL-CLOSED",
        }
    }
}

/// Neutral/caution attestation view.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AttestationTraceVm {
    value: AttestationTraceV1,
}

impl AttestationTraceVm {
    /// Creates a view from one closed attestation state.
    #[must_use]
    pub const fn new(value: AttestationTraceV1) -> Self {
        Self { value }
    }

    /// Exhaustive neutral/caution label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self.value {
            AttestationTraceV1::NotApplicable => "NOT APPLICABLE",
            AttestationTraceV1::ExactHit => "EXACT HIT",
            AttestationTraceV1::MissNoRecord => "MISS — NO RECORD",
            AttestationTraceV1::MissExpired => "MISS — EXPIRED",
            AttestationTraceV1::MissEvicted => "MISS — EVICTED",
            AttestationTraceV1::InvalidatedContentBytes => "INVALIDATED — CONTENT BYTES",
            AttestationTraceV1::InvalidatedStructuralIdentity => {
                "INVALIDATED — STRUCTURAL IDENTITY"
            }
            AttestationTraceV1::InvalidatedPolicyOrRulepack => "INVALIDATED — POLICY OR RULEPACK",
            AttestationTraceV1::InvalidatedRecognizerOrArtifact => {
                "INVALIDATED — RECOGNIZER OR ARTIFACT"
            }
            AttestationTraceV1::InvalidatedLocaleOrDictionary => {
                "INVALIDATED — LOCALE OR DICTIONARY"
            }
            AttestationTraceV1::InvalidatedNormalizationVersion => {
                "INVALIDATED — NORMALIZATION VERSION"
            }
            AttestationTraceV1::InvalidatedSessionEpoch => "INVALIDATED — SESSION EPOCH",
            AttestationTraceV1::InvalidatedConfigurationGeneration => {
                "INVALIDATED — CONFIGURATION GENERATION"
            }
            AttestationTraceV1::AmbiguousFailClosed => "AMBIGUOUS FAIL-CLOSED",
            AttestationTraceV1::UnavailableFailClosed => "UNAVAILABLE FAIL-CLOSED",
            AttestationTraceV1::UnknownFuture => "UNKNOWN FUTURE",
        }
    }
}

/// Startup-fixed domain availability. ProviderVisible cannot be NotCaptured.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DomainsAvailabilityVm {
    provider_visible: ProjectionAvailabilityVm,
    owner_raw_captured: bool,
    owner_restored_captured: bool,
}

impl DomainsAvailabilityVm {
    /// Creates availability from immutable startup policy and exact producer omission.
    #[must_use]
    pub const fn new(
        provider_visible: ProjectionAvailabilityVm,
        owner_raw_captured: bool,
        owner_restored_captured: bool,
    ) -> Self {
        Self {
            provider_visible,
            owner_raw_captured,
            owner_restored_captured,
        }
    }
}

/// Closed runtime view. Queue telemetry is explicitly unavailable/not measured.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeVm {
    queue_telemetry_available: bool,
    view_incomplete: bool,
}

impl RuntimeVm {
    /// Creates the accepted current runtime limitation.
    #[must_use]
    pub const fn queue_unmeasured(view_incomplete: bool) -> Self {
        Self {
            queue_telemetry_available: false,
            view_incomplete,
        }
    }
}

const fn port_label(value: PortSelectionCodeV1) -> &'static str {
    match value {
        PortSelectionCodeV1::NotApplicable => "NOT APPLICABLE",
        PortSelectionCodeV1::FixedConfigured => "FIXED CONFIGURED",
        PortSelectionCodeV1::OsAssigned => "OS ASSIGNED",
        PortSelectionCodeV1::DynamicallyDiscovered => "DYNAMICALLY DISCOVERED",
        PortSelectionCodeV1::ChangedAndRevalidated => "CHANGED AND REVALIDATED",
        PortSelectionCodeV1::UnavailableFailClosed => "UNAVAILABLE FAIL-CLOSED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_vm_has_no_per_entry_byte_or_timing_surface() {
        let vm = SseTimelineVm::present(vec![SseTimelineEntryVm::new(
            4,
            SseEventKindV1::ContentBlockDelta,
            Some(SseDeltaKindV1::Text),
            Some(2),
        )]);
        let json = serde_json::to_string(&vm).unwrap();
        assert!(!json.contains("byte"));
        assert!(!json.contains("timing"));
        assert!(!json.contains("time"));
        assert_eq!(
            json,
            r#"{"availability":{"state":"present"},"entries":[{"ordinal":4,"kind":"content_block_delta","delta":"text","content_block_index":2}]}"#
        );
    }

    #[test]
    fn closed_omissions_never_render_as_zero_empty_or_clean() {
        for reason in [
            ProjectionOmissionReasonV1::NotCapturedByPolicy,
            ProjectionOmissionReasonV1::ProjectionFailedClosed,
            ProjectionOmissionReasonV1::UnknownFuture,
        ] {
            let vm = SafeJsonShapeVm::omitted(reason);
            let json = serde_json::to_string(&vm).unwrap();
            assert!(json.contains("node_count\":null"));
            assert!(!json.contains("\"node_count\":0"));
            assert!(!json.to_ascii_lowercase().contains("clean"));
        }
    }
}
