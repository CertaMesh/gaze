//! Provider-neutral, bounded inspection values.
//!
//! This module deliberately contains no sensitive payload ownership, queue, sink, HTTP, async,
//! provider-adapter, or dashboard runtime. Content-derived values are suitable only inside a
//! startup-authorized domain projection; [`InspectionEventMetaV1`] is the hostile-sink-safe
//! metadata surface.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum JSON-shape nodes in one authorized projection.
pub const MAX_JSON_SHAPE_NODES_V1: usize = 1_024;
/// Maximum SSE timeline entries in one authorized projection.
pub const MAX_SSE_TIMELINE_ENTRIES_V1: usize = 10_000;
/// Maximum PII summary entries in one authorized projection.
pub const MAX_PII_SUMMARY_ENTRIES_V1: usize = 256;
/// Maximum decision trace entries in one authorized projection.
pub const MAX_DECISION_TRACE_ENTRIES_V1: usize = 256;

/// Closed validation failures for inspection values.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum InspectionValueErrorV1 {
    /// A stage/domain pair is not one of the four reviewed V1 pairs.
    #[error("invalid_inspection_stage_domain")]
    InvalidStageDomain,
    /// A bounded projection exceeded its entry cap.
    #[error("inspection_projection_entry_limit")]
    EntryLimit,
    /// A strictly monotonic sequence would overflow.
    #[error("inspection_sequence_overflow")]
    SequenceOverflow,
}

macro_rules! ordinal_newtype {
    ($name:ident, $inner:ty) => {
        #[doc = concat!("Bounded provider-neutral `", stringify!($name), "` value.")]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name($inner);

        impl $name {
            /// Creates the value from a proxy-owned bounded integer.
            #[must_use]
            pub const fn new(value: $inner) -> Self {
                Self(value)
            }

            /// Returns the bounded integer representation.
            #[must_use]
            pub const fn get(self) -> $inner {
                self.0
            }
        }
    };
}

ordinal_newtype!(LogicalInspectionIdV1, u64);
ordinal_newtype!(InspectionEmissionIdV1, u64);
ordinal_newtype!(InspectionEpochV1, u64);
ordinal_newtype!(JsonNodeOrdinalV1, u32);
ordinal_newtype!(SseEntryOrdinalV1, u32);
ordinal_newtype!(ContentBlockIndexV1, u32);

/// Strictly monotonic producer-assigned order within one logical inspection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct InspectionSequenceV1(u64);

impl InspectionSequenceV1 {
    /// Creates a sequence from its bounded integer representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the bounded integer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the sequence without reuse or wrapping.
    pub fn checked_next(self) -> Result<Self, InspectionValueErrorV1> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(InspectionValueErrorV1::SequenceOverflow)
    }
}

/// User-visible inspection stage.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionStageV1 {
    /// Owner request before protection.
    OwnerRequest,
    /// Request representation sent to the provider.
    ProviderRequest,
    /// Response representation received from the provider.
    ProviderResponse,
    /// Owner response after restoration.
    OwnerRestoredResponse,
}

/// Sensitive-domain identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionDomainV1 {
    /// Original data-owner bytes.
    OwnerRaw,
    /// Pseudonymized provider-visible bytes.
    ProviderVisible,
    /// Restored data-owner bytes.
    OwnerRestored,
}

impl InspectionDomainV1 {
    /// Exhaustive V1 domain set.
    pub const ALL: [Self; 3] = [Self::OwnerRaw, Self::ProviderVisible, Self::OwnerRestored];
}

/// The exhaustive set of valid stage/domain pairs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionStageDomainV1 {
    /// Owner request with OwnerRaw payload.
    OwnerRequest,
    /// Provider request with ProviderVisible payload.
    ProviderRequest,
    /// Provider response with ProviderVisible payload.
    ProviderResponse,
    /// Owner-restored response with OwnerRestored payload.
    OwnerRestoredResponse,
}

impl InspectionStageDomainV1 {
    /// Returns the stage discriminant.
    #[must_use]
    pub const fn stage(self) -> InspectionStageV1 {
        match self {
            Self::OwnerRequest => InspectionStageV1::OwnerRequest,
            Self::ProviderRequest => InspectionStageV1::ProviderRequest,
            Self::ProviderResponse => InspectionStageV1::ProviderResponse,
            Self::OwnerRestoredResponse => InspectionStageV1::OwnerRestoredResponse,
        }
    }

    /// Returns the authorized domain discriminant.
    #[must_use]
    pub const fn domain(self) -> InspectionDomainV1 {
        match self {
            Self::OwnerRequest => InspectionDomainV1::OwnerRaw,
            Self::ProviderRequest | Self::ProviderResponse => InspectionDomainV1::ProviderVisible,
            Self::OwnerRestoredResponse => InspectionDomainV1::OwnerRestored,
        }
    }

    /// Validates a stage/domain pair against the exhaustive V1 matrix.
    pub const fn try_from_parts(
        stage: InspectionStageV1,
        domain: InspectionDomainV1,
    ) -> Result<Self, InspectionValueErrorV1> {
        match (stage, domain) {
            (InspectionStageV1::OwnerRequest, InspectionDomainV1::OwnerRaw) => {
                Ok(Self::OwnerRequest)
            }
            (InspectionStageV1::ProviderRequest, InspectionDomainV1::ProviderVisible) => {
                Ok(Self::ProviderRequest)
            }
            (InspectionStageV1::ProviderResponse, InspectionDomainV1::ProviderVisible) => {
                Ok(Self::ProviderResponse)
            }
            (InspectionStageV1::OwnerRestoredResponse, InspectionDomainV1::OwnerRestored) => {
                Ok(Self::OwnerRestoredResponse)
            }
            _ => Err(InspectionValueErrorV1::InvalidStageDomain),
        }
    }
}

/// Closed logical route identity; it never carries a URL or path.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteCodeV1 {
    /// Reviewed Anthropic Messages logical route.
    AnthropicMessages,
    /// A route whose spelling is intentionally not retained.
    Unknown,
    /// A rejected route whose spelling is intentionally not retained.
    Rejected,
}

/// Closed provider-profile identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProfileCodeV1 {
    AnthropicMessagesDirect,
    OpenAiCompatible,
    GeminiCompatible,
    GenericLocalAnthropicCompatible,
    OtherReviewed,
}

/// Closed endpoint-selection identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointSelectionCodeV1 {
    FixedHttpsOrigin,
    ConfiguredHttpsOrigin,
    OsAssignedLoopback,
    DynamicallyDiscoveredLoopback,
    RevalidatedLoopback,
    UnavailableFailClosed,
}

/// Closed port-selection identity; it never carries a numeric port.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortSelectionCodeV1 {
    NotApplicable,
    FixedConfigured,
    OsAssigned,
    DynamicallyDiscovered,
    ChangedAndRevalidated,
    UnavailableFailClosed,
}

/// Hostile-sink-safe operational status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionOperationalStatusV1 {
    Accepted,
    Omitted,
    Purging,
    Disabled,
    Closed,
    FailedClosed,
}

/// Hostile-sink-safe closed error code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionErrorCodeV1 {
    None,
    ProjectionFailedClosed,
    SequenceOverflow,
    EpochOverflow,
    SinkRejected,
    SinkPanicked,
    RuntimeClosed,
    InternalFailClosed,
}

/// Hostile-sink-safe drop reason.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionDropCodeV1 {
    None,
    QueueFull,
    QueueByteLimit,
    QueueContended,
    QueueClosed,
    StaleEpoch,
    Purging,
    Disabled,
    SequenceOverflow,
    DuplicateConflict,
    PartialEvent,
}

/// Hostile-sink-safe delivery state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionDeliveryCodeV1 {
    Pending,
    Delivered,
    Dropped,
    SinkRejected,
    SinkPanicked,
    Closed,
}

/// Coarse duration only; exact timestamps and deltas are intentionally unrepresentable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoarseDurationBucketV1 {
    NotMeasured,
    UnderOneHundredMilliseconds,
    OneHundredMillisecondsToOneSecond,
    OneToTenSeconds,
    OverTenSeconds,
}

/// Immutable startup capture-domain selection, with all eight combinations explicit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureDomainsV1 {
    MetadataOnly,
    OwnerRaw,
    ProviderVisible,
    OwnerRestored,
    OwnerRawAndProviderVisible,
    OwnerRawAndOwnerRestored,
    ProviderVisibleAndOwnerRestored,
    All,
}

impl CaptureDomainsV1 {
    /// Exhaustive V1 startup combinations.
    pub const ALL: [Self; 8] = [
        Self::MetadataOnly,
        Self::OwnerRaw,
        Self::ProviderVisible,
        Self::OwnerRestored,
        Self::OwnerRawAndProviderVisible,
        Self::OwnerRawAndOwnerRestored,
        Self::ProviderVisibleAndOwnerRestored,
        Self::All,
    ];

    /// Reports whether a domain was authorized at immutable startup.
    #[must_use]
    pub const fn captures(self, domain: InspectionDomainV1) -> bool {
        matches!(
            (self, domain),
            (Self::OwnerRaw, InspectionDomainV1::OwnerRaw)
                | (Self::ProviderVisible, InspectionDomainV1::ProviderVisible)
                | (Self::OwnerRestored, InspectionDomainV1::OwnerRestored)
                | (
                    Self::OwnerRawAndProviderVisible,
                    InspectionDomainV1::OwnerRaw
                )
                | (
                    Self::OwnerRawAndProviderVisible,
                    InspectionDomainV1::ProviderVisible
                )
                | (Self::OwnerRawAndOwnerRestored, InspectionDomainV1::OwnerRaw)
                | (
                    Self::OwnerRawAndOwnerRestored,
                    InspectionDomainV1::OwnerRestored
                )
                | (
                    Self::ProviderVisibleAndOwnerRestored,
                    InspectionDomainV1::ProviderVisible
                )
                | (
                    Self::ProviderVisibleAndOwnerRestored,
                    InspectionDomainV1::OwnerRestored
                )
                | (Self::All, _)
        )
    }
}

/// Exact immutable startup descriptor shared by the pending halves.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct DashboardCaptureDescriptorV1 {
    domains: CaptureDomainsV1,
}

impl DashboardCaptureDescriptorV1 {
    /// Creates a descriptor from the closed domain set.
    #[must_use]
    pub const fn new(domains: CaptureDomainsV1) -> Self {
        Self { domains }
    }

    /// Returns the immutable domain selection.
    #[must_use]
    pub const fn domains(self) -> CaptureDomainsV1 {
        self.domains
    }

    /// Reports whether a domain was authorized at startup.
    #[must_use]
    pub const fn captures(self, domain: InspectionDomainV1) -> bool {
        self.domains.captures(domain)
    }
}

/// Typed, content-free surface reference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionSurfaceRefV1 {
    WholeBody,
    JsonNode(JsonNodeOrdinalV1),
    SseEntry(SseEntryOrdinalV1),
}

/// Mandatory reason for a domain or projection not being present.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionOmissionReasonV1 {
    NotCapturedByPolicy,
    NotApplicable,
    UnsupportedFormat,
    MalformedFailClosed,
    LimitExceeded,
    SignedOrEncryptedSurface,
    Purging,
    Disabled,
    QueueClosed,
    ProjectionFailedClosed,
    UnknownFuture,
}

/// Total projection availability. A missing value always has a closed reason.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionAvailabilityV1<T> {
    Present(T),
    Omitted(ProjectionOmissionReasonV1),
}

/// Closed JSON node type.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonTypeCodeV1 {
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
}

/// One ordinal/type-only JSON shape entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JsonShapeNodeV1 {
    ordinal: JsonNodeOrdinalV1,
    type_code: JsonTypeCodeV1,
}

impl JsonShapeNodeV1 {
    #[must_use]
    pub const fn new(ordinal: JsonNodeOrdinalV1, type_code: JsonTypeCodeV1) -> Self {
        Self { ordinal, type_code }
    }

    #[must_use]
    pub const fn ordinal(self) -> JsonNodeOrdinalV1 {
        self.ordinal
    }

    #[must_use]
    pub const fn type_code(self) -> JsonTypeCodeV1 {
        self.type_code
    }
}

/// Whole-projection truncation marker; no lengths or byte counts are carried.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonShapeTruncationCodeV1 {
    Complete,
    NodeLimit,
    DepthLimit,
    UnknownFuture,
}

/// Bounded JSON ordinal/type summary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JsonShapeSummaryV1 {
    nodes: Vec<JsonShapeNodeV1>,
    truncation: JsonShapeTruncationCodeV1,
}

impl JsonShapeSummaryV1 {
    pub fn try_new(
        nodes: Vec<JsonShapeNodeV1>,
        truncation: JsonShapeTruncationCodeV1,
    ) -> Result<Self, InspectionValueErrorV1> {
        if nodes.len() > MAX_JSON_SHAPE_NODES_V1 {
            return Err(InspectionValueErrorV1::EntryLimit);
        }
        Ok(Self { nodes, truncation })
    }

    #[must_use]
    pub fn nodes(&self) -> &[JsonShapeNodeV1] {
        &self.nodes
    }

    #[must_use]
    pub const fn truncation(&self) -> JsonShapeTruncationCodeV1 {
        self.truncation
    }
}

/// Closed SSE event kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SseEventKindV1 {
    MessageStart,
    ContentBlockStart,
    ContentBlockDelta,
    ContentBlockStop,
    MessageDelta,
    MessageStop,
    Ping,
    Error,
    UnknownFuture,
}

/// Closed SSE delta kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SseDeltaKindV1 {
    Text,
    InputJson,
    Thinking,
    Signature,
    Citation,
    UnknownFuture,
}

/// One ordinal/kind/index-only SSE timeline entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SseTimelineEntryV1 {
    ordinal: SseEntryOrdinalV1,
    event_kind: SseEventKindV1,
    delta_kind: ProjectionAvailabilityV1<SseDeltaKindV1>,
    content_block: ProjectionAvailabilityV1<ContentBlockIndexV1>,
}

impl SseTimelineEntryV1 {
    #[must_use]
    pub const fn new(
        ordinal: SseEntryOrdinalV1,
        event_kind: SseEventKindV1,
        delta_kind: ProjectionAvailabilityV1<SseDeltaKindV1>,
        content_block: ProjectionAvailabilityV1<ContentBlockIndexV1>,
    ) -> Self {
        Self {
            ordinal,
            event_kind,
            delta_kind,
            content_block,
        }
    }

    #[must_use]
    pub const fn ordinal(&self) -> SseEntryOrdinalV1 {
        self.ordinal
    }
}

/// Bounded SSE timeline without bytes or timing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SseTimelineMetaV1 {
    entries: Vec<SseTimelineEntryV1>,
}

impl SseTimelineMetaV1 {
    pub fn try_new(entries: Vec<SseTimelineEntryV1>) -> Result<Self, InspectionValueErrorV1> {
        if entries.len() > MAX_SSE_TIMELINE_ENTRIES_V1 {
            return Err(InspectionValueErrorV1::EntryLimit);
        }
        Ok(Self { entries })
    }

    #[must_use]
    pub fn entries(&self) -> &[SseTimelineEntryV1] {
        &self.entries
    }
}

/// Closed PII class for an authorized diagnostic projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiClassCodeV1 {
    PersonName,
    Email,
    Phone,
    Address,
    Organization,
    Credential,
    OtherReviewed,
    UnknownFuture,
}

/// One bounded PII-class count projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PiiCountV1 {
    class: PiiClassCodeV1,
    detected: u32,
    pseudonymized: u32,
}

impl PiiCountV1 {
    #[must_use]
    pub const fn new(class: PiiClassCodeV1, detected: u32, pseudonymized: u32) -> Self {
        Self {
            class,
            detected,
            pseudonymized,
        }
    }
}

/// Bounded PII summary retained only inside an authorized domain projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PiiSummaryV1 {
    entries: Vec<PiiCountV1>,
}

impl PiiSummaryV1 {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn try_new(entries: Vec<PiiCountV1>) -> Result<Self, InspectionValueErrorV1> {
        if entries.len() > MAX_PII_SUMMARY_ENTRIES_V1 {
            return Err(InspectionValueErrorV1::EntryLimit);
        }
        Ok(Self { entries })
    }

    #[must_use]
    pub fn entries(&self) -> &[PiiCountV1] {
        &self.entries
    }
}

/// Closed decision engine.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionEngineCodeV1 {
    DeterministicRecognizer,
    Ner,
    NeuralSafetyNet,
    ResidualScan,
}

/// Closed decision outcome.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionOutcomeCodeV1 {
    RanClean,
    RanFindings,
    SkippedNotApplicable,
    UnavailableFailClosed,
    TimeoutFailClosed,
    ErrorFailClosed,
}

/// One decision entry without strings, scores, spans, offsets, or timing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DecisionTraceEntryV1 {
    engine: DecisionEngineCodeV1,
    outcome: DecisionOutcomeCodeV1,
    surface: InspectionSurfaceRefV1,
    pii_class: ProjectionAvailabilityV1<PiiClassCodeV1>,
    detected: u32,
    pseudonymized: u32,
}

impl DecisionTraceEntryV1 {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        engine: DecisionEngineCodeV1,
        outcome: DecisionOutcomeCodeV1,
        surface: InspectionSurfaceRefV1,
        pii_class: ProjectionAvailabilityV1<PiiClassCodeV1>,
        detected: u32,
        pseudonymized: u32,
    ) -> Self {
        Self {
            engine,
            outcome,
            surface,
            pii_class,
            detected,
            pseudonymized,
        }
    }
}

/// Bounded decision trace retained only inside an authorized domain projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DecisionTraceV1 {
    entries: Vec<DecisionTraceEntryV1>,
}

impl DecisionTraceV1 {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn try_new(entries: Vec<DecisionTraceEntryV1>) -> Result<Self, InspectionValueErrorV1> {
        if entries.len() > MAX_DECISION_TRACE_ENTRIES_V1 {
            return Err(InspectionValueErrorV1::EntryLimit);
        }
        Ok(Self { entries })
    }

    #[must_use]
    pub fn entries(&self) -> &[DecisionTraceEntryV1] {
        &self.entries
    }
}

/// Closed attestation projection without digest or identity material.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttestationTraceV1 {
    NotApplicable,
    ExactHit,
    MissNoRecord,
    MissExpired,
    MissEvicted,
    InvalidatedContentBytes,
    InvalidatedStructuralIdentity,
    InvalidatedPolicyOrRulepack,
    InvalidatedRecognizerOrArtifact,
    InvalidatedLocaleOrDictionary,
    InvalidatedNormalizationVersion,
    InvalidatedSessionEpoch,
    InvalidatedConfigurationGeneration,
    AmbiguousFailClosed,
    UnavailableFailClosed,
    UnknownFuture,
}

/// Exact aggregate measurements, usable only inside an authorized domain projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectionMeasurementV1 {
    byte_count: u64,
    chunk_count: u32,
}

impl InspectionMeasurementV1 {
    #[must_use]
    pub const fn new(byte_count: u64, chunk_count: u32) -> Self {
        Self {
            byte_count,
            chunk_count,
        }
    }

    #[must_use]
    pub const fn byte_count(self) -> u64 {
        self.byte_count
    }

    #[must_use]
    pub const fn chunk_count(self) -> u32 {
        self.chunk_count
    }
}

/// Saturating operational queue counters safe for hostile MetadataOnly sinks.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectionQueueSnapshotV1 {
    admitted: u64,
    dropped: u64,
    delivered: u64,
}

impl InspectionQueueSnapshotV1 {
    #[must_use]
    pub const fn new(admitted: u64, dropped: u64, delivered: u64) -> Self {
        Self {
            admitted,
            dropped,
            delivered,
        }
    }
}

/// Closed safe fields supplied by a producer before IDs and stage are assigned.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectionSafeFieldsV1 {
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

impl InspectionSafeFieldsV1 {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
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
    ) -> Self {
        Self {
            route,
            profile,
            endpoint,
            port,
            status,
            error,
            drop,
            delivery,
            duration,
            queue,
        }
    }
}

/// Safe metadata schema version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionMetadataVersionV1 {
    V1,
}

/// Versioned hostile-sink-safe metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectionEventMetaV1 {
    version: InspectionMetadataVersionV1,
    logical_id: LogicalInspectionIdV1,
    emission_id: InspectionEmissionIdV1,
    sequence: InspectionSequenceV1,
    epoch: InspectionEpochV1,
    stage_domain: InspectionStageDomainV1,
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

impl InspectionEventMetaV1 {
    #[must_use]
    pub const fn new(
        logical_id: LogicalInspectionIdV1,
        emission_id: InspectionEmissionIdV1,
        sequence: InspectionSequenceV1,
        epoch: InspectionEpochV1,
        stage_domain: InspectionStageDomainV1,
        safe: InspectionSafeFieldsV1,
    ) -> Self {
        Self {
            version: InspectionMetadataVersionV1::V1,
            logical_id,
            emission_id,
            sequence,
            epoch,
            stage_domain,
            route: safe.route,
            profile: safe.profile,
            endpoint: safe.endpoint,
            port: safe.port,
            status: safe.status,
            error: safe.error,
            drop: safe.drop,
            delivery: safe.delivery,
            duration: safe.duration,
            queue: safe.queue,
        }
    }

    #[must_use]
    pub const fn logical_id(&self) -> LogicalInspectionIdV1 {
        self.logical_id
    }

    #[must_use]
    pub const fn emission_id(&self) -> InspectionEmissionIdV1 {
        self.emission_id
    }

    #[must_use]
    pub const fn sequence(&self) -> InspectionSequenceV1 {
        self.sequence
    }

    #[must_use]
    pub const fn epoch(&self) -> InspectionEpochV1 {
        self.epoch
    }

    #[must_use]
    pub const fn stage_domain(&self) -> InspectionStageDomainV1 {
        self.stage_domain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_domain_matrix_is_exhaustive_and_rejects_invalid_pairs() {
        let valid = [
            InspectionStageDomainV1::OwnerRequest,
            InspectionStageDomainV1::ProviderRequest,
            InspectionStageDomainV1::ProviderResponse,
            InspectionStageDomainV1::OwnerRestoredResponse,
        ];
        assert_eq!(valid.len(), 4);
        for pair in valid {
            assert_eq!(
                InspectionStageDomainV1::try_from_parts(pair.stage(), pair.domain()),
                Ok(pair)
            );
        }
        assert!(InspectionStageDomainV1::try_from_parts(
            InspectionStageV1::OwnerRequest,
            InspectionDomainV1::ProviderVisible,
        )
        .is_err());
        assert_ne!(
            InspectionStageDomainV1::ProviderRequest,
            InspectionStageDomainV1::ProviderResponse
        );
    }

    #[test]
    fn capture_descriptor_covers_all_closed_startup_domain_combinations() {
        for domains in CaptureDomainsV1::ALL {
            let descriptor = DashboardCaptureDescriptorV1::new(domains);
            assert_eq!(descriptor.domains(), domains);
            for domain in InspectionDomainV1::ALL {
                assert_eq!(descriptor.captures(domain), domains.captures(domain));
            }
        }
    }

    #[test]
    fn safe_metadata_has_only_closed_operational_fields() {
        let metadata = InspectionEventMetaV1::new(
            LogicalInspectionIdV1::new(7),
            InspectionEmissionIdV1::new(9),
            InspectionSequenceV1::new(1),
            InspectionEpochV1::new(0),
            InspectionStageDomainV1::ProviderRequest,
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
            ),
        );
        let value = serde_json::to_value(metadata).unwrap();
        let object = value.as_object().unwrap();
        let keys: Vec<_> = object.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "delivery",
                "drop",
                "duration",
                "emission_id",
                "endpoint",
                "epoch",
                "error",
                "logical_id",
                "port",
                "profile",
                "queue",
                "route",
                "sequence",
                "stage_domain",
                "status",
                "version",
            ]
        );
        let serialized = serde_json::to_string(&value).unwrap();
        for forbidden in [
            "payload",
            "byte_count",
            "chunk_count",
            "pii",
            "json",
            "sse",
            "decision",
            "attestation",
            "hash",
            "digest",
            "offset",
            "path",
            "key",
            "timestamp",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "forbidden field {forbidden}"
            );
        }
    }

    #[test]
    fn json_and_sse_projections_expose_only_the_rev7_allowlists() {
        let json = JsonShapeSummaryV1::try_new(
            vec![JsonShapeNodeV1::new(
                JsonNodeOrdinalV1::new(0),
                JsonTypeCodeV1::Object,
            )],
            JsonShapeTruncationCodeV1::Complete,
        )
        .unwrap();
        let sse = SseTimelineMetaV1::try_new(vec![SseTimelineEntryV1::new(
            SseEntryOrdinalV1::new(0),
            SseEventKindV1::ContentBlockDelta,
            ProjectionAvailabilityV1::Present(SseDeltaKindV1::Text),
            ProjectionAvailabilityV1::Present(ContentBlockIndexV1::new(2)),
        )])
        .unwrap();
        assert_eq!(
            serde_json::to_value(json).unwrap(),
            serde_json::json!({
                "nodes": [{"ordinal": 0, "type_code": "object"}],
                "truncation": "complete"
            })
        );
        assert_eq!(
            serde_json::to_value(sse).unwrap(),
            serde_json::json!({
                "entries": [{
                    "ordinal": 0,
                    "event_kind": "content_block_delta",
                    "delta_kind": {"present": "text"},
                    "content_block": {"present": 2}
                }]
            })
        );
    }

    #[test]
    fn projection_availability_is_total_and_sequence_overflow_is_closed() {
        let omitted: ProjectionAvailabilityV1<JsonShapeSummaryV1> =
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotCapturedByPolicy);
        assert!(matches!(
            omitted,
            ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotCapturedByPolicy)
        ));
        assert_eq!(
            InspectionSequenceV1::new(u64::MAX).checked_next(),
            Err(InspectionValueErrorV1::SequenceOverflow)
        );
    }

    #[test]
    fn unknown_codes_reject_and_projection_caps_are_enforced() {
        assert!(serde_json::from_str::<RouteCodeV1>("\"caller_route\"").is_err());
        assert!(serde_json::from_str::<DecisionEngineCodeV1>("\"model_name\"").is_err());
        assert_eq!(
            JsonShapeSummaryV1::try_new(
                vec![
                    JsonShapeNodeV1::new(JsonNodeOrdinalV1::new(0), JsonTypeCodeV1::Null,);
                    MAX_JSON_SHAPE_NODES_V1 + 1
                ],
                JsonShapeTruncationCodeV1::NodeLimit,
            ),
            Err(InspectionValueErrorV1::EntryLimit)
        );
    }
}
