use std::fmt;
use std::ops::Range;

use gaze::{
    CommittedSessionSnapshot, PrefixCacheWriteMode, RestoreError, RestoredTextWithProvenance,
};
use gaze_types::inspection::{
    JsonShapeSummaryV1, ProjectionAvailabilityV1, ProjectionOmissionReasonV1, SseTimelineMetaV1,
};

/// Transport-independent body representation selected by a reviewed adapter contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireFormat {
    Empty,
    Json,
    Ndjson,
    Utf8Text,
    Sse,
}

/// Closed phase reported by a codec failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecPhase {
    RequestParse,
    RequestProtection,
    RequestProof,
    ResponseParse,
    ResponseRestore,
    ResponseProof,
    SseDecode,
    SseLifecycle,
    SseReplay,
}

/// Closed, sanitized codec failure code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecErrorCode {
    InvalidUtf8,
    InvalidJson,
    InvalidWireFormat,
    DuplicateObjectKey,
    DepthLimitExceeded,
    NodeLimitExceeded,
    StringLimitExceeded,
    NumberLimitExceeded,
    BodyLimitExceeded,
    GrowthLimitExceeded,
    InternalCoverageFailure,
    UnsupportedRequestSurface,
    UnsupportedResponseSurface,
    ControlWouldMutate,
    OpaqueMediaUninspected,
    SignedMutationRequired,
    SignedSurfaceMalformed,
    ProtectionFailedClosed,
    RestoreFailedClosed,
    ProviderOriginNumeric,
    ProviderOriginPii,
    TransformedKeyCollision,
    InvalidSseField,
    InvalidSseFrame,
    InvalidSseEventPair,
    SseLineLimitExceeded,
    SseFrameLimitExceeded,
    SseEventLimitExceeded,
    SseIndexLimitExceeded,
    SseAccumulatorLimitExceeded,
    InvalidSseLifecycle,
    InvalidContentBlockIndex,
    InvalidContentBlockDelta,
    IncompleteSseStream,
    UnsafeFutureEvent,
    ProviderErrorEvent,
}

/// Sanitized codec failure. It never retains parser, body, key, value, or provider error text.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct CodecError {
    code: CodecErrorCode,
    phase: CodecPhase,
}

impl CodecError {
    pub(crate) const fn new(code: CodecErrorCode, phase: CodecPhase) -> Self {
        Self { code, phase }
    }

    #[must_use]
    pub const fn code(self) -> CodecErrorCode {
        self.code
    }

    #[must_use]
    pub const fn phase(self) -> CodecPhase {
        self.phase
    }
}

impl fmt::Debug for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodecError")
            .field("code", &self.code)
            .field("phase", &self.phase)
            .finish()
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "codec_{:?}_{:?}", self.phase, self.code)
    }
}

impl std::error::Error for CodecError {}

/// Reviewed finite codec limits. Builders may only lower these defaults in I3.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodecLimits {
    max_body_bytes: usize,
    max_json_depth: usize,
    max_json_nodes: usize,
    max_string_bytes: usize,
    max_number_bytes: usize,
    max_growth_numerator: usize,
    max_growth_denominator: usize,
    max_sse_line_bytes: usize,
    max_sse_frame_bytes: usize,
    max_sse_events: usize,
    max_sse_indices: usize,
    max_sse_accumulator_bytes: usize,
}

impl Default for CodecLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 32 * 1024 * 1024,
            max_json_depth: 128,
            max_json_nodes: 100_000,
            max_string_bytes: 4 * 1024 * 1024,
            max_number_bytes: 128,
            max_growth_numerator: 2,
            max_growth_denominator: 1,
            max_sse_line_bytes: 256 * 1024,
            max_sse_frame_bytes: 1024 * 1024,
            max_sse_events: 10_000,
            max_sse_indices: 256,
            max_sse_accumulator_bytes: 8 * 1024 * 1024,
        }
    }
}

impl CodecLimits {
    #[must_use]
    pub const fn with_max_body_bytes(mut self, value: usize) -> Self {
        self.max_body_bytes = value;
        self
    }

    #[must_use]
    pub const fn with_max_json_depth(mut self, value: usize) -> Self {
        self.max_json_depth = value;
        self
    }

    #[must_use]
    pub const fn with_max_json_nodes(mut self, value: usize) -> Self {
        self.max_json_nodes = value;
        self
    }

    #[must_use]
    pub const fn with_max_string_bytes(mut self, value: usize) -> Self {
        self.max_string_bytes = value;
        self
    }

    #[must_use]
    pub const fn with_max_number_bytes(mut self, value: usize) -> Self {
        self.max_number_bytes = value;
        self
    }

    #[must_use]
    pub const fn with_max_sse_line_bytes(mut self, value: usize) -> Self {
        self.max_sse_line_bytes = value;
        self
    }

    #[must_use]
    pub const fn with_max_sse_frame_bytes(mut self, value: usize) -> Self {
        self.max_sse_frame_bytes = value;
        self
    }

    #[must_use]
    pub const fn with_max_sse_events(mut self, value: usize) -> Self {
        self.max_sse_events = value;
        self
    }

    #[must_use]
    pub const fn with_max_sse_indices(mut self, value: usize) -> Self {
        self.max_sse_indices = value;
        self
    }

    #[must_use]
    pub const fn with_max_sse_accumulator_bytes(mut self, value: usize) -> Self {
        self.max_sse_accumulator_bytes = value;
        self
    }

    #[must_use]
    pub const fn max_body_bytes(self) -> usize {
        self.max_body_bytes
    }

    #[must_use]
    pub const fn max_json_depth(self) -> usize {
        self.max_json_depth
    }

    #[must_use]
    pub const fn max_json_nodes(self) -> usize {
        self.max_json_nodes
    }

    #[must_use]
    pub const fn max_string_bytes(self) -> usize {
        self.max_string_bytes
    }

    #[must_use]
    pub const fn max_number_bytes(self) -> usize {
        self.max_number_bytes
    }

    #[must_use]
    pub const fn max_sse_line_bytes(self) -> usize {
        self.max_sse_line_bytes
    }

    #[must_use]
    pub const fn max_sse_frame_bytes(self) -> usize {
        self.max_sse_frame_bytes
    }

    #[must_use]
    pub const fn max_sse_events(self) -> usize {
        self.max_sse_events
    }

    #[must_use]
    pub const fn max_sse_indices(self) -> usize {
        self.max_sse_indices
    }

    #[must_use]
    pub const fn max_sse_accumulator_bytes(self) -> usize {
        self.max_sse_accumulator_bytes
    }

    pub(crate) fn output_within_growth_limit(self, input: usize, output: usize) -> bool {
        let proportional = input
            .checked_mul(self.max_growth_numerator)
            .map(|value| value / self.max_growth_denominator)
            .unwrap_or(usize::MAX);
        output <= self.max_body_bytes && output <= proportional.max(input)
    }
}

/// Staged request protection boundary supplied by I4.
pub trait RequestPseudonymizer {
    /// Protects one complete decoded logical value against the staged transaction.
    fn protect(&mut self, input: &str) -> Result<String, CodecErrorCode>;

    /// Protects one decoded value with an explicit prefix-cache mode.
    ///
    /// [`PrefixCacheWriteMode::Allow`] preserves the source-compatible delegation to
    /// [`Self::protect`]. [`PrefixCacheWriteMode::Suppress`] fails closed unless an
    /// implementation explicitly overrides this method with a stateless or isolated
    /// no-read/no-write guarantee.
    fn protect_with_prefix_cache_write_mode(
        &mut self,
        input: &str,
        mode: PrefixCacheWriteMode,
    ) -> Result<String, CodecErrorCode> {
        match mode {
            PrefixCacheWriteMode::Allow => self.protect(input),
            PrefixCacheWriteMode::Suppress => Err(CodecErrorCode::ProtectionFailedClosed),
        }
    }

    /// Proves that every Gaze-shaped token belongs to the staged transaction.
    fn validate_token_shapes(&mut self, input: &str) -> Result<(), CodecErrorCode>;
}

/// Residual response proof boundary supplied by I4.
pub trait ResponseResidualValidator {
    /// Validates one complete restored logical value. Every finding must fit wholly in an
    /// authorized manifest-backed output range.
    fn validate(
        &mut self,
        value: &str,
        authorized_output_ranges: &[Range<usize>],
    ) -> Result<(), CodecErrorCode>;
}

/// Private-field request transformation context.
pub struct RequestTransformContext<'a> {
    pseudonymizer: &'a mut dyn RequestPseudonymizer,
    format: WireFormat,
    limits: CodecLimits,
    inspection_projection_requested: bool,
}

impl<'a> RequestTransformContext<'a> {
    #[must_use]
    pub(crate) fn new(
        pseudonymizer: &'a mut dyn RequestPseudonymizer,
        format: WireFormat,
        limits: CodecLimits,
    ) -> Self {
        Self {
            pseudonymizer,
            format,
            limits,
            inspection_projection_requested: false,
        }
    }

    #[must_use]
    pub const fn format(&self) -> WireFormat {
        self.format
    }

    #[must_use]
    pub const fn limits(&self) -> CodecLimits {
        self.limits
    }

    pub(crate) fn request_inspection_projection(&mut self) {
        self.inspection_projection_requested = true;
    }

    pub(crate) const fn inspection_projection_requested(&self) -> bool {
        self.inspection_projection_requested
    }

    pub(crate) fn protect(&mut self, input: &str) -> Result<String, CodecErrorCode> {
        self.pseudonymizer
            .protect_with_prefix_cache_write_mode(input, PrefixCacheWriteMode::Allow)
    }

    /// Runs an isolated mutation probe that may neither reuse nor publish prefix-cache state.
    pub(crate) fn probe_without_prefix_cache_write(
        &mut self,
        input: &str,
    ) -> Result<String, CodecErrorCode> {
        self.pseudonymizer
            .protect_with_prefix_cache_write_mode(input, PrefixCacheWriteMode::Suppress)
    }

    pub(crate) fn validate_token_shapes(&mut self, input: &str) -> Result<(), CodecErrorCode> {
        self.pseudonymizer.validate_token_shapes(input)
    }
}

/// Private-field response transformation context.
pub struct ResponseTransformContext<'a> {
    snapshot: &'a CommittedSessionSnapshot,
    residual: &'a mut dyn ResponseResidualValidator,
    format: WireFormat,
    limits: CodecLimits,
    inspection_projection_requested: bool,
}

impl<'a> ResponseTransformContext<'a> {
    #[must_use]
    pub(crate) fn new(
        snapshot: &'a CommittedSessionSnapshot,
        residual: &'a mut dyn ResponseResidualValidator,
        format: WireFormat,
        limits: CodecLimits,
    ) -> Self {
        Self {
            snapshot,
            residual,
            format,
            limits,
            inspection_projection_requested: false,
        }
    }

    #[must_use]
    pub const fn format(&self) -> WireFormat {
        self.format
    }

    #[must_use]
    pub const fn limits(&self) -> CodecLimits {
        self.limits
    }

    pub(crate) fn request_inspection_projection(&mut self) {
        self.inspection_projection_requested = true;
    }

    pub(crate) const fn inspection_projection_requested(&self) -> bool {
        self.inspection_projection_requested
    }

    pub(crate) fn restore(
        &mut self,
        input: &str,
    ) -> Result<RestoredTextWithProvenance, CodecErrorCode> {
        self.snapshot
            .restore_strict_text_with_provenance(input)
            .map_err(map_restore_error)
    }

    pub(crate) fn validate(
        &mut self,
        value: &str,
        authorized_output_ranges: &[Range<usize>],
    ) -> Result<(), CodecErrorCode> {
        self.residual.validate(value, authorized_output_ranges)
    }
}

fn map_restore_error(_error: RestoreError) -> CodecErrorCode {
    CodecErrorCode::RestoreFailedClosed
}

/// Output-coordinate proof retained beside the exact emitted buffer.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct OutputProvenance {
    authorized_output_ranges: Vec<Range<usize>>,
    signed_opaque_ranges: Vec<Range<usize>>,
    occurrence_count: usize,
    final_buffer_verified: bool,
}

impl OutputProvenance {
    #[must_use]
    pub fn authorized_output_ranges(&self) -> &[Range<usize>] {
        &self.authorized_output_ranges
    }

    #[must_use]
    pub fn signed_opaque_ranges(&self) -> &[Range<usize>] {
        &self.signed_opaque_ranges
    }

    #[must_use]
    pub const fn occurrence_count(&self) -> usize {
        self.occurrence_count
    }

    #[must_use]
    pub const fn final_buffer_verified(&self) -> bool {
        self.final_buffer_verified
    }

    pub(crate) fn record_authorized(&mut self, range: Range<usize>) {
        push_coalesced(&mut self.authorized_output_ranges, range);
    }

    pub(crate) fn record_signed(&mut self, range: Range<usize>) {
        push_coalesced(&mut self.signed_opaque_ranges, range);
    }

    pub(crate) fn set_occurrence_count(&mut self, count: usize) {
        self.occurrence_count = count;
    }

    pub(crate) fn mark_final_buffer_verified(&mut self) {
        self.final_buffer_verified = true;
    }
}

fn push_coalesced(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    if range.start >= range.end {
        return;
    }
    if let Some(previous) = ranges.last_mut() {
        if range.start <= previous.end {
            previous.end = previous.end.max(range.end);
            return;
        }
    }
    ranges.push(range);
}

/// Crate-private structural projection created lazily from codec-owned proved structure.
pub(crate) struct InspectionStructuralProjectionV1 {
    pub(crate) json_shape: ProjectionAvailabilityV1<JsonShapeSummaryV1>,
    pub(crate) sse_timeline: ProjectionAvailabilityV1<SseTimelineMetaV1>,
}

impl InspectionStructuralProjectionV1 {
    pub(crate) const fn unsupported() -> Self {
        Self {
            json_shape: ProjectionAvailabilityV1::Omitted(
                ProjectionOmissionReasonV1::UnsupportedFormat,
            ),
            sse_timeline: ProjectionAvailabilityV1::Omitted(
                ProjectionOmissionReasonV1::UnsupportedFormat,
            ),
        }
    }

    pub(crate) const fn unavailable(format: WireFormat) -> Self {
        match format {
            WireFormat::Json => Self {
                json_shape: ProjectionAvailabilityV1::Omitted(
                    ProjectionOmissionReasonV1::ProjectionFailedClosed,
                ),
                sse_timeline: ProjectionAvailabilityV1::Omitted(
                    ProjectionOmissionReasonV1::UnsupportedFormat,
                ),
            },
            WireFormat::Sse => Self {
                json_shape: ProjectionAvailabilityV1::Omitted(
                    ProjectionOmissionReasonV1::UnsupportedFormat,
                ),
                sse_timeline: ProjectionAvailabilityV1::Omitted(
                    ProjectionOmissionReasonV1::ProjectionFailedClosed,
                ),
            },
            WireFormat::Ndjson | WireFormat::Utf8Text | WireFormat::Empty => Self {
                json_shape: ProjectionAvailabilityV1::Omitted(
                    ProjectionOmissionReasonV1::UnsupportedFormat,
                ),
                sse_timeline: ProjectionAvailabilityV1::Omitted(
                    ProjectionOmissionReasonV1::UnsupportedFormat,
                ),
            },
        }
    }
}

/// Opaque crate-private parsed facts moved from the codec's existing proof pass.
///
/// These facts are retained only for an immutable startup-selected stage. Turning them into an
/// inspection source/timeline remains inside the `gaze-inspection` build closure, after its real
/// lifecycle pre-projection fence. Proved bytes are never reparsed by the proxy.
pub(crate) enum InspectionParsedFactsV1 {
    AnthropicJson(crate::anthropic::JsonInspectionParsedFactsV1),
    AnthropicSse(crate::anthropic::SseInspectionParsedFactsV1),
}

impl InspectionParsedFactsV1 {
    pub(crate) fn project(self) -> InspectionStructuralProjectionV1 {
        match self {
            Self::AnthropicJson(facts) => facts.project(),
            Self::AnthropicSse(facts) => facts.project(),
        }
    }
}

macro_rules! proved_body {
    ($name:ident) => {
        pub struct $name {
            bytes: Vec<u8>,
            format: WireFormat,
            provenance: OutputProvenance,
            inspection_parsed_facts: Option<InspectionParsedFactsV1>,
            signed_or_encrypted_surface: bool,
        }

        impl $name {
            pub(crate) fn new(
                bytes: Vec<u8>,
                format: WireFormat,
                provenance: OutputProvenance,
            ) -> Self {
                let signed_or_encrypted_surface = !provenance.signed_opaque_ranges().is_empty();
                Self {
                    bytes,
                    format,
                    provenance,
                    inspection_parsed_facts: None,
                    signed_or_encrypted_surface,
                }
            }

            pub(crate) fn new_with_inspection_parsed_facts(
                bytes: Vec<u8>,
                format: WireFormat,
                provenance: OutputProvenance,
                inspection_parsed_facts: InspectionParsedFactsV1,
            ) -> Self {
                let signed_or_encrypted_surface = !provenance.signed_opaque_ranges().is_empty();
                Self {
                    bytes,
                    format,
                    provenance,
                    inspection_parsed_facts: (!signed_or_encrypted_surface)
                        .then_some(inspection_parsed_facts),
                    signed_or_encrypted_surface,
                }
            }

            #[must_use]
            pub fn bytes(&self) -> &[u8] {
                &self.bytes
            }

            #[must_use]
            pub const fn format(&self) -> WireFormat {
                self.format
            }

            #[must_use]
            pub const fn provenance(&self) -> &OutputProvenance {
                &self.provenance
            }

            pub(crate) const fn signed_or_encrypted_surface(&self) -> bool {
                self.signed_or_encrypted_surface
            }

            pub(crate) fn into_inspection_parts(
                self,
            ) -> (Vec<u8>, Option<InspectionParsedFactsV1>, bool) {
                (
                    self.bytes,
                    self.inspection_parsed_facts,
                    self.signed_or_encrypted_surface,
                )
            }

            pub(crate) fn take_inspection_parsed_facts(
                &mut self,
            ) -> Option<InspectionParsedFactsV1> {
                self.inspection_parsed_facts.take()
            }

            #[must_use]
            pub fn into_bytes(self) -> Vec<u8> {
                self.bytes
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(stringify!($name))
            }
        }
    };
}

proved_body!(ProvedRequestBody);
proved_body!(ProvedResponseBody);

/// Object-safe, synchronous, transport-free body codec.
pub trait BodyCodec: Send + Sync + 'static {
    fn protect_request(
        &self,
        input: &[u8],
        ctx: &mut RequestTransformContext<'_>,
    ) -> Result<ProvedRequestBody, CodecError>;

    fn restore_response(
        &self,
        input: &[u8],
        ctx: &mut ResponseTransformContext<'_>,
    ) -> Result<ProvedResponseBody, CodecError>;
}
