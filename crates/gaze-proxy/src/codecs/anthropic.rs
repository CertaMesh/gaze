use std::collections::{BTreeMap, HashSet};
use std::ops::Range;

#[cfg(test)]
use std::cell::Cell;

use gaze_types::inspection::{
    ContentBlockIndexV1, JsonNodeOrdinalV1, JsonShapeNodeV1, JsonShapeSummaryV1,
    JsonShapeTruncationCodeV1, JsonTypeCodeV1, ProjectionAvailabilityV1,
    ProjectionOmissionReasonV1, SseDeltaKindV1, SseEntryOrdinalV1, SseEventKindV1,
    SseTimelineEntryV1, SseTimelineMetaV1, MAX_JSON_SHAPE_NODES_V1,
};

use crate::codec::{
    BodyCodec, CodecError, CodecErrorCode, CodecLimits, CodecPhase, InspectionParsedFactsV1,
    InspectionStructuralProjectionV1, OutputProvenance, ProvedRequestBody, ProvedResponseBody,
    RequestTransformContext, ResponseTransformContext, WireFormat,
};

pub const ANTHROPIC_PROXY_PING_FRAME: &[u8] = b"event: ping\ndata: {\"type\":\"ping\"}\n\n";
pub const ANTHROPIC_PROXY_ERROR_FRAME: &[u8] = b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"proxy_validation_failed\"}}\n\n";
const MAX_ANTHROPIC_REQUEST_BODY_BYTES: usize = 2 * 1024 * 1024;
// Mirror the frozen default JSON-depth ceiling for wrapper syntax inside one string.
const MAX_ANGLE_WRAPPER_DEPTH: usize = 128;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct InspectionConstructionCountsV1 {
    pub(crate) json_sources_built: usize,
    pub(crate) sse_sources_built: usize,
    pub(crate) sse_timelines_built: usize,
}

#[cfg(test)]
thread_local! {
    static INSPECTION_CONSTRUCTION_COUNTS: Cell<InspectionConstructionCountsV1> =
        const { Cell::new(InspectionConstructionCountsV1 {
            json_sources_built: 0,
            sse_sources_built: 0,
            sse_timelines_built: 0,
        }) };
}

#[cfg(test)]
pub(crate) fn reset_inspection_construction_counts() {
    INSPECTION_CONSTRUCTION_COUNTS.with(|counts| {
        counts.set(InspectionConstructionCountsV1::default());
    });
}

#[cfg(test)]
pub(crate) fn inspection_construction_counts() -> InspectionConstructionCountsV1 {
    INSPECTION_CONSTRUCTION_COUNTS.with(Cell::get)
}

#[cfg(test)]
fn record_inspection_construction(update: impl FnOnce(&mut InspectionConstructionCountsV1)) {
    INSPECTION_CONSTRUCTION_COUNTS.with(|cell| {
        let mut counts = cell.get();
        update(&mut counts);
        cell.set(counts);
    });
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AnthropicMessagesCodec;

impl BodyCodec for AnthropicMessagesCodec {
    fn protect_request(
        &self,
        input: &[u8],
        ctx: &mut RequestTransformContext<'_>,
    ) -> Result<ProvedRequestBody, CodecError> {
        if ctx.format() != WireFormat::Json {
            return Err(codec_error(
                CodecErrorCode::InvalidWireFormat,
                CodecPhase::RequestParse,
            ));
        }
        if input.len()
            > ctx
                .limits()
                .max_body_bytes()
                .min(MAX_ANTHROPIC_REQUEST_BODY_BYTES)
        {
            return Err(codec_error(
                CodecErrorCode::BodyLimitExceeded,
                CodecPhase::RequestParse,
            ));
        }
        let mut document = parse_json(input, ctx.limits(), CodecPhase::RequestParse)?;
        let mut ledger = CoverageLedger::new(document.occurrence_count);
        transform_request_root(&mut document.root, input, ctx, &mut ledger)
            .map_err(|code| codec_error(code, CodecPhase::RequestProtection))?;
        ledger
            .finish()
            .map_err(|code| codec_error(code, CodecPhase::RequestProof))?;
        let expected_signed = collect_signed_slices(&document.root, input)
            .map_err(|code| codec_error(code, CodecPhase::RequestProof))?;
        let expected_authorized = collect_authorized_wire_chunks(&document.root)
            .map_err(|code| codec_error(code, CodecPhase::RequestProof))?;
        let (bytes, mut provenance) = write_document(&document, input)
            .map_err(|code| codec_error(code, CodecPhase::RequestProof))?;
        if bytes.len()
            > ctx
                .limits()
                .max_body_bytes()
                .min(MAX_ANTHROPIC_REQUEST_BODY_BYTES)
        {
            return Err(codec_error(
                CodecErrorCode::BodyLimitExceeded,
                CodecPhase::RequestProof,
            ));
        }
        if !ctx
            .limits()
            .output_within_growth_limit(input.len(), bytes.len())
        {
            return Err(codec_error(
                CodecErrorCode::GrowthLimitExceeded,
                CodecPhase::RequestProof,
            ));
        }
        let mut reparsed = parse_json(&bytes, ctx.limits(), CodecPhase::RequestProof)?;
        let mut final_ledger = CoverageLedger::new(reparsed.occurrence_count);
        transform_request_root(&mut reparsed.root, &bytes, ctx, &mut final_ledger)
            .map_err(|code| codec_error(code, CodecPhase::RequestProof))?;
        final_ledger
            .finish()
            .map_err(|code| codec_error(code, CodecPhase::RequestProof))?;
        if !json_semantically_equal(&document.root, &reparsed.root) {
            return Err(codec_error(
                CodecErrorCode::InternalCoverageFailure,
                CodecPhase::RequestProof,
            ));
        }
        let (reproved, _) = write_document(&reparsed, &bytes)
            .map_err(|code| codec_error(code, CodecPhase::RequestProof))?;
        if reproved != bytes {
            return Err(codec_error(
                CodecErrorCode::InternalCoverageFailure,
                CodecPhase::RequestProof,
            ));
        }
        provenance.set_occurrence_count(reparsed.occurrence_count);
        verify_final_wire_proof(&bytes, &provenance, &expected_authorized, &expected_signed)
            .map_err(|code| codec_error(code, CodecPhase::RequestProof))?;
        provenance.mark_final_buffer_verified();
        if ctx.inspection_projection_requested() && provenance.signed_opaque_ranges().is_empty() {
            Ok(ProvedRequestBody::new_with_inspection_parsed_facts(
                bytes,
                WireFormat::Json,
                provenance,
                InspectionParsedFactsV1::AnthropicJson(JsonInspectionParsedFactsV1 {
                    root: reparsed.root,
                }),
            ))
        } else {
            Ok(ProvedRequestBody::new(bytes, WireFormat::Json, provenance))
        }
    }

    fn restore_response(
        &self,
        input: &[u8],
        ctx: &mut ResponseTransformContext<'_>,
    ) -> Result<ProvedResponseBody, CodecError> {
        if input.len() > ctx.limits().max_body_bytes() {
            return Err(codec_error(
                CodecErrorCode::BodyLimitExceeded,
                CodecPhase::ResponseParse,
            ));
        }
        match ctx.format() {
            WireFormat::Json => restore_json_response(input, ctx),
            WireFormat::Sse => self.restore_sse_chunks([input], ctx),
            WireFormat::Empty if input.is_empty() => {
                let mut provenance = OutputProvenance::default();
                provenance.mark_final_buffer_verified();
                Ok(ProvedResponseBody::new(
                    Vec::new(),
                    WireFormat::Empty,
                    provenance,
                ))
            }
            WireFormat::Utf8Text => restore_text_response(input, ctx),
            WireFormat::Ndjson => restore_ndjson_response(input, ctx),
            WireFormat::Empty => Err(codec_error(
                CodecErrorCode::InvalidWireFormat,
                CodecPhase::ResponseParse,
            )),
        }
    }
}

impl AnthropicMessagesCodec {
    pub fn restore_sse_chunks<I, B>(
        &self,
        chunks: I,
        ctx: &mut ResponseTransformContext<'_>,
    ) -> Result<ProvedResponseBody, CodecError>
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        if ctx.format() != WireFormat::Sse {
            return Err(codec_error(
                CodecErrorCode::InvalidWireFormat,
                CodecPhase::SseDecode,
            ));
        }
        let mut decoder = SseDecoder::new(ctx.limits());
        for chunk in chunks {
            decoder.push(chunk.as_ref())?;
        }
        let input_len = decoder.input_len();
        let frames = decoder.finish()?;
        let proved = restore_sse_frames(frames, ctx)?;
        if !ctx
            .limits()
            .output_within_growth_limit(input_len, proved.bytes().len())
        {
            return Err(codec_error(
                CodecErrorCode::GrowthLimitExceeded,
                CodecPhase::SseReplay,
            ));
        }
        Ok(proved)
    }
}

fn codec_error(code: CodecErrorCode, phase: CodecPhase) -> CodecError {
    CodecError::new(code, phase)
}

fn restore_json_response(
    input: &[u8],
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<ProvedResponseBody, CodecError> {
    let mut document = parse_json(input, ctx.limits(), CodecPhase::ResponseParse)?;
    let mut ledger = CoverageLedger::new(document.occurrence_count);
    transform_response_root(&mut document.root, input, ctx, &mut ledger)
        .map_err(|code| codec_error(code, CodecPhase::ResponseRestore))?;
    ledger
        .finish()
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    validate_adjacent_response_text(&document.root, ctx)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    let expected_signed = collect_signed_slices(&document.root, input)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    let expected_authorized = collect_authorized_wire_chunks(&document.root)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    let (bytes, mut provenance) = write_document(&document, input)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    if !ctx
        .limits()
        .output_within_growth_limit(input.len(), bytes.len())
    {
        return Err(codec_error(
            CodecErrorCode::GrowthLimitExceeded,
            CodecPhase::ResponseProof,
        ));
    }
    let reparsed = parse_json(&bytes, ctx.limits(), CodecPhase::ResponseProof)?;
    let mut final_ledger = CoverageLedger::new(reparsed.occurrence_count);
    account_subtree(&reparsed.root, &mut final_ledger)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    final_ledger
        .finish()
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    if !json_semantically_equal(&document.root, &reparsed.root) {
        return Err(codec_error(
            CodecErrorCode::InternalCoverageFailure,
            CodecPhase::ResponseProof,
        ));
    }
    let (reproved, _) = write_document(&reparsed, &bytes)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    if reproved != bytes {
        return Err(codec_error(
            CodecErrorCode::InternalCoverageFailure,
            CodecPhase::ResponseProof,
        ));
    }
    provenance.set_occurrence_count(reparsed.occurrence_count);
    verify_final_wire_proof(&bytes, &provenance, &expected_authorized, &expected_signed)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    provenance.mark_final_buffer_verified();
    if ctx.inspection_projection_requested() && provenance.signed_opaque_ranges().is_empty() {
        Ok(ProvedResponseBody::new_with_inspection_parsed_facts(
            bytes,
            WireFormat::Json,
            provenance,
            InspectionParsedFactsV1::AnthropicJson(JsonInspectionParsedFactsV1 {
                root: reparsed.root,
            }),
        ))
    } else {
        Ok(ProvedResponseBody::new(bytes, WireFormat::Json, provenance))
    }
}

fn validate_adjacent_response_text(
    root: &JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<(), CodecErrorCode> {
    let JsonKind::Object(root_members) = &root.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    let content = root_members
        .iter()
        .find(|member| member.key.value == "content")
        .ok_or(CodecErrorCode::UnsupportedResponseSurface)?;
    let JsonKind::Array(blocks) = &content.value.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    let mut logical = String::new();
    let mut authorized = Vec::<Range<usize>>::new();
    for block in blocks {
        if object_string(block, "type")? != "text" {
            continue;
        }
        let JsonKind::Object(members) = &block.kind else {
            return Err(CodecErrorCode::UnsupportedResponseSurface);
        };
        let text = members
            .iter()
            .find(|member| member.key.value == "text")
            .ok_or(CodecErrorCode::UnsupportedResponseSurface)?;
        let JsonKind::String(value) = &text.value.kind else {
            return Err(CodecErrorCode::UnsupportedResponseSurface);
        };
        let base = logical.len();
        logical.push_str(&value.value);
        authorized.extend(
            value
                .authorized
                .iter()
                .map(|range| base + range.start..base + range.end),
        );
    }
    ctx.validate(&logical, &authorized)
}

fn restore_text_response(
    input: &[u8],
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<ProvedResponseBody, CodecError> {
    let text = std::str::from_utf8(input)
        .map_err(|_| codec_error(CodecErrorCode::InvalidUtf8, CodecPhase::ResponseParse))?;
    let restored = ctx
        .restore(text)
        .map_err(|code| codec_error(code, CodecPhase::ResponseRestore))?;
    guard_mutable_text_response(&restored.text)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    ctx.validate(&restored.text, &restored.authorized_output_ranges)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    if !ctx
        .limits()
        .output_within_growth_limit(input.len(), restored.text.len())
    {
        return Err(codec_error(
            CodecErrorCode::GrowthLimitExceeded,
            CodecPhase::ResponseProof,
        ));
    }
    let output = restored.text.into_bytes();
    let mut provenance = OutputProvenance::default();
    let mut expected_authorized = Vec::new();
    for range in restored.authorized_output_ranges {
        expected_authorized.push(
            output
                .get(range.clone())
                .ok_or_else(|| {
                    codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::ResponseProof,
                    )
                })?
                .to_vec(),
        );
        provenance.record_authorized(range);
    }
    let final_text = std::str::from_utf8(&output)
        .map_err(|_| codec_error(CodecErrorCode::InvalidUtf8, CodecPhase::ResponseProof))?;
    guard_mutable_text_response(final_text)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    ctx.validate(final_text, provenance.authorized_output_ranges())
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    verify_final_wire_proof(&output, &provenance, &expected_authorized, &[])
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    provenance.mark_final_buffer_verified();
    Ok(ProvedResponseBody::new(
        output,
        WireFormat::Utf8Text,
        provenance,
    ))
}

fn restore_ndjson_response(
    input: &[u8],
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<ProvedResponseBody, CodecError> {
    let text = std::str::from_utf8(input)
        .map_err(|_| codec_error(CodecErrorCode::InvalidUtf8, CodecPhase::ResponseParse))?;
    let mut output = Vec::with_capacity(input.len());
    let mut provenance = OutputProvenance::default();
    let mut expected_authorized = Vec::<Vec<u8>>::new();
    let mut expected_signed = Vec::<Vec<u8>>::new();
    let mut expected_authorized_ranges = Vec::<Range<usize>>::new();
    let mut expected_signed_ranges = Vec::<Range<usize>>::new();
    for line in text.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        if body.trim().is_empty() {
            output.extend_from_slice(newline.as_bytes());
            continue;
        }
        let proved = restore_json_response(body.as_bytes(), ctx)?;
        let offset = output.len();
        for range in proved.provenance().authorized_output_ranges() {
            append_expected_wire_chunk(
                proved.bytes(),
                range,
                offset,
                &mut expected_authorized_ranges,
                &mut expected_authorized,
            )
            .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
        }
        for range in proved.provenance().signed_opaque_ranges() {
            append_expected_wire_chunk(
                proved.bytes(),
                range,
                offset,
                &mut expected_signed_ranges,
                &mut expected_signed,
            )
            .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
        }
        output.extend_from_slice(proved.bytes());
        for range in proved.provenance().authorized_output_ranges() {
            provenance.record_authorized(offset + range.start..offset + range.end);
        }
        for range in proved.provenance().signed_opaque_ranges() {
            provenance.record_signed(offset + range.start..offset + range.end);
        }
        output.extend_from_slice(newline.as_bytes());
    }
    if output.len() > ctx.limits().max_body_bytes() {
        return Err(codec_error(
            CodecErrorCode::BodyLimitExceeded,
            CodecPhase::ResponseProof,
        ));
    }
    if !ctx
        .limits()
        .output_within_growth_limit(input.len(), output.len())
    {
        return Err(codec_error(
            CodecErrorCode::GrowthLimitExceeded,
            CodecPhase::ResponseProof,
        ));
    }
    let final_occurrence_count = reprove_ndjson_output(&output, ctx.limits())?;
    provenance.set_occurrence_count(final_occurrence_count);
    verify_final_wire_proof(&output, &provenance, &expected_authorized, &expected_signed)
        .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
    provenance.mark_final_buffer_verified();
    Ok(ProvedResponseBody::new(
        output,
        WireFormat::Ndjson,
        provenance,
    ))
}

fn reprove_ndjson_output(output: &[u8], limits: CodecLimits) -> Result<usize, CodecError> {
    let text = std::str::from_utf8(output)
        .map_err(|_| codec_error(CodecErrorCode::InvalidUtf8, CodecPhase::ResponseProof))?;
    let mut occurrence_count = 0usize;
    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if body.trim().is_empty() {
            continue;
        }
        let document = parse_json(body.as_bytes(), limits, CodecPhase::ResponseProof)?;
        let mut ledger = CoverageLedger::new(document.occurrence_count);
        account_subtree(&document.root, &mut ledger)
            .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
        ledger
            .finish()
            .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
        prove_response_root_shape(&document.root)
            .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
        let (reproved, _) = write_document(&document, body.as_bytes())
            .map_err(|code| codec_error(code, CodecPhase::ResponseProof))?;
        if reproved != body.as_bytes() {
            return Err(codec_error(
                CodecErrorCode::InternalCoverageFailure,
                CodecPhase::ResponseProof,
            ));
        }
        occurrence_count = occurrence_count
            .checked_add(document.occurrence_count)
            .ok_or_else(|| {
                codec_error(CodecErrorCode::NodeLimitExceeded, CodecPhase::ResponseProof)
            })?;
    }
    Ok(occurrence_count)
}

struct JsonDocument {
    root: JsonNode,
    occurrence_count: usize,
}

struct JsonNode {
    occurrence: usize,
    span: Range<usize>,
    raw_preserve: bool,
    kind: JsonKind,
}

enum JsonKind {
    Object(Vec<JsonMember>),
    Array(Vec<JsonNode>),
    String(JsonString),
    Number(String),
    Bool(bool),
    Null,
}

pub(crate) struct JsonInspectionParsedFactsV1 {
    root: JsonNode,
}

pub(crate) struct SseInspectionParsedFactsV1 {
    frames: Vec<SseFrame>,
    complete: bool,
}

struct JsonInspectionProjectionSourceV1 {
    root: JsonNode,
}

struct SseInspectionProjectionSourceV1 {
    frames: Vec<SseFrame>,
    complete: bool,
}

impl SseInspectionParsedFactsV1 {
    pub(crate) fn project(self) -> InspectionStructuralProjectionV1 {
        #[cfg(test)]
        record_inspection_construction(|counts| counts.sse_sources_built += 1);
        SseInspectionProjectionSourceV1 {
            frames: self.frames,
            complete: self.complete,
        }
        .project()
    }
}

impl SseInspectionProjectionSourceV1 {
    fn project(self) -> InspectionStructuralProjectionV1 {
        #[cfg(test)]
        record_inspection_construction(|counts| counts.sse_timelines_built += 1);
        let entries = self
            .complete
            .then(|| {
                self.frames
                    .into_iter()
                    .filter_map(|frame| frame.inspection_fact)
                    .enumerate()
                    .map(|(ordinal, fact)| {
                        let ordinal = u32::try_from(ordinal).ok()?;
                        Some(SseTimelineEntryV1::new(
                            SseEntryOrdinalV1::new(ordinal),
                            fact.event_kind,
                            fact.delta_kind,
                            fact.content_block,
                        ))
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .flatten();
        let sse_timeline = entries
            .and_then(|entries| SseTimelineMetaV1::try_new(entries).ok())
            .map_or_else(
                || {
                    ProjectionAvailabilityV1::Omitted(
                        ProjectionOmissionReasonV1::ProjectionFailedClosed,
                    )
                },
                ProjectionAvailabilityV1::Present,
            );
        InspectionStructuralProjectionV1 {
            json_shape: ProjectionAvailabilityV1::Omitted(
                ProjectionOmissionReasonV1::UnsupportedFormat,
            ),
            sse_timeline,
        }
    }
}

impl JsonInspectionParsedFactsV1 {
    pub(crate) fn project(self) -> InspectionStructuralProjectionV1 {
        #[cfg(test)]
        record_inspection_construction(|counts| counts.json_sources_built += 1);
        JsonInspectionProjectionSourceV1 { root: self.root }.project()
    }
}

impl JsonInspectionProjectionSourceV1 {
    fn project(self) -> InspectionStructuralProjectionV1 {
        let mut nodes = Vec::with_capacity(MAX_JSON_SHAPE_NODES_V1.min(64));
        let mut truncated = false;
        collect_json_shape(&self.root, &mut nodes, &mut truncated);
        let truncation = if truncated {
            JsonShapeTruncationCodeV1::NodeLimit
        } else {
            JsonShapeTruncationCodeV1::Complete
        };
        let json_shape = JsonShapeSummaryV1::try_new(nodes, truncation).map_or_else(
            |_| {
                ProjectionAvailabilityV1::Omitted(
                    ProjectionOmissionReasonV1::ProjectionFailedClosed,
                )
            },
            ProjectionAvailabilityV1::Present,
        );
        InspectionStructuralProjectionV1 {
            json_shape,
            sse_timeline: ProjectionAvailabilityV1::Omitted(
                ProjectionOmissionReasonV1::UnsupportedFormat,
            ),
        }
    }
}

fn collect_json_shape(node: &JsonNode, output: &mut Vec<JsonShapeNodeV1>, truncated: &mut bool) {
    if output.len() >= MAX_JSON_SHAPE_NODES_V1 {
        *truncated = true;
        return;
    }
    let ordinal = JsonNodeOrdinalV1::new(output.len() as u32);
    let type_code = match &node.kind {
        JsonKind::Object(_) => JsonTypeCodeV1::Object,
        JsonKind::Array(_) => JsonTypeCodeV1::Array,
        JsonKind::String(_) => JsonTypeCodeV1::String,
        JsonKind::Number(_) => JsonTypeCodeV1::Number,
        JsonKind::Bool(_) => JsonTypeCodeV1::Boolean,
        JsonKind::Null => JsonTypeCodeV1::Null,
    };
    output.push(JsonShapeNodeV1::new(ordinal, type_code));
    match &node.kind {
        JsonKind::Object(members) => {
            for member in members {
                collect_json_shape(&member.value, output, truncated);
            }
        }
        JsonKind::Array(values) => {
            for value in values {
                collect_json_shape(value, output, truncated);
            }
        }
        JsonKind::String(_) | JsonKind::Number(_) | JsonKind::Bool(_) | JsonKind::Null => {}
    }
}

struct JsonMember {
    key_occurrence: usize,
    key: JsonString,
    value: JsonNode,
}

struct JsonString {
    value: String,
    authorized: Vec<Range<usize>>,
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    position: usize,
    limits: CodecLimits,
    nodes: usize,
    next_occurrence: usize,
}

fn parse_json(
    input: &[u8],
    limits: CodecLimits,
    phase: CodecPhase,
) -> Result<JsonDocument, CodecError> {
    if input.len() > limits.max_body_bytes() {
        return Err(codec_error(CodecErrorCode::BodyLimitExceeded, phase));
    }
    std::str::from_utf8(input).map_err(|_| codec_error(CodecErrorCode::InvalidUtf8, phase))?;
    let mut parser = JsonParser {
        bytes: input,
        position: 0,
        limits,
        nodes: 0,
        next_occurrence: 0,
    };
    parser.skip_whitespace();
    let root = parser
        .parse_value(0)
        .map_err(|code| codec_error(code, phase))?;
    parser.skip_whitespace();
    if parser.position != input.len() {
        return Err(codec_error(CodecErrorCode::InvalidJson, phase));
    }
    Ok(JsonDocument {
        root,
        occurrence_count: parser.next_occurrence,
    })
}

impl JsonParser<'_> {
    fn parse_value(&mut self, depth: usize) -> Result<JsonNode, CodecErrorCode> {
        if depth > self.limits.max_json_depth() {
            return Err(CodecErrorCode::DepthLimitExceeded);
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or(CodecErrorCode::NodeLimitExceeded)?;
        if self.nodes > self.limits.max_json_nodes() {
            return Err(CodecErrorCode::NodeLimitExceeded);
        }
        let occurrence = self.occurrence();
        let start = self.position;
        let kind = match self.peek() {
            Some(b'{') => self.parse_object(depth + 1)?,
            Some(b'[') => self.parse_array(depth + 1)?,
            Some(b'"') => JsonKind::String(self.parse_string()?),
            Some(b't') => {
                self.consume_literal(b"true")?;
                JsonKind::Bool(true)
            }
            Some(b'f') => {
                self.consume_literal(b"false")?;
                JsonKind::Bool(false)
            }
            Some(b'n') => {
                self.consume_literal(b"null")?;
                JsonKind::Null
            }
            Some(b'-' | b'0'..=b'9') => JsonKind::Number(self.parse_number()?),
            _ => return Err(CodecErrorCode::InvalidJson),
        };
        Ok(JsonNode {
            occurrence,
            span: start..self.position,
            raw_preserve: false,
            kind,
        })
    }

    fn parse_object(&mut self, depth: usize) -> Result<JsonKind, CodecErrorCode> {
        self.position += 1;
        self.skip_whitespace();
        let mut members = Vec::new();
        let mut keys = HashSet::new();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(JsonKind::Object(members));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err(CodecErrorCode::InvalidJson);
            }
            let key_occurrence = self.occurrence();
            let key = self.parse_string()?;
            if !key.value.is_ascii() {
                return Err(CodecErrorCode::InvalidJson);
            }
            if !keys.insert(key.value.clone()) {
                return Err(CodecErrorCode::DuplicateObjectKey);
            }
            self.skip_whitespace();
            if self.take() != Some(b':') {
                return Err(CodecErrorCode::InvalidJson);
            }
            self.skip_whitespace();
            let value = self.parse_value(depth)?;
            members.push(JsonMember {
                key_occurrence,
                key,
                value,
            });
            self.skip_whitespace();
            match self.take() {
                Some(b',') => self.skip_whitespace(),
                Some(b'}') => break,
                _ => return Err(CodecErrorCode::InvalidJson),
            }
        }
        Ok(JsonKind::Object(members))
    }

    fn parse_array(&mut self, depth: usize) -> Result<JsonKind, CodecErrorCode> {
        self.position += 1;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(JsonKind::Array(values));
        }
        loop {
            values.push(self.parse_value(depth)?);
            self.skip_whitespace();
            match self.take() {
                Some(b',') => self.skip_whitespace(),
                Some(b']') => break,
                _ => return Err(CodecErrorCode::InvalidJson),
            }
        }
        Ok(JsonKind::Array(values))
    }

    fn parse_string(&mut self) -> Result<JsonString, CodecErrorCode> {
        if self.take() != Some(b'"') {
            return Err(CodecErrorCode::InvalidJson);
        }
        let mut value = String::new();
        loop {
            let byte = self.take().ok_or(CodecErrorCode::InvalidJson)?;
            match byte {
                b'"' => break,
                b'\\' => self.parse_escape(&mut value)?,
                0x00..=0x1f => return Err(CodecErrorCode::InvalidJson),
                0x20..=0x7f => value.push(char::from(byte)),
                _ => {
                    let start = self.position - 1;
                    let remaining = std::str::from_utf8(&self.bytes[start..])
                        .map_err(|_| CodecErrorCode::InvalidUtf8)?;
                    let character = remaining
                        .chars()
                        .next()
                        .ok_or(CodecErrorCode::InvalidUtf8)?;
                    value.push(character);
                    self.position = start + character.len_utf8();
                }
            }
            if value.len() > self.limits.max_string_bytes() {
                return Err(CodecErrorCode::StringLimitExceeded);
            }
        }
        Ok(JsonString {
            value,
            authorized: Vec::new(),
        })
    }

    fn parse_escape(&mut self, output: &mut String) -> Result<(), CodecErrorCode> {
        match self.take().ok_or(CodecErrorCode::InvalidJson)? {
            b'"' => output.push('"'),
            b'\\' => output.push('\\'),
            b'/' => output.push('/'),
            b'b' => output.push('\u{0008}'),
            b'f' => output.push('\u{000c}'),
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b't' => output.push('\t'),
            b'u' => {
                let first = self.parse_hex_quad()?;
                let scalar = if (0xd800..=0xdbff).contains(&first) {
                    if self.take() != Some(b'\\') || self.take() != Some(b'u') {
                        return Err(CodecErrorCode::InvalidJson);
                    }
                    let second = self.parse_hex_quad()?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return Err(CodecErrorCode::InvalidJson);
                    }
                    0x1_0000 + (((u32::from(first) - 0xd800) << 10) | (u32::from(second) - 0xdc00))
                } else if (0xdc00..=0xdfff).contains(&first) {
                    return Err(CodecErrorCode::InvalidJson);
                } else {
                    u32::from(first)
                };
                output.push(char::from_u32(scalar).ok_or(CodecErrorCode::InvalidJson)?);
            }
            _ => return Err(CodecErrorCode::InvalidJson),
        }
        Ok(())
    }

    fn parse_hex_quad(&mut self) -> Result<u16, CodecErrorCode> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self
                .take()
                .and_then(hex_value)
                .ok_or(CodecErrorCode::InvalidJson)?;
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<String, CodecErrorCode> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        match self.take() {
            Some(b'0') if matches!(self.peek(), Some(b'0'..=b'9')) => {
                return Err(CodecErrorCode::InvalidJson);
            }
            Some(b'0'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.position += 1;
                }
            }
            _ => return Err(CodecErrorCode::InvalidJson),
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(CodecErrorCode::InvalidJson);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.position += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(CodecErrorCode::InvalidJson);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.position += 1;
            }
        }
        let raw = &self.bytes[start..self.position];
        if raw.len() > self.limits.max_number_bytes() {
            return Err(CodecErrorCode::NumberLimitExceeded);
        }
        Ok(std::str::from_utf8(raw)
            .map_err(|_| CodecErrorCode::InvalidUtf8)?
            .to_owned())
    }

    fn consume_literal(&mut self, literal: &[u8]) -> Result<(), CodecErrorCode> {
        if self.bytes.get(self.position..self.position + literal.len()) != Some(literal) {
            return Err(CodecErrorCode::InvalidJson);
        }
        self.position += literal.len();
        Ok(())
    }

    fn occurrence(&mut self) -> usize {
        let occurrence = self.next_occurrence;
        self.next_occurrence += 1;
        occurrence
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.position += 1;
        Some(value)
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

struct CoverageLedger {
    visited: Vec<bool>,
}

impl CoverageLedger {
    fn new(count: usize) -> Self {
        Self {
            visited: vec![false; count],
        }
    }

    fn mark(&mut self, occurrence: usize) -> Result<(), CodecErrorCode> {
        let slot = self
            .visited
            .get_mut(occurrence)
            .ok_or(CodecErrorCode::InternalCoverageFailure)?;
        if *slot {
            return Err(CodecErrorCode::InternalCoverageFailure);
        }
        *slot = true;
        Ok(())
    }

    fn finish(self) -> Result<(), CodecErrorCode> {
        if self.visited.into_iter().all(|visited| visited) {
            Ok(())
        } else {
            Err(CodecErrorCode::InternalCoverageFailure)
        }
    }
}

fn account_subtree(node: &JsonNode, ledger: &mut CoverageLedger) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    match &node.kind {
        JsonKind::Object(members) => {
            for member in members {
                ledger.mark(member.key_occurrence)?;
                account_subtree(&member.value, ledger)?;
            }
        }
        JsonKind::Array(values) => {
            for value in values {
                account_subtree(value, ledger)?;
            }
        }
        JsonKind::String(_) | JsonKind::Number(_) | JsonKind::Bool(_) | JsonKind::Null => {}
    }
    Ok(())
}

fn transform_request_root(
    node: &mut JsonNode,
    input: &[u8],
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    let mut saw_model = false;
    let mut saw_max_tokens = false;
    let mut saw_messages = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "model" => {
                request_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_model = true;
            }
            "service_tier" => request_enum_string_control(
                &mut member.value,
                &["auto", "standard_only"],
                ctx,
                ledger,
            )?,
            "max_tokens" => {
                request_integer_control(&member.value, ledger, true)?;
                saw_max_tokens = true;
            }
            "top_k" => request_integer_control(&member.value, ledger, false)?,
            "temperature" | "top_p" => request_unit_interval_control(&member.value, ledger)?,
            "stream" => request_bool_control(&member.value, ledger)?,
            "stop_sequences" => request_string_array_control(&mut member.value, ctx, ledger)?,
            "system" => transform_request_system(&mut member.value, input, ctx, ledger)?,
            "messages" => {
                transform_request_messages(&mut member.value, input, ctx, ledger)?;
                saw_messages = true;
            }
            "tools" => transform_request_tools(&mut member.value, input, ctx, ledger)?,
            "metadata" => transform_request_mutable(&mut member.value, ctx, ledger)?,
            "thinking" => transform_thinking_control(&mut member.value, ctx, ledger)?,
            "tool_choice" => transform_tool_choice_control(&mut member.value, ctx, ledger)?,
            "cache_control" => transform_cache_control(&mut member.value, ctx, ledger)?,
            _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
        }
    }
    if saw_model && saw_max_tokens && saw_messages {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn transform_request_messages(
    node: &mut JsonNode,
    input: &[u8],
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Array(messages) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    for message in messages {
        ledger.mark(message.occurrence)?;
        let JsonKind::Object(members) = &mut message.kind else {
            return Err(CodecErrorCode::UnsupportedRequestSurface);
        };
        let mut saw_role = false;
        let mut saw_content = false;
        for member in members {
            ledger.mark(member.key_occurrence)?;
            match member.key.value.as_str() {
                "role" => {
                    request_enum_string_control(
                        &mut member.value,
                        &["user", "assistant"],
                        ctx,
                        ledger,
                    )?;
                    saw_role = true;
                }
                "content" => {
                    transform_request_content(&mut member.value, input, ctx, ledger)?;
                    saw_content = true;
                }
                _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
            }
        }
        if !saw_role || !saw_content {
            return Err(CodecErrorCode::UnsupportedRequestSurface);
        }
    }
    Ok(())
}

fn transform_request_content(
    node: &mut JsonNode,
    input: &[u8],
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    match &mut node.kind {
        JsonKind::String(_) => transform_request_string(node, ctx, ledger),
        JsonKind::Array(blocks) => {
            ledger.mark(node.occurrence)?;
            if blocks.is_empty() {
                return Err(CodecErrorCode::UnsupportedRequestSurface);
            }
            for block in blocks {
                transform_request_block(block, input, ctx, ledger)?;
            }
            Ok(())
        }
        _ => Err(CodecErrorCode::UnsupportedRequestSurface),
    }
}

fn transform_request_system(
    node: &mut JsonNode,
    input: &[u8],
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    match &mut node.kind {
        JsonKind::String(_) => transform_request_string(node, ctx, ledger),
        JsonKind::Array(blocks) => {
            ledger.mark(node.occurrence)?;
            if blocks.is_empty() {
                return Err(CodecErrorCode::UnsupportedRequestSurface);
            }
            for block in blocks {
                if object_string(block, "type")? != "text" {
                    return Err(CodecErrorCode::UnsupportedRequestSurface);
                }
                transform_request_block(block, input, ctx, ledger)?;
            }
            Ok(())
        }
        _ => Err(CodecErrorCode::UnsupportedRequestSurface),
    }
}

fn transform_request_block(
    node: &mut JsonNode,
    input: &[u8],
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    let block_type = object_string(node, "type")?.to_owned();
    if !matches!(
        block_type.as_str(),
        "text"
            | "tool_use"
            | "tool_result"
            | "document"
            | "search_result"
            | "custom_content"
            | "thinking"
            | "redacted_thinking"
            | "image"
    ) {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    if block_type == "image" {
        return Err(CodecErrorCode::OpaqueMediaUninspected);
    }
    if matches!(block_type.as_str(), "thinking" | "redacted_thinking") {
        validate_signed_request_block(node, input, ctx, ledger, &block_type)?;
        node.raw_preserve = true;
        return Ok(());
    }
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    let mut saw_text = false;
    let mut saw_id = false;
    let mut saw_name = false;
    let mut saw_input = false;
    let mut saw_tool_use_id = false;
    let mut saw_content = false;
    let mut saw_source = false;
    let mut saw_user_payload = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match (block_type.as_str(), member.key.value.as_str()) {
            (_, "type") => {
                request_enum_string_control(
                    &mut member.value,
                    &[block_type.as_str()],
                    ctx,
                    ledger,
                )?;
            }
            (_, "cache_control") => transform_cache_control(&mut member.value, ctx, ledger)?,
            ("text", "text") => {
                transform_request_string(&mut member.value, ctx, ledger)?;
                saw_text = true;
            }
            ("tool_use", "id") => {
                request_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_id = true;
            }
            ("tool_use", "name") => {
                request_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_name = true;
            }
            ("tool_use", "input") => {
                transform_request_mutable(&mut member.value, ctx, ledger)?;
                saw_input = true;
            }
            ("tool_result", "tool_use_id") => {
                request_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_tool_use_id = true;
            }
            ("tool_result", "content") => {
                transform_request_content(&mut member.value, input, ctx, ledger)?;
                saw_content = true;
            }
            ("tool_result", "is_error") => request_bool_control(&member.value, ledger)?,
            ("document", "source") => {
                transform_request_document_source(&mut member.value, ctx, ledger)?;
                saw_source = true;
            }
            ("document", "title" | "context") => {
                transform_request_mutable(&mut member.value, ctx, ledger)?
            }
            ("search_result" | "custom_content", "title" | "context" | "text" | "content") => {
                transform_request_mutable(&mut member.value, ctx, ledger)?;
                saw_user_payload = true;
            }
            _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
        }
    }
    let valid = match block_type.as_str() {
        "text" => saw_text,
        "tool_use" => saw_id && saw_name && saw_input,
        "tool_result" => saw_tool_use_id && saw_content,
        "document" => saw_source,
        "search_result" | "custom_content" => saw_user_payload,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn validate_signed_request_block(
    node: &JsonNode,
    input: &[u8],
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
    block_type: &str,
) -> Result<(), CodecErrorCode> {
    account_subtree(node, ledger)?;
    let JsonKind::Object(members) = &node.kind else {
        return Err(CodecErrorCode::SignedSurfaceMalformed);
    };
    let mut has_payload = false;
    let mut has_signature = false;
    for member in members {
        match member.key.value.as_str() {
            "type" if string_value(&member.value)? == block_type => {}
            "cache_control" => validate_cache_control_probe(&member.value, ctx)?,
            "signature" => {
                ctx.validate_token_shapes(string_value(&member.value)?)?;
                has_signature = true;
            }
            "thinking" if block_type == "thinking" => {
                let text = string_value(&member.value)?;
                guard_mutable_text(text)?;
                ctx.validate_token_shapes(text)?;
                if ctx.probe_without_prefix_cache_write(text)? != text {
                    return Err(CodecErrorCode::SignedMutationRequired);
                }
                has_payload = true;
            }
            "data" if block_type == "redacted_thinking" => {
                ctx.validate_token_shapes(string_value(&member.value)?)?;
                has_payload = true;
            }
            _ => return Err(CodecErrorCode::SignedSurfaceMalformed),
        }
    }
    if !has_payload || (block_type == "thinking" && !has_signature) || node.span.end > input.len() {
        return Err(CodecErrorCode::SignedSurfaceMalformed);
    }
    Ok(())
}

fn probe_signed_request_value(
    node: &JsonNode,
    ctx: &mut RequestTransformContext<'_>,
) -> Result<(), CodecErrorCode> {
    match &node.kind {
        JsonKind::String(value) => {
            ctx.validate_token_shapes(&value.value)?;
            if ctx.probe_without_prefix_cache_write(&value.value)? != value.value {
                return Err(CodecErrorCode::SignedMutationRequired);
            }
        }
        JsonKind::Object(members) => {
            for member in members {
                ctx.validate_token_shapes(&member.key.value)?;
                if ctx.probe_without_prefix_cache_write(&member.key.value)? != member.key.value {
                    return Err(CodecErrorCode::SignedMutationRequired);
                }
                probe_signed_request_value(&member.value, ctx)?;
            }
        }
        JsonKind::Array(values) => {
            for value in values {
                probe_signed_request_value(value, ctx)?;
            }
        }
        JsonKind::Number(_) | JsonKind::Bool(_) | JsonKind::Null => {}
    }
    Ok(())
}

fn transform_request_document_source(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    let source_type = object_string(node, "type")?.to_owned();
    if source_type != "text" && source_type != "content" {
        return Err(CodecErrorCode::OpaqueMediaUninspected);
    }
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    let mut saw_payload = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "type" => request_enum_string_control(
                &mut member.value,
                &[source_type.as_str()],
                ctx,
                ledger,
            )?,
            "media_type" => request_string_control(&mut member.value, ctx, ledger)?,
            "data" | "text" | "content" => {
                transform_request_mutable(&mut member.value, ctx, ledger)?;
                saw_payload = true;
            }
            _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
        }
    }
    if saw_payload {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn transform_request_tools(
    node: &mut JsonNode,
    _input: &[u8],
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Array(tools) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    if tools.is_empty() {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    for tool in tools {
        ledger.mark(tool.occurrence)?;
        let JsonKind::Object(members) = &mut tool.kind else {
            return Err(CodecErrorCode::UnsupportedRequestSurface);
        };
        let mut saw_name = false;
        let mut saw_schema = false;
        for member in members {
            ledger.mark(member.key_occurrence)?;
            match member.key.value.as_str() {
                "name" => {
                    request_nonempty_string_control(&mut member.value, ctx, ledger)?;
                    saw_name = true;
                }
                "type" => request_enum_string_control(&mut member.value, &["custom"], ctx, ledger)?,
                "cache_control" => transform_cache_control(&mut member.value, ctx, ledger)?,
                "description" => transform_request_string(&mut member.value, ctx, ledger)?,
                "input_schema" => {
                    if object_string(&member.value, "type")? != "object" {
                        return Err(CodecErrorCode::UnsupportedRequestSurface);
                    }
                    transform_request_schema(&mut member.value, ctx, ledger)?;
                    saw_schema = true;
                }
                _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
            }
        }
        if !saw_name || !saw_schema {
            return Err(CodecErrorCode::UnsupportedRequestSurface);
        }
    }
    Ok(())
}

fn transform_request_schema(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    match &mut node.kind {
        JsonKind::Object(members) => {
            ledger.mark(node.occurrence)?;
            let mut property_key_map = Vec::<(String, String)>::new();
            let mut transformed_property_keys = HashSet::new();
            if let Some(properties_member) = members
                .iter()
                .find(|member| member.key.value == "properties")
            {
                let JsonKind::Object(properties) = &properties_member.value.kind else {
                    return Err(CodecErrorCode::UnsupportedRequestSurface);
                };
                for property in properties {
                    guard_mutable_text(&property.key.value)?;
                    ctx.validate_token_shapes(&property.key.value)?;
                    let transformed = ctx.protect(&property.key.value)?;
                    if !transformed_property_keys.insert(transformed.clone()) {
                        return Err(CodecErrorCode::TransformedKeyCollision);
                    }
                    property_key_map.push((property.key.value.clone(), transformed));
                }
            }
            for member in members {
                ledger.mark(member.key_occurrence)?;
                match member.key.value.as_str() {
                    "description" | "title" | "examples" | "default" => {
                        transform_request_mutable(&mut member.value, ctx, ledger)?
                    }
                    "properties" => {
                        ledger.mark(member.value.occurrence)?;
                        let JsonKind::Object(properties) = &mut member.value.kind else {
                            return Err(CodecErrorCode::UnsupportedRequestSurface);
                        };
                        for property in properties {
                            ledger.mark(property.key_occurrence)?;
                            property.key.value = property_key_map
                                .iter()
                                .find(|(original, _)| original == &property.key.value)
                                .map(|(_, transformed)| transformed.clone())
                                .ok_or(CodecErrorCode::InternalCoverageFailure)?;
                            transform_request_schema(&mut property.value, ctx, ledger)?;
                        }
                    }
                    "$defs" => {
                        ledger.mark(member.value.occurrence)?;
                        let JsonKind::Object(definitions) = &mut member.value.kind else {
                            return Err(CodecErrorCode::UnsupportedRequestSurface);
                        };
                        for definition in definitions {
                            ledger.mark(definition.key_occurrence)?;
                            guard_mutable_text(&definition.key.value)?;
                            ctx.validate_token_shapes(&definition.key.value)?;
                            if ctx.probe_without_prefix_cache_write(&definition.key.value)?
                                != definition.key.value
                            {
                                return Err(CodecErrorCode::ControlWouldMutate);
                            }
                            transform_request_schema(&mut definition.value, ctx, ledger)?;
                        }
                    }
                    "items" => transform_request_schema(&mut member.value, ctx, ledger)?,
                    "additionalProperties" => {
                        transform_additional_properties(&mut member.value, ctx, ledger)?
                    }
                    "anyOf" | "allOf" | "oneOf" => {
                        transform_schema_list(&mut member.value, ctx, ledger)?
                    }
                    "required" => transform_required_property_names(
                        &mut member.value,
                        &property_key_map,
                        ledger,
                    )?,
                    "type" => transform_schema_type(&mut member.value, ctx, ledger)?,
                    "$ref" | "$id" | "$schema" | "format" | "pattern" => {
                        request_string_control(&mut member.value, ctx, ledger)?
                    }
                    "const" => transform_schema_literal(&mut member.value, ctx, ledger)?,
                    "enum" => transform_schema_enum(&mut member.value, ctx, ledger)?,
                    "minimum" | "maximum" => request_number_control(&member.value, ledger)?,
                    _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
                }
            }
            Ok(())
        }
        _ => Err(CodecErrorCode::UnsupportedRequestSurface),
    }
}

fn transform_required_property_names(
    node: &mut JsonNode,
    property_key_map: &[(String, String)],
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Array(values) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    for value in values {
        ledger.mark(value.occurrence)?;
        let JsonKind::String(required) = &mut value.kind else {
            return Err(CodecErrorCode::UnsupportedRequestSurface);
        };
        required.value = property_key_map
            .iter()
            .find(|(original, _)| original == &required.value)
            .map(|(_, transformed)| transformed.clone())
            .ok_or(CodecErrorCode::UnsupportedRequestSurface)?;
    }
    Ok(())
}

fn transform_request_mutable(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    match &mut node.kind {
        JsonKind::Object(members) => {
            ledger.mark(node.occurrence)?;
            let mut transformed_keys = HashSet::new();
            for member in members {
                ledger.mark(member.key_occurrence)?;
                protect_key(&mut member.key, ctx)?;
                if !transformed_keys.insert(member.key.value.clone()) {
                    return Err(CodecErrorCode::TransformedKeyCollision);
                }
                transform_request_mutable(&mut member.value, ctx, ledger)?;
            }
            Ok(())
        }
        JsonKind::Array(values) => {
            ledger.mark(node.occurrence)?;
            for value in values {
                transform_request_mutable(value, ctx, ledger)?;
            }
            Ok(())
        }
        JsonKind::String(_) => transform_request_string(node, ctx, ledger),
        JsonKind::Bool(_) | JsonKind::Null => ledger.mark(node.occurrence),
        JsonKind::Number(_) => {
            ledger.mark(node.occurrence)?;
            Err(CodecErrorCode::UnsupportedRequestSurface)
        }
    }
}

fn transform_request_string(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::String(value) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    guard_mutable_text(&value.value)?;
    ctx.validate_token_shapes(&value.value)?;
    value.value = ctx.protect(&value.value)?;
    Ok(())
}

fn protect_key(
    key: &mut JsonString,
    ctx: &mut RequestTransformContext<'_>,
) -> Result<(), CodecErrorCode> {
    guard_mutable_text(&key.value)?;
    ctx.validate_token_shapes(&key.value)?;
    key.value = ctx.protect(&key.value)?;
    Ok(())
}

fn request_string_control(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::String(value) = &node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    guard_mutable_text(&value.value)?;
    ctx.validate_token_shapes(&value.value)?;
    if ctx.probe_without_prefix_cache_write(&value.value)? == value.value {
        Ok(())
    } else {
        Err(CodecErrorCode::ControlWouldMutate)
    }
}

fn request_nonempty_string_control(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    if string_value(node)?.is_empty() {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    request_string_control(node, ctx, ledger)
}

fn request_enum_string_control(
    node: &mut JsonNode,
    allowed: &[&str],
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    if !allowed.contains(&string_value(node)?) {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    request_string_control(node, ctx, ledger)
}

fn request_string_array_control(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Array(values) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    for value in values {
        request_string_control(value, ctx, ledger)?;
    }
    Ok(())
}

fn transform_cache_control(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    let mut saw_type = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "type" => {
                request_enum_string_control(&mut member.value, &["ephemeral"], ctx, ledger)?;
                saw_type = true;
            }
            "ttl" => request_enum_string_control(&mut member.value, &["5m", "1h"], ctx, ledger)?,
            _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
        }
    }
    if !saw_type {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    Ok(())
}

fn validate_cache_control_probe(
    node: &JsonNode,
    ctx: &mut RequestTransformContext<'_>,
) -> Result<(), CodecErrorCode> {
    let JsonKind::Object(members) = &node.kind else {
        return Err(CodecErrorCode::SignedSurfaceMalformed);
    };
    let mut saw_type = false;
    for member in members {
        let value = string_value(&member.value)?;
        let valid = match member.key.value.as_str() {
            "type" => {
                saw_type = true;
                value == "ephemeral"
            }
            "ttl" => matches!(value, "5m" | "1h"),
            _ => false,
        };
        if !valid {
            return Err(CodecErrorCode::SignedSurfaceMalformed);
        }
        guard_mutable_text(value)?;
        ctx.validate_token_shapes(value)?;
        if ctx.probe_without_prefix_cache_write(value)? != value {
            return Err(CodecErrorCode::SignedMutationRequired);
        }
    }
    if saw_type {
        Ok(())
    } else {
        Err(CodecErrorCode::SignedSurfaceMalformed)
    }
}

fn transform_thinking_control(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    let thinking_type = object_string(node, "type")?.to_owned();
    if !matches!(thinking_type.as_str(), "enabled" | "disabled") {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    let mut saw_type = false;
    let mut saw_budget = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "type" => {
                request_enum_string_control(
                    &mut member.value,
                    &[thinking_type.as_str()],
                    ctx,
                    ledger,
                )?;
                saw_type = true;
            }
            "budget_tokens" if thinking_type == "enabled" => {
                request_integer_control(&member.value, ledger, true)?;
                saw_budget = true;
            }
            _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
        }
    }
    if saw_type && (thinking_type == "disabled" || saw_budget) {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn transform_tool_choice_control(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    let choice_type = object_string(node, "type")?.to_owned();
    if !matches!(choice_type.as_str(), "auto" | "any" | "tool" | "none") {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    let mut saw_type = false;
    let mut saw_name = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "type" => {
                request_enum_string_control(
                    &mut member.value,
                    &[choice_type.as_str()],
                    ctx,
                    ledger,
                )?;
                saw_type = true;
            }
            "name" if choice_type == "tool" => {
                request_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_name = true;
            }
            "disable_parallel_tool_use" => request_bool_control(&member.value, ledger)?,
            _ => return Err(CodecErrorCode::UnsupportedRequestSurface),
        }
    }
    if saw_type && (choice_type != "tool" || saw_name) {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn transform_schema_type(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    const TYPES: &[&str] = &[
        "object", "array", "string", "number", "integer", "boolean", "null",
    ];
    match &mut node.kind {
        JsonKind::String(_) => request_enum_string_control(node, TYPES, ctx, ledger),
        JsonKind::Array(values) => {
            ledger.mark(node.occurrence)?;
            if values.is_empty() {
                return Err(CodecErrorCode::UnsupportedRequestSurface);
            }
            for value in values {
                request_enum_string_control(value, TYPES, ctx, ledger)?;
            }
            Ok(())
        }
        _ => Err(CodecErrorCode::UnsupportedRequestSurface),
    }
}

fn transform_additional_properties(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    if matches!(&node.kind, JsonKind::Bool(_)) {
        request_bool_control(node, ledger)
    } else if matches!(&node.kind, JsonKind::Object(_)) {
        transform_request_schema(node, ctx, ledger)
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn transform_schema_list(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Array(values) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    if values.is_empty() {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    for value in values {
        transform_request_schema(value, ctx, ledger)?;
    }
    Ok(())
}

fn transform_schema_literal(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    match &node.kind {
        JsonKind::String(_) => request_string_control(node, ctx, ledger),
        JsonKind::Number(_) => request_number_control(node, ledger),
        JsonKind::Bool(_) | JsonKind::Null => ledger.mark(node.occurrence),
        JsonKind::Object(_) | JsonKind::Array(_) => Err(CodecErrorCode::UnsupportedRequestSurface),
    }
}

fn transform_schema_enum(
    node: &mut JsonNode,
    ctx: &mut RequestTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Array(values) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    if values.is_empty() {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    for value in values {
        transform_schema_literal(value, ctx, ledger)?;
    }
    Ok(())
}

fn request_number_control(
    node: &JsonNode,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    if matches!(node.kind, JsonKind::Number(_)) {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn request_integer_control(
    node: &JsonNode,
    ledger: &mut CoverageLedger,
    nonzero: bool,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Number(raw) = &node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    if raw.starts_with('-') || raw.contains(['.', 'e', 'E']) {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    let value = raw
        .parse::<u64>()
        .map_err(|_| CodecErrorCode::UnsupportedRequestSurface)?;
    if nonzero && value == 0 {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    Ok(())
}

fn request_unit_interval_control(
    node: &JsonNode,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Number(raw) = &node.kind else {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    };
    if raw.starts_with('-') {
        return Err(CodecErrorCode::UnsupportedRequestSurface);
    }
    let value = raw
        .parse::<f64>()
        .map_err(|_| CodecErrorCode::UnsupportedRequestSurface)?;
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn request_bool_control(
    node: &JsonNode,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    if matches!(node.kind, JsonKind::Bool(_)) {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedRequestSurface)
    }
}

fn guard_mutable_text(value: &str) -> Result<(), CodecErrorCode> {
    let mut inspected = value;
    let mut wrapper_depth = 0usize;
    while let Some(inner) = inspected
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
    {
        wrapper_depth += 1;
        if wrapper_depth > MAX_ANGLE_WRAPPER_DEPTH {
            return Err(CodecErrorCode::OpaqueMediaUninspected);
        }
        inspected = inner;
    }
    let lower = inspected.to_ascii_lowercase();
    if inspected.chars().any(|character| {
        (character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
            || matches!(
                character,
                '\u{061c}'
                    | '\u{200e}'
                    | '\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
    }) || contains_carrier_scheme(&lower)
        || inspected.as_bytes().windows(3).any(|window| {
            window[0] == b'%' && hex_value(window[1]).is_some() && hex_value(window[2]).is_some()
        })
        || (inspected.len() >= 64
            && (inspected.bytes().all(|byte| byte.is_ascii_hexdigit())
                || inspected.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'-' | b'_')
                })))
    {
        return Err(CodecErrorCode::OpaqueMediaUninspected);
    }
    Ok(())
}

fn contains_carrier_scheme(value: &str) -> bool {
    value.char_indices().any(|(index, _)| {
        let candidate = &value[index..];
        let starts_scheme = ["data:", "file:", "http://", "https://"]
            .iter()
            .any(|scheme| candidate.starts_with(scheme));
        starts_scheme
            && (index == 0
                || !value[..index].chars().next_back().is_some_and(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                }))
    })
}

fn transform_response_root(
    node: &mut JsonNode,
    input: &[u8],
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    let mut saw_id = false;
    let mut saw_type = false;
    let mut saw_role = false;
    let mut saw_model = false;
    let mut saw_content = false;
    let mut saw_stop_reason = false;
    let mut saw_stop_sequence = false;
    let mut saw_usage = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "id" => {
                response_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_id = true;
            }
            "type" => {
                response_enum_string_control(&mut member.value, &["message"], ctx, ledger)?;
                saw_type = true;
            }
            "role" => {
                response_enum_string_control(&mut member.value, &["assistant"], ctx, ledger)?;
                saw_role = true;
            }
            "model" => {
                response_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_model = true;
            }
            "stop_reason" => {
                response_enum_string_or_null(
                    &mut member.value,
                    &[
                        "end_turn",
                        "max_tokens",
                        "stop_sequence",
                        "tool_use",
                        "pause_turn",
                        "refusal",
                        "model_context_window_exceeded",
                    ],
                    ctx,
                    ledger,
                )?;
                saw_stop_reason = true;
            }
            "stop_sequence" => {
                response_string_or_null(&mut member.value, ctx, ledger)?;
                saw_stop_sequence = true;
            }
            "content" => {
                transform_response_content(&mut member.value, input, ctx, ledger)?;
                saw_content = true;
            }
            "usage" => {
                response_usage(&mut member.value, ledger, true)?;
                saw_usage = true;
            }
            _ => return Err(CodecErrorCode::UnsupportedResponseSurface),
        }
    }
    if saw_id
        && saw_type
        && saw_role
        && saw_model
        && saw_content
        && saw_stop_reason
        && saw_stop_sequence
        && saw_usage
    {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedResponseSurface)
    }
}

fn transform_response_content(
    node: &mut JsonNode,
    input: &[u8],
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Array(blocks) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    for block in blocks {
        transform_response_block(block, input, ctx, ledger)?;
    }
    Ok(())
}

fn transform_response_block(
    node: &mut JsonNode,
    input: &[u8],
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    let block_type = object_string(node, "type")?.to_owned();
    if !matches!(
        block_type.as_str(),
        "text"
            | "tool_use"
            | "tool_result"
            | "search_result"
            | "document"
            | "thinking"
            | "redacted_thinking"
    ) {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    }
    if matches!(block_type.as_str(), "thinking" | "redacted_thinking") {
        validate_signed_response_block(node, input, ctx, ledger, &block_type)?;
        node.raw_preserve = true;
        return Ok(());
    }
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    let mut saw_type = false;
    let mut saw_text = false;
    let mut saw_id = false;
    let mut saw_name = false;
    let mut saw_input = false;
    let mut saw_tool_use_id = false;
    let mut saw_content = false;
    let mut saw_user_payload = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match (block_type.as_str(), member.key.value.as_str()) {
            (_, "type") => {
                response_enum_string_control(
                    &mut member.value,
                    &[block_type.as_str()],
                    ctx,
                    ledger,
                )?;
                saw_type = true;
            }
            ("tool_use", "id") => {
                response_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_id = true;
            }
            ("tool_use", "name") => {
                response_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_name = true;
            }
            ("text", "text") => {
                transform_response_string(&mut member.value, ctx, ledger)?;
                saw_text = true;
            }
            ("text", "citations") => transform_response_citations(&mut member.value, ctx, ledger)?,
            ("tool_use", "input") => {
                transform_response_mutable(&mut member.value, ctx, ledger)?;
                saw_input = true;
            }
            ("tool_result", "content") => {
                transform_response_mutable(&mut member.value, ctx, ledger)?;
                saw_content = true;
            }
            ("tool_result", "tool_use_id") => {
                response_nonempty_string_control(&mut member.value, ctx, ledger)?;
                saw_tool_use_id = true;
            }
            ("tool_result", "is_error") => response_bool_or_null(&member.value, ledger)?,
            ("search_result" | "document", "title" | "context" | "text" | "content") => {
                transform_response_mutable(&mut member.value, ctx, ledger)?;
                saw_user_payload = true;
            }
            ("search_result" | "document", "source" | "id") => {
                response_string_control(&mut member.value, ctx, ledger)?
            }
            _ => return Err(CodecErrorCode::UnsupportedResponseSurface),
        }
    }
    let valid = match block_type.as_str() {
        "text" => saw_text,
        "tool_use" => saw_id && saw_name && saw_input,
        "tool_result" => saw_tool_use_id && saw_content,
        "search_result" | "document" => saw_user_payload,
        _ => false,
    };
    if saw_type && valid {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedResponseSurface)
    }
}

fn validate_signed_response_block(
    node: &JsonNode,
    input: &[u8],
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
    block_type: &str,
) -> Result<(), CodecErrorCode> {
    account_subtree(node, ledger)?;
    let JsonKind::Object(members) = &node.kind else {
        return Err(CodecErrorCode::SignedSurfaceMalformed);
    };
    let mut has_payload = false;
    let mut has_type = false;
    let mut has_signature = false;
    for member in members {
        match member.key.value.as_str() {
            "type" if string_value(&member.value)? == block_type => {
                has_type = true;
            }
            "signature" => {
                let _ = ctx.restore(string_value(&member.value)?)?;
                has_signature = true;
            }
            "thinking" if block_type == "thinking" => {
                let text = string_value(&member.value)?;
                guard_mutable_text_response(text)?;
                let _ = ctx.restore(text)?;
                ctx.validate(text, &[])?;
                has_payload = true;
            }
            "data" if block_type == "redacted_thinking" => {
                let data = string_value(&member.value)?;
                let _ = ctx.restore(data)?;
                has_payload = true;
            }
            _ => return Err(CodecErrorCode::SignedSurfaceMalformed),
        }
    }
    if !has_type
        || !has_payload
        || (block_type == "thinking" && !has_signature)
        || node.span.end > input.len()
    {
        return Err(CodecErrorCode::SignedSurfaceMalformed);
    }
    Ok(())
}

fn transform_response_mutable(
    node: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    match &mut node.kind {
        JsonKind::Object(members) => {
            ledger.mark(node.occurrence)?;
            let mut transformed_keys = HashSet::new();
            for member in members {
                ledger.mark(member.key_occurrence)?;
                restore_json_string(&mut member.key, ctx)?;
                if !transformed_keys.insert(member.key.value.clone()) {
                    return Err(CodecErrorCode::TransformedKeyCollision);
                }
                transform_response_mutable(&mut member.value, ctx, ledger)?;
            }
            Ok(())
        }
        JsonKind::Array(values) => {
            ledger.mark(node.occurrence)?;
            for value in values.iter_mut() {
                transform_response_mutable(value, ctx, ledger)?;
            }
            let mut logical = String::new();
            let mut authorized = Vec::<Range<usize>>::new();
            for value in values {
                if let JsonKind::String(value) = &value.kind {
                    let base = logical.len();
                    logical.push_str(&value.value);
                    authorized.extend(
                        value
                            .authorized
                            .iter()
                            .map(|range| base + range.start..base + range.end),
                    );
                }
            }
            guard_mutable_text_response(&logical)?;
            ctx.validate(&logical, &authorized)?;
            Ok(())
        }
        JsonKind::String(_) => transform_response_string(node, ctx, ledger),
        JsonKind::Bool(_) | JsonKind::Null => ledger.mark(node.occurrence),
        JsonKind::Number(_) => {
            ledger.mark(node.occurrence)?;
            Err(CodecErrorCode::ProviderOriginNumeric)
        }
    }
}

fn transform_response_string(
    node: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::String(value) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    restore_json_string(value, ctx)
}

fn restore_json_string(
    value: &mut JsonString,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<(), CodecErrorCode> {
    guard_mutable_text_response(&value.value)?;
    let restored = ctx.restore(&value.value)?;
    ctx.validate(&restored.text, &restored.authorized_output_ranges)?;
    value.value = restored.text;
    value.authorized = restored.authorized_output_ranges;
    Ok(())
}

fn guard_mutable_text_response(value: &str) -> Result<(), CodecErrorCode> {
    guard_mutable_text(value).map_err(|_| CodecErrorCode::ProviderOriginPii)
}

fn transform_response_citations(
    node: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Array(citations) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    for citation in citations {
        transform_response_citation(citation, ctx, ledger)?;
    }
    Ok(())
}

fn transform_response_citation(
    citation: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(citation.occurrence)?;
    let JsonKind::Object(members) = &mut citation.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "cited_text" | "document_title" | "title" => {
                transform_response_string(&mut member.value, ctx, ledger)?
            }
            "type" | "url" | "source" => response_string_control(&mut member.value, ctx, ledger)?,
            "document_index"
            | "start_char_index"
            | "end_char_index"
            | "start_page_number"
            | "end_page_number"
            | "start_block_index"
            | "end_block_index"
            | "search_result_index" => response_numeric_control(&member.value, ledger)?,
            _ => return Err(CodecErrorCode::UnsupportedResponseSurface),
        }
    }
    Ok(())
}

fn response_string_control(
    node: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::String(value) = &node.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    guard_mutable_text_response(&value.value)?;
    ctx.validate(&value.value, &[])
}

fn response_nonempty_string_control(
    node: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    if string_value(node)?.is_empty() {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    }
    response_string_control(node, ctx, ledger)
}

fn response_enum_string_control(
    node: &mut JsonNode,
    allowed: &[&str],
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    if !allowed.contains(&string_value(node)?) {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    }
    response_string_control(node, ctx, ledger)
}

fn response_string_or_null(
    node: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    match &node.kind {
        JsonKind::String(_) => response_string_control(node, ctx, ledger),
        JsonKind::Null => ledger.mark(node.occurrence),
        _ => Err(CodecErrorCode::UnsupportedResponseSurface),
    }
}

fn response_enum_string_or_null(
    node: &mut JsonNode,
    allowed: &[&str],
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    match &node.kind {
        JsonKind::String(_) => response_enum_string_control(node, allowed, ctx, ledger),
        JsonKind::Null => ledger.mark(node.occurrence),
        _ => Err(CodecErrorCode::UnsupportedResponseSurface),
    }
}

fn response_numeric_control(
    node: &JsonNode,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Number(raw) = &node.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    if raw.starts_with('-') || raw.contains(['.', 'e', 'E']) {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    }
    raw.parse::<u64>()
        .map(|_| ())
        .map_err(|_| CodecErrorCode::UnsupportedResponseSurface)
}

fn response_usage(
    node: &mut JsonNode,
    ledger: &mut CoverageLedger,
    require_input_tokens: bool,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    let JsonKind::Object(members) = &mut node.kind else {
        return Err(CodecErrorCode::UnsupportedResponseSurface);
    };
    let mut saw_input_tokens = false;
    let mut saw_output_tokens = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "input_tokens" => {
                response_numeric_control(&member.value, ledger)?;
                saw_input_tokens = true;
            }
            "output_tokens" => {
                response_numeric_control(&member.value, ledger)?;
                saw_output_tokens = true;
            }
            "cache_creation_input_tokens" | "cache_read_input_tokens" => {
                response_numeric_control(&member.value, ledger)?
            }
            _ => return Err(CodecErrorCode::UnsupportedResponseSurface),
        }
    }
    if (!require_input_tokens || saw_input_tokens) && saw_output_tokens {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedResponseSurface)
    }
}

fn response_bool_or_null(
    node: &JsonNode,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(node.occurrence)?;
    if matches!(&node.kind, JsonKind::Bool(_) | JsonKind::Null) {
        Ok(())
    } else {
        Err(CodecErrorCode::UnsupportedResponseSurface)
    }
}

fn object_string<'a>(node: &'a JsonNode, key: &str) -> Result<&'a str, CodecErrorCode> {
    let JsonKind::Object(members) = &node.kind else {
        return Err(CodecErrorCode::InvalidJson);
    };
    let member = members
        .iter()
        .find(|member| member.key.value == key)
        .ok_or(CodecErrorCode::InvalidJson)?;
    string_value(&member.value)
}

fn string_value(node: &JsonNode) -> Result<&str, CodecErrorCode> {
    let JsonKind::String(value) = &node.kind else {
        return Err(CodecErrorCode::InvalidJson);
    };
    Ok(&value.value)
}

fn write_document(
    document: &JsonDocument,
    input: &[u8],
) -> Result<(Vec<u8>, OutputProvenance), CodecErrorCode> {
    let mut output = Vec::new();
    let mut provenance = OutputProvenance::default();
    write_json_node(&document.root, input, &mut output, &mut provenance)?;
    provenance.set_occurrence_count(document.occurrence_count);
    Ok((output, provenance))
}

fn write_json_node(
    node: &JsonNode,
    input: &[u8],
    output: &mut Vec<u8>,
    provenance: &mut OutputProvenance,
) -> Result<(), CodecErrorCode> {
    if node.raw_preserve {
        let raw = input
            .get(node.span.clone())
            .ok_or(CodecErrorCode::InternalCoverageFailure)?;
        let start = output.len();
        output.extend_from_slice(raw);
        provenance.record_signed(start..output.len());
        return Ok(());
    }
    match &node.kind {
        JsonKind::Object(members) => {
            output.push(b'{');
            for (index, member) in members.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_json_string(&member.key, output, provenance);
                output.push(b':');
                write_json_node(&member.value, input, output, provenance)?;
            }
            output.push(b'}');
        }
        JsonKind::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_json_node(value, input, output, provenance)?;
            }
            output.push(b']');
        }
        JsonKind::String(value) => write_json_string(value, output, provenance),
        JsonKind::Number(raw) => output.extend_from_slice(raw.as_bytes()),
        JsonKind::Bool(true) => output.extend_from_slice(b"true"),
        JsonKind::Bool(false) => output.extend_from_slice(b"false"),
        JsonKind::Null => output.extend_from_slice(b"null"),
    }
    Ok(())
}

fn write_json_string(value: &JsonString, output: &mut Vec<u8>, provenance: &mut OutputProvenance) {
    output.push(b'"');
    for (logical_start, character) in value.value.char_indices() {
        let logical_end = logical_start + character.len_utf8();
        let authorized = value
            .authorized
            .iter()
            .any(|range| range.start <= logical_start && range.end >= logical_end);
        let wire_start = output.len();
        match character {
            '"' => output.extend_from_slice(br#"\""#),
            '\\' => output.extend_from_slice(br#"\\"#),
            '\u{0008}' => output.extend_from_slice(br"\b"),
            '\u{000c}' => output.extend_from_slice(br"\f"),
            '\n' => output.extend_from_slice(br"\n"),
            '\r' => output.extend_from_slice(br"\r"),
            '\t' => output.extend_from_slice(br"\t"),
            character if character <= '\u{001f}' => {
                output.extend_from_slice(format!("\\u{:04x}", u32::from(character)).as_bytes());
            }
            character => {
                let mut bytes = [0u8; 4];
                output.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
            }
        }
        if authorized {
            provenance.record_authorized(wire_start..output.len());
        }
    }
    output.push(b'"');
}

fn json_semantically_equal(left: &JsonNode, right: &JsonNode) -> bool {
    match (&left.kind, &right.kind) {
        (JsonKind::Object(left_members), JsonKind::Object(right_members)) => {
            left_members.len() == right_members.len()
                && left_members.iter().zip(right_members).all(|(left, right)| {
                    left.key.value == right.key.value
                        && json_semantically_equal(&left.value, &right.value)
                })
        }
        (JsonKind::Array(left_values), JsonKind::Array(right_values)) => {
            left_values.len() == right_values.len()
                && left_values
                    .iter()
                    .zip(right_values)
                    .all(|(left, right)| json_semantically_equal(left, right))
        }
        (JsonKind::String(left), JsonKind::String(right)) => left.value == right.value,
        (JsonKind::Number(left), JsonKind::Number(right)) => left == right,
        (JsonKind::Bool(left), JsonKind::Bool(right)) => left == right,
        (JsonKind::Null, JsonKind::Null) => true,
        _ => false,
    }
}

fn collect_signed_slices(node: &JsonNode, input: &[u8]) -> Result<Vec<Vec<u8>>, CodecErrorCode> {
    let mut slices = Vec::new();
    collect_signed_slices_into(node, input, &mut slices)?;
    Ok(slices)
}

fn collect_signed_slices_into(
    node: &JsonNode,
    input: &[u8],
    slices: &mut Vec<Vec<u8>>,
) -> Result<(), CodecErrorCode> {
    if node.raw_preserve {
        slices.push(
            input
                .get(node.span.clone())
                .ok_or(CodecErrorCode::InternalCoverageFailure)?
                .to_vec(),
        );
        return Ok(());
    }
    match &node.kind {
        JsonKind::Object(members) => {
            for member in members {
                collect_signed_slices_into(&member.value, input, slices)?;
            }
        }
        JsonKind::Array(values) => {
            for value in values {
                collect_signed_slices_into(value, input, slices)?;
            }
        }
        JsonKind::String(_) | JsonKind::Number(_) | JsonKind::Bool(_) | JsonKind::Null => {}
    }
    Ok(())
}

fn collect_authorized_wire_chunks(node: &JsonNode) -> Result<Vec<Vec<u8>>, CodecErrorCode> {
    let mut chunks = Vec::new();
    collect_authorized_wire_chunks_into(node, &mut chunks)?;
    Ok(chunks)
}

fn collect_authorized_wire_chunks_into(
    node: &JsonNode,
    chunks: &mut Vec<Vec<u8>>,
) -> Result<(), CodecErrorCode> {
    if node.raw_preserve {
        return Ok(());
    }
    match &node.kind {
        JsonKind::Object(members) => {
            for member in members {
                collect_authorized_string_chunks(&member.key, chunks)?;
                collect_authorized_wire_chunks_into(&member.value, chunks)?;
            }
        }
        JsonKind::Array(values) => {
            for value in values {
                collect_authorized_wire_chunks_into(value, chunks)?;
            }
        }
        JsonKind::String(value) => collect_authorized_string_chunks(value, chunks)?,
        JsonKind::Number(_) | JsonKind::Bool(_) | JsonKind::Null => {}
    }
    Ok(())
}

fn collect_authorized_string_chunks(
    value: &JsonString,
    chunks: &mut Vec<Vec<u8>>,
) -> Result<(), CodecErrorCode> {
    for range in &value.authorized {
        let logical = value
            .value
            .get(range.clone())
            .ok_or(CodecErrorCode::InternalCoverageFailure)?;
        let mut encoded = Vec::new();
        write_json_string(
            &JsonString {
                value: logical.to_owned(),
                authorized: Vec::new(),
            },
            &mut encoded,
            &mut OutputProvenance::default(),
        );
        let content = encoded
            .get(1..encoded.len().saturating_sub(1))
            .ok_or(CodecErrorCode::InternalCoverageFailure)?;
        chunks.push(content.to_vec());
    }
    Ok(())
}

fn verify_final_wire_proof(
    output: &[u8],
    provenance: &OutputProvenance,
    expected_authorized: &[Vec<u8>],
    expected_signed: &[Vec<u8>],
) -> Result<(), CodecErrorCode> {
    verify_range_chunks(
        output,
        provenance.authorized_output_ranges(),
        expected_authorized,
    )?;
    verify_range_chunks(output, provenance.signed_opaque_ranges(), expected_signed)?;
    for authorized in provenance.authorized_output_ranges() {
        if provenance
            .signed_opaque_ranges()
            .iter()
            .any(|signed| authorized.start < signed.end && signed.start < authorized.end)
        {
            return Err(CodecErrorCode::InternalCoverageFailure);
        }
    }
    Ok(())
}

fn verify_range_chunks(
    output: &[u8],
    ranges: &[Range<usize>],
    expected: &[Vec<u8>],
) -> Result<(), CodecErrorCode> {
    if ranges.len() != expected.len() {
        return Err(CodecErrorCode::InternalCoverageFailure);
    }
    let mut previous_end = 0usize;
    for (range, expected) in ranges.iter().zip(expected) {
        if range.start < previous_end
            || range.start >= range.end
            || range.end > output.len()
            || output.get(range.clone()) != Some(expected.as_slice())
        {
            return Err(CodecErrorCode::InternalCoverageFailure);
        }
        previous_end = range.end;
    }
    Ok(())
}

struct SseDecoder {
    limits: CodecLimits,
    bytes: Vec<u8>,
}

struct SseFrame {
    event: Option<String>,
    data: Vec<u8>,
    raw: Vec<u8>,
    comment: bool,
    inspection_fact: Option<SseInspectionFrameFactV1>,
}

struct SseInspectionFrameFactV1 {
    event_kind: SseEventKindV1,
    delta_kind: ProjectionAvailabilityV1<SseDeltaKindV1>,
    content_block: ProjectionAvailabilityV1<ContentBlockIndexV1>,
}

impl SseDecoder {
    fn new(limits: CodecLimits) -> Self {
        Self {
            limits,
            bytes: Vec::new(),
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Result<(), CodecError> {
        let next =
            self.bytes.len().checked_add(chunk.len()).ok_or_else(|| {
                codec_error(CodecErrorCode::BodyLimitExceeded, CodecPhase::SseDecode)
            })?;
        if next > self.limits.max_body_bytes() {
            return Err(codec_error(
                CodecErrorCode::BodyLimitExceeded,
                CodecPhase::SseDecode,
            ));
        }
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    fn input_len(&self) -> usize {
        self.bytes.len()
    }

    fn finish(self) -> Result<Vec<SseFrame>, CodecError> {
        if self.bytes.starts_with(&[0xef, 0xbb, 0xbf]) || self.bytes.contains(&0) {
            return Err(codec_error(
                CodecErrorCode::InvalidSseFrame,
                CodecPhase::SseDecode,
            ));
        }
        std::str::from_utf8(&self.bytes)
            .map_err(|_| codec_error(CodecErrorCode::InvalidUtf8, CodecPhase::SseDecode))?;
        let lines = split_sse_lines(&self.bytes, self.limits)?;
        let mut frames = Vec::new();
        let mut event = None;
        let mut data_lines = Vec::<Vec<u8>>::new();
        let mut raw = Vec::new();
        let mut comment_only = false;
        for line in lines {
            if line.content.is_empty() {
                if comment_only && event.is_none() && data_lines.is_empty() {
                    raw.extend_from_slice(line.raw);
                    if raw.len() > self.limits.max_sse_frame_bytes() {
                        return Err(codec_error(
                            CodecErrorCode::SseFrameLimitExceeded,
                            CodecPhase::SseDecode,
                        ));
                    }
                    frames.push(SseFrame {
                        event: None,
                        data: Vec::new(),
                        raw: std::mem::take(&mut raw),
                        comment: true,
                        inspection_fact: None,
                    });
                } else if event.is_some() || !data_lines.is_empty() {
                    raw.extend_from_slice(line.raw);
                    if raw.len() > self.limits.max_sse_frame_bytes() {
                        return Err(codec_error(
                            CodecErrorCode::SseFrameLimitExceeded,
                            CodecPhase::SseDecode,
                        ));
                    }
                    let data_len = data_lines.iter().map(Vec::len).sum::<usize>()
                        + data_lines.len().saturating_sub(1);
                    if data_len > self.limits.max_sse_frame_bytes() {
                        return Err(codec_error(
                            CodecErrorCode::SseFrameLimitExceeded,
                            CodecPhase::SseDecode,
                        ));
                    }
                    let mut data = Vec::with_capacity(data_len);
                    for (index, value) in data_lines.drain(..).enumerate() {
                        if index > 0 {
                            data.push(b'\n');
                        }
                        data.extend_from_slice(&value);
                    }
                    frames.push(SseFrame {
                        event: event.take(),
                        data,
                        raw: std::mem::take(&mut raw),
                        comment: false,
                        inspection_fact: None,
                    });
                } else if !raw.is_empty() {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseFrame,
                        CodecPhase::SseDecode,
                    ));
                }
                comment_only = false;
                if frames.len() > self.limits.max_sse_events() {
                    return Err(codec_error(
                        CodecErrorCode::SseEventLimitExceeded,
                        CodecPhase::SseDecode,
                    ));
                }
                continue;
            }
            raw.extend_from_slice(line.raw);
            if raw.len() > self.limits.max_sse_frame_bytes() {
                return Err(codec_error(
                    CodecErrorCode::SseFrameLimitExceeded,
                    CodecPhase::SseDecode,
                ));
            }
            if line.content.starts_with(b":") {
                if event.is_some() || !data_lines.is_empty() || comment_only {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseFrame,
                        CodecPhase::SseDecode,
                    ));
                }
                comment_only = true;
                continue;
            }
            if comment_only {
                return Err(codec_error(
                    CodecErrorCode::InvalidSseFrame,
                    CodecPhase::SseDecode,
                ));
            }
            let colon = line
                .content
                .iter()
                .position(|byte| *byte == b':')
                .ok_or_else(|| {
                    codec_error(CodecErrorCode::InvalidSseField, CodecPhase::SseDecode)
                })?;
            let field = &line.content[..colon];
            let mut value = &line.content[colon + 1..];
            if value.first() == Some(&b' ') {
                value = &value[1..];
            }
            match field {
                b"event" => {
                    if event.is_some() || !data_lines.is_empty() {
                        return Err(codec_error(
                            CodecErrorCode::InvalidSseField,
                            CodecPhase::SseDecode,
                        ));
                    }
                    event = Some(
                        std::str::from_utf8(value)
                            .map_err(|_| {
                                codec_error(CodecErrorCode::InvalidUtf8, CodecPhase::SseDecode)
                            })?
                            .to_owned(),
                    );
                }
                b"data" => data_lines.push(value.to_vec()),
                _ => {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseField,
                        CodecPhase::SseDecode,
                    ));
                }
            }
        }
        if event.is_some() || !data_lines.is_empty() || comment_only || !raw.is_empty() {
            return Err(codec_error(
                CodecErrorCode::IncompleteSseStream,
                CodecPhase::SseDecode,
            ));
        }
        Ok(frames)
    }
}

struct SseLine<'a> {
    content: &'a [u8],
    raw: &'a [u8],
}

fn split_sse_lines(bytes: &[u8], limits: CodecLimits) -> Result<Vec<SseLine<'_>>, CodecError> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut position = 0usize;
    while position < bytes.len() {
        if !matches!(bytes[position], b'\r' | b'\n') {
            position += 1;
            if position - start > limits.max_sse_line_bytes() {
                return Err(codec_error(
                    CodecErrorCode::SseLineLimitExceeded,
                    CodecPhase::SseDecode,
                ));
            }
            continue;
        }
        let content_end = position;
        if bytes[position] == b'\r' && bytes.get(position + 1) == Some(&b'\n') {
            position += 2;
        } else {
            position += 1;
        }
        lines.push(SseLine {
            content: &bytes[start..content_end],
            raw: &bytes[start..position],
        });
        start = position;
    }
    if start != bytes.len() {
        return Err(codec_error(
            CodecErrorCode::IncompleteSseStream,
            CodecPhase::SseDecode,
        ));
    }
    Ok(lines)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum MessageLifecycle {
    AwaitingMessageStart,
    InMessage,
    MessageStopped,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BlockKind {
    Text,
    InputJson,
    Thinking,
    RedactedThinking,
}

struct BlockAccumulator {
    kind: BlockKind,
    start_slot: usize,
    start_frame: Vec<u8>,
    delta_slots: Vec<usize>,
    logical: String,
    signature: String,
    signed_frames: Vec<(usize, Vec<u8>)>,
    passthrough_frames: Vec<(usize, Vec<u8>, OutputProvenance)>,
}

#[derive(Default)]
struct ReplaySlot {
    bytes: Option<Vec<u8>>,
    provenance: OutputProvenance,
}

fn restore_sse_frames(
    mut frames: Vec<SseFrame>,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<ProvedResponseBody, CodecError> {
    let mut lifecycle = MessageLifecycle::AwaitingMessageStart;
    let mut blocks = BTreeMap::<u32, BlockAccumulator>::new();
    let mut seen_indices = HashSet::<u32>::new();
    let mut slots = Vec::<ReplaySlot>::new();
    let mut concatenated_text = String::new();
    let mut concatenated_authorized = Vec::<Range<usize>>::new();
    let mut occurrence_count = 0usize;
    let mut inspection_facts_complete = true;
    for frame in &mut frames {
        if frame.comment {
            if matches!(lifecycle, MessageLifecycle::MessageStopped) {
                return Err(codec_error(
                    CodecErrorCode::InvalidSseLifecycle,
                    CodecPhase::SseLifecycle,
                ));
            }
            continue;
        }
        let event = frame
            .event
            .as_deref()
            .ok_or_else(|| codec_error(CodecErrorCode::InvalidSseFrame, CodecPhase::SseDecode))?;
        if matches!(lifecycle, MessageLifecycle::MessageStopped) {
            return Err(codec_error(
                CodecErrorCode::InvalidSseLifecycle,
                CodecPhase::SseLifecycle,
            ));
        }
        let mut document = parse_json(&frame.data, ctx.limits(), CodecPhase::SseDecode)?;
        occurrence_count = occurrence_count
            .checked_add(document.occurrence_count)
            .ok_or_else(|| {
                codec_error(CodecErrorCode::NodeLimitExceeded, CodecPhase::SseLifecycle)
            })?;
        let discriminator = object_string(&document.root, "type")
            .map_err(|code| codec_error(code, CodecPhase::SseDecode))?;
        if discriminator != event {
            return Err(codec_error(
                CodecErrorCode::InvalidSseEventPair,
                CodecPhase::SseDecode,
            ));
        }
        match event {
            "ping" => {
                validate_simple_event(&document, "ping")?;
            }
            "error" => {
                return Err(codec_error(
                    CodecErrorCode::ProviderErrorEvent,
                    CodecPhase::SseLifecycle,
                ));
            }
            "message_start" => {
                if !matches!(lifecycle, MessageLifecycle::AwaitingMessageStart) {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseLifecycle,
                        CodecPhase::SseLifecycle,
                    ));
                }
                transform_message_start(&mut document, &frame.data, ctx)?;
                let (bytes, provenance) = canonical_sse_frame(event, &document, &frame.data)?;
                slots.push(ReplaySlot {
                    bytes: Some(bytes),
                    provenance,
                });
                lifecycle = MessageLifecycle::InMessage;
            }
            "content_block_start" => {
                if !matches!(lifecycle, MessageLifecycle::InMessage) {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseLifecycle,
                        CodecPhase::SseLifecycle,
                    ));
                }
                if seen_indices.len() >= ctx.limits().max_sse_indices() {
                    return Err(codec_error(
                        CodecErrorCode::SseIndexLimitExceeded,
                        CodecPhase::SseLifecycle,
                    ));
                }
                let index = event_index(&document)?;
                if !seen_indices.insert(index) {
                    return Err(codec_error(
                        CodecErrorCode::InvalidContentBlockIndex,
                        CodecPhase::SseLifecycle,
                    ));
                }
                let slot = slots.len();
                slots.push(ReplaySlot::default());
                let accumulator =
                    transform_block_start(&mut document, &frame.data, &frame.raw, slot, ctx)?;
                blocks.insert(index, accumulator);
            }
            "content_block_delta" => {
                if !matches!(lifecycle, MessageLifecycle::InMessage) {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseLifecycle,
                        CodecPhase::SseLifecycle,
                    ));
                }
                let index = event_index(&document)?;
                let accumulator = blocks.get_mut(&index).ok_or_else(|| {
                    codec_error(
                        CodecErrorCode::InvalidContentBlockIndex,
                        CodecPhase::SseLifecycle,
                    )
                })?;
                let slot = slots.len();
                slots.push(ReplaySlot::default());
                transform_block_delta(
                    accumulator,
                    &mut document,
                    &frame.data,
                    &frame.raw,
                    slot,
                    ctx,
                )?;
            }
            "content_block_stop" => {
                if !matches!(lifecycle, MessageLifecycle::InMessage) {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseLifecycle,
                        CodecPhase::SseLifecycle,
                    ));
                }
                let index = event_index(&document)?;
                let accumulator = blocks.remove(&index).ok_or_else(|| {
                    codec_error(
                        CodecErrorCode::InvalidContentBlockIndex,
                        CodecPhase::SseLifecycle,
                    )
                })?;
                let stop_slot = slots.len();
                slots.push(ReplaySlot::default());
                finish_block(
                    index,
                    accumulator,
                    &mut slots,
                    StopFrame {
                        slot: stop_slot,
                        document: &mut document,
                        input: &frame.data,
                    },
                    ctx,
                    &mut concatenated_text,
                    &mut concatenated_authorized,
                )?;
            }
            "message_delta" => {
                if !matches!(lifecycle, MessageLifecycle::InMessage) || !blocks.is_empty() {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseLifecycle,
                        CodecPhase::SseLifecycle,
                    ));
                }
                transform_message_delta(&mut document, ctx)?;
                let (bytes, provenance) = canonical_sse_frame(event, &document, &frame.data)?;
                slots.push(ReplaySlot {
                    bytes: Some(bytes),
                    provenance,
                });
            }
            "message_stop" => {
                if !matches!(lifecycle, MessageLifecycle::InMessage) || !blocks.is_empty() {
                    return Err(codec_error(
                        CodecErrorCode::InvalidSseLifecycle,
                        CodecPhase::SseLifecycle,
                    ));
                }
                validate_simple_event(&document, "message_stop")?;
                let (bytes, provenance) = canonical_sse_frame(event, &document, &frame.data)?;
                slots.push(ReplaySlot {
                    bytes: Some(bytes),
                    provenance,
                });
                lifecycle = MessageLifecycle::MessageStopped;
            }
            _ => {
                validate_future_event(&document, event, ctx)?;
            }
        }
        if ctx.inspection_projection_requested() {
            frame.inspection_fact = sse_inspection_fact(&document, event);
            inspection_facts_complete &= frame.inspection_fact.is_some();
        }
    }
    if !matches!(lifecycle, MessageLifecycle::MessageStopped) || !blocks.is_empty() {
        return Err(codec_error(
            CodecErrorCode::IncompleteSseStream,
            CodecPhase::SseLifecycle,
        ));
    }
    ctx.validate(&concatenated_text, &concatenated_authorized)
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    let mut output = Vec::new();
    let mut provenance = OutputProvenance::default();
    let mut expected_authorized = Vec::<Vec<u8>>::new();
    let mut expected_signed = Vec::<Vec<u8>>::new();
    let mut expected_authorized_ranges = Vec::<Range<usize>>::new();
    let mut expected_signed_ranges = Vec::<Range<usize>>::new();
    for slot in slots {
        if let Some(bytes) = slot.bytes {
            let offset = output.len();
            for range in slot.provenance.authorized_output_ranges() {
                append_expected_wire_chunk(
                    &bytes,
                    range,
                    offset,
                    &mut expected_authorized_ranges,
                    &mut expected_authorized,
                )
                .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
            }
            for range in slot.provenance.signed_opaque_ranges() {
                append_expected_wire_chunk(
                    &bytes,
                    range,
                    offset,
                    &mut expected_signed_ranges,
                    &mut expected_signed,
                )
                .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
            }
            output.extend_from_slice(&bytes);
            for range in slot.provenance.authorized_output_ranges() {
                provenance.record_authorized(offset + range.start..offset + range.end);
            }
            for range in slot.provenance.signed_opaque_ranges() {
                provenance.record_signed(offset + range.start..offset + range.end);
            }
        }
    }
    if output.len() > ctx.limits().max_body_bytes() {
        return Err(codec_error(
            CodecErrorCode::BodyLimitExceeded,
            CodecPhase::SseReplay,
        ));
    }
    let final_occurrence_count = reprove_sse_output(&output, ctx.limits())?;
    provenance.set_occurrence_count(final_occurrence_count);
    verify_final_wire_proof(&output, &provenance, &expected_authorized, &expected_signed)
        .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
    provenance.mark_final_buffer_verified();
    if ctx.inspection_projection_requested() && provenance.signed_opaque_ranges().is_empty() {
        for frame in &mut frames {
            frame.event = None;
            frame.data = Vec::new();
            frame.raw = Vec::new();
        }
        Ok(ProvedResponseBody::new_with_inspection_parsed_facts(
            output,
            WireFormat::Sse,
            provenance,
            InspectionParsedFactsV1::AnthropicSse(SseInspectionParsedFactsV1 {
                frames,
                complete: inspection_facts_complete,
            }),
        ))
    } else {
        Ok(ProvedResponseBody::new(output, WireFormat::Sse, provenance))
    }
}

fn sse_inspection_fact(document: &JsonDocument, event: &str) -> Option<SseInspectionFrameFactV1> {
    let event_kind = match event {
        "message_start" => SseEventKindV1::MessageStart,
        "content_block_start" => SseEventKindV1::ContentBlockStart,
        "content_block_delta" => SseEventKindV1::ContentBlockDelta,
        "content_block_stop" => SseEventKindV1::ContentBlockStop,
        "message_delta" => SseEventKindV1::MessageDelta,
        "message_stop" => SseEventKindV1::MessageStop,
        "ping" => SseEventKindV1::Ping,
        "error" => SseEventKindV1::Error,
        _ => SseEventKindV1::UnknownFuture,
    };
    let content_block = match event {
        "content_block_start" | "content_block_delta" | "content_block_stop" => {
            ProjectionAvailabilityV1::Present(ContentBlockIndexV1::new(event_index(document).ok()?))
        }
        _ => ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable),
    };
    let delta_kind = if event == "content_block_delta" {
        let delta = object_value(&document.root, "delta").ok()?;
        let delta_type = object_string(delta, "type").ok()?;
        let kind = match delta_type {
            "text_delta" => SseDeltaKindV1::Text,
            "input_json_delta" => SseDeltaKindV1::InputJson,
            "thinking_delta" => SseDeltaKindV1::Thinking,
            "signature_delta" => SseDeltaKindV1::Signature,
            "citations_delta" => SseDeltaKindV1::Citation,
            _ => SseDeltaKindV1::UnknownFuture,
        };
        ProjectionAvailabilityV1::Present(kind)
    } else {
        ProjectionAvailabilityV1::Omitted(ProjectionOmissionReasonV1::NotApplicable)
    };
    Some(SseInspectionFrameFactV1 {
        event_kind,
        delta_kind,
        content_block,
    })
}

fn append_expected_wire_chunk(
    source: &[u8],
    local: &Range<usize>,
    offset: usize,
    ranges: &mut Vec<Range<usize>>,
    chunks: &mut Vec<Vec<u8>>,
) -> Result<(), CodecErrorCode> {
    let chunk = source
        .get(local.clone())
        .ok_or(CodecErrorCode::InternalCoverageFailure)?;
    let global = offset
        .checked_add(local.start)
        .ok_or(CodecErrorCode::InternalCoverageFailure)?
        ..offset
            .checked_add(local.end)
            .ok_or(CodecErrorCode::InternalCoverageFailure)?;
    if let Some(previous) = ranges.last_mut() {
        if global.start <= previous.end {
            let overlap = previous.end.saturating_sub(global.start);
            if overlap > chunk.len() {
                return Err(CodecErrorCode::InternalCoverageFailure);
            }
            let previous_chunk = chunks
                .last_mut()
                .ok_or(CodecErrorCode::InternalCoverageFailure)?;
            if overlap > previous_chunk.len()
                || previous_chunk[previous_chunk.len() - overlap..] != chunk[..overlap]
            {
                return Err(CodecErrorCode::InternalCoverageFailure);
            }
            previous_chunk.extend_from_slice(&chunk[overlap..]);
            previous.end = previous.end.max(global.end);
            return Ok(());
        }
    }
    ranges.push(global);
    chunks.push(chunk.to_vec());
    Ok(())
}

fn reprove_sse_output(output: &[u8], limits: CodecLimits) -> Result<usize, CodecError> {
    let mut decoder = SseDecoder::new(limits);
    decoder.push(output)?;
    let frames = decoder.finish()?;
    let mut reconstructed = Vec::with_capacity(output.len());
    let mut lifecycle = MessageLifecycle::AwaitingMessageStart;
    let mut blocks = BTreeMap::<u32, BlockKind>::new();
    let mut seen_indices = HashSet::<u32>::new();
    let mut occurrence_count = 0usize;
    for frame in frames {
        if frame.comment {
            return Err(codec_error(
                CodecErrorCode::InternalCoverageFailure,
                CodecPhase::SseReplay,
            ));
        }
        reconstructed.extend_from_slice(&frame.raw);
        let event = frame.event.as_deref().ok_or_else(|| {
            codec_error(
                CodecErrorCode::InternalCoverageFailure,
                CodecPhase::SseReplay,
            )
        })?;
        let document = parse_json(&frame.data, limits, CodecPhase::SseReplay)?;
        occurrence_count = occurrence_count
            .checked_add(document.occurrence_count)
            .ok_or_else(|| codec_error(CodecErrorCode::NodeLimitExceeded, CodecPhase::SseReplay))?;
        validate_full_coverage(&document).map_err(|_| {
            codec_error(
                CodecErrorCode::InternalCoverageFailure,
                CodecPhase::SseReplay,
            )
        })?;
        if object_string(&document.root, "type") != Ok(event) {
            return Err(codec_error(
                CodecErrorCode::InternalCoverageFailure,
                CodecPhase::SseReplay,
            ));
        }
        match event {
            "message_start" => {
                if lifecycle != MessageLifecycle::AwaitingMessageStart {
                    return Err(codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    ));
                }
                exact_object_shape(&document.root, &["type", "message"], &["type", "message"])
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                let message = object_value(&document.root, "message")
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                prove_response_root_shape(message)
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                lifecycle = MessageLifecycle::InMessage;
            }
            "content_block_start" => {
                if lifecycle != MessageLifecycle::InMessage {
                    return Err(codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    ));
                }
                exact_object_shape(
                    &document.root,
                    &["type", "index", "content_block"],
                    &["type", "index", "content_block"],
                )
                .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                let index = event_index(&document)?;
                if seen_indices.len() >= limits.max_sse_indices()
                    || !seen_indices.insert(index)
                    || blocks.contains_key(&index)
                {
                    return Err(codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    ));
                }
                let content_block = object_value(&document.root, "content_block")
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                let kind = prove_sse_block_start_shape(content_block)
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                blocks.insert(index, kind);
            }
            "content_block_delta" => {
                if lifecycle != MessageLifecycle::InMessage {
                    return Err(codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    ));
                }
                exact_object_shape(
                    &document.root,
                    &["type", "index", "delta"],
                    &["type", "index", "delta"],
                )
                .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                let index = event_index(&document)?;
                let kind = blocks.get(&index).copied().ok_or_else(|| {
                    codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    )
                })?;
                let delta = object_value(&document.root, "delta")
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                prove_sse_delta_shape(delta, kind, limits)
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
            }
            "content_block_stop" => {
                if lifecycle != MessageLifecycle::InMessage {
                    return Err(codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    ));
                }
                exact_object_shape(&document.root, &["type", "index"], &["type", "index"])
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                let index = event_index(&document)?;
                if blocks.remove(&index).is_none() {
                    return Err(codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    ));
                }
            }
            "message_delta" => {
                if lifecycle != MessageLifecycle::InMessage || !blocks.is_empty() {
                    return Err(codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    ));
                }
                prove_message_delta_shape(&document.root)
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
            }
            "message_stop" => {
                if lifecycle != MessageLifecycle::InMessage || !blocks.is_empty() {
                    return Err(codec_error(
                        CodecErrorCode::InternalCoverageFailure,
                        CodecPhase::SseReplay,
                    ));
                }
                exact_object_shape(&document.root, &["type"], &["type"])
                    .map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
                lifecycle = MessageLifecycle::MessageStopped;
            }
            _ => {
                return Err(codec_error(
                    CodecErrorCode::InternalCoverageFailure,
                    CodecPhase::SseReplay,
                ));
            }
        }
    }
    if reconstructed != output
        || lifecycle != MessageLifecycle::MessageStopped
        || !blocks.is_empty()
    {
        return Err(codec_error(
            CodecErrorCode::InternalCoverageFailure,
            CodecPhase::SseReplay,
        ));
    }
    Ok(occurrence_count)
}

fn exact_object_shape(
    node: &JsonNode,
    allowed: &[&str],
    required: &[&str],
) -> Result<(), CodecErrorCode> {
    let JsonKind::Object(members) = &node.kind else {
        return Err(CodecErrorCode::InternalCoverageFailure);
    };
    if members
        .iter()
        .any(|member| !allowed.contains(&member.key.value.as_str()))
        || required
            .iter()
            .any(|required| !members.iter().any(|member| member.key.value == *required))
    {
        return Err(CodecErrorCode::InternalCoverageFailure);
    }
    Ok(())
}

fn object_value<'a>(node: &'a JsonNode, key: &str) -> Result<&'a JsonNode, CodecErrorCode> {
    let JsonKind::Object(members) = &node.kind else {
        return Err(CodecErrorCode::InternalCoverageFailure);
    };
    members
        .iter()
        .find(|member| member.key.value == key)
        .map(|member| &member.value)
        .ok_or(CodecErrorCode::InternalCoverageFailure)
}

fn prove_response_root_shape(node: &JsonNode) -> Result<(), CodecErrorCode> {
    const KEYS: &[&str] = &[
        "id",
        "type",
        "role",
        "model",
        "content",
        "stop_reason",
        "stop_sequence",
        "usage",
    ];
    exact_object_shape(node, KEYS, KEYS)?;
    if object_string(node, "type")? != "message"
        || object_string(node, "role")? != "assistant"
        || object_string(node, "id")?.is_empty()
        || object_string(node, "model")?.is_empty()
    {
        return Err(CodecErrorCode::InternalCoverageFailure);
    }
    prove_optional_enum_string(
        object_value(node, "stop_reason")?,
        &[
            "end_turn",
            "max_tokens",
            "stop_sequence",
            "tool_use",
            "pause_turn",
            "refusal",
            "model_context_window_exceeded",
        ],
    )?;
    prove_optional_string(object_value(node, "stop_sequence")?)?;
    let content = object_value(node, "content")?;
    let JsonKind::Array(blocks) = &content.kind else {
        return Err(CodecErrorCode::InternalCoverageFailure);
    };
    for block in blocks {
        prove_response_block_shape(block)?;
    }
    prove_usage_shape(object_value(node, "usage")?, true)
}

fn prove_response_block_shape(node: &JsonNode) -> Result<(), CodecErrorCode> {
    match object_string(node, "type")? {
        "text" => exact_object_shape(node, &["type", "text", "citations"], &["type", "text"]),
        "tool_use" => exact_object_shape(
            node,
            &["type", "id", "name", "input"],
            &["type", "id", "name", "input"],
        ),
        "tool_result" => exact_object_shape(
            node,
            &["type", "tool_use_id", "content", "is_error"],
            &["type", "tool_use_id", "content"],
        ),
        "search_result" | "document" => exact_object_shape(
            node,
            &[
                "type", "title", "context", "text", "content", "source", "id",
            ],
            &["type"],
        ),
        "thinking" => exact_object_shape(
            node,
            &["type", "thinking", "signature"],
            &["type", "thinking", "signature"],
        ),
        "redacted_thinking" => exact_object_shape(node, &["type", "data"], &["type", "data"]),
        _ => Err(CodecErrorCode::InternalCoverageFailure),
    }
}

fn prove_sse_block_start_shape(node: &JsonNode) -> Result<BlockKind, CodecErrorCode> {
    let kind = match object_string(node, "type")? {
        "text" => {
            exact_object_shape(node, &["type", "text"], &["type", "text"])?;
            BlockKind::Text
        }
        "tool_use" => {
            exact_object_shape(
                node,
                &["type", "id", "name", "input"],
                &["type", "id", "name", "input"],
            )?;
            BlockKind::InputJson
        }
        "thinking" => {
            exact_object_shape(
                node,
                &["type", "thinking", "signature"],
                &["type", "thinking"],
            )?;
            BlockKind::Thinking
        }
        "redacted_thinking" => {
            exact_object_shape(node, &["type", "data"], &["type", "data"])?;
            BlockKind::RedactedThinking
        }
        _ => return Err(CodecErrorCode::InternalCoverageFailure),
    };
    Ok(kind)
}

fn prove_sse_delta_shape(
    node: &JsonNode,
    kind: BlockKind,
    limits: CodecLimits,
) -> Result<(), CodecErrorCode> {
    match (kind, object_string(node, "type")?) {
        (BlockKind::Text, "text_delta") => {
            exact_object_shape(node, &["type", "text"], &["type", "text"])
        }
        (BlockKind::Text, "citations_delta") => {
            exact_object_shape(node, &["type", "citation"], &["type", "citation"])
        }
        (BlockKind::InputJson, "input_json_delta") => {
            exact_object_shape(node, &["type", "partial_json"], &["type", "partial_json"])?;
            prove_rebuilt_partial_json(object_string(node, "partial_json")?, limits)
        }
        (BlockKind::Thinking, "thinking_delta") => {
            exact_object_shape(node, &["type", "thinking"], &["type", "thinking"])
        }
        (BlockKind::Thinking | BlockKind::RedactedThinking, "signature_delta") => {
            exact_object_shape(node, &["type", "signature"], &["type", "signature"])
        }
        _ => Err(CodecErrorCode::InternalCoverageFailure),
    }
}

fn prove_rebuilt_partial_json(
    partial_json: &str,
    limits: CodecLimits,
) -> Result<(), CodecErrorCode> {
    let document = parse_json(partial_json.as_bytes(), limits, CodecPhase::SseReplay)
        .map_err(|error| error.code())?;
    let mut ledger = CoverageLedger::new(document.occurrence_count);
    account_subtree(&document.root, &mut ledger)?;
    ledger.finish()?;
    prove_final_mutable_shape(&document.root)?;
    let (reproved, _) = write_document(&document, partial_json.as_bytes())?;
    if reproved == partial_json.as_bytes() {
        Ok(())
    } else {
        Err(CodecErrorCode::InternalCoverageFailure)
    }
}

fn prove_final_mutable_shape(node: &JsonNode) -> Result<(), CodecErrorCode> {
    match &node.kind {
        JsonKind::Object(members) => {
            for member in members {
                prove_final_mutable_shape(&member.value)?;
            }
            Ok(())
        }
        JsonKind::Array(values) => {
            for value in values {
                prove_final_mutable_shape(value)?;
            }
            Ok(())
        }
        JsonKind::String(_) | JsonKind::Bool(_) | JsonKind::Null => Ok(()),
        JsonKind::Number(_) => Err(CodecErrorCode::InternalCoverageFailure),
    }
}

fn prove_message_delta_shape(node: &JsonNode) -> Result<(), CodecErrorCode> {
    exact_object_shape(
        node,
        &["type", "delta", "usage"],
        &["type", "delta", "usage"],
    )?;
    exact_object_shape(
        object_value(node, "delta")?,
        &["stop_reason", "stop_sequence"],
        &["stop_reason", "stop_sequence"],
    )?;
    let delta = object_value(node, "delta")?;
    prove_optional_enum_string(
        object_value(delta, "stop_reason")?,
        &[
            "end_turn",
            "max_tokens",
            "stop_sequence",
            "tool_use",
            "pause_turn",
            "refusal",
            "model_context_window_exceeded",
        ],
    )?;
    prove_optional_string(object_value(delta, "stop_sequence")?)?;
    prove_usage_shape(object_value(node, "usage")?, false)
}

fn prove_optional_enum_string(node: &JsonNode, allowed: &[&str]) -> Result<(), CodecErrorCode> {
    match &node.kind {
        JsonKind::String(value) if allowed.contains(&value.value.as_str()) => Ok(()),
        JsonKind::Null => Ok(()),
        _ => Err(CodecErrorCode::InternalCoverageFailure),
    }
}

fn prove_optional_string(node: &JsonNode) -> Result<(), CodecErrorCode> {
    if matches!(&node.kind, JsonKind::String(_) | JsonKind::Null) {
        Ok(())
    } else {
        Err(CodecErrorCode::InternalCoverageFailure)
    }
}

fn prove_usage_shape(node: &JsonNode, require_input_tokens: bool) -> Result<(), CodecErrorCode> {
    const KEYS: &[&str] = &[
        "input_tokens",
        "output_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ];
    let required: &[&str] = if require_input_tokens {
        &["input_tokens", "output_tokens"]
    } else {
        &["output_tokens"]
    };
    exact_object_shape(node, KEYS, required)?;
    let JsonKind::Object(members) = &node.kind else {
        return Err(CodecErrorCode::InternalCoverageFailure);
    };
    if members.iter().any(|member| {
        !matches!(&member.value.kind, JsonKind::Number(raw) if !raw.starts_with('-') && !raw.contains(['.', 'e', 'E']))
    }) {
        return Err(CodecErrorCode::InternalCoverageFailure);
    }
    Ok(())
}

fn validate_simple_event(document: &JsonDocument, expected: &str) -> Result<(), CodecError> {
    let JsonKind::Object(members) = &document.root.kind else {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseDecode,
        ));
    };
    if members.len() != 1 || object_string(&document.root, "type") != Ok(expected) {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseDecode,
        ));
    }
    validate_full_coverage(document)
}

fn validate_full_coverage(document: &JsonDocument) -> Result<(), CodecError> {
    let mut ledger = CoverageLedger::new(document.occurrence_count);
    account_subtree(&document.root, &mut ledger)
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    ledger
        .finish()
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))
}

fn transform_message_start(
    document: &mut JsonDocument,
    input: &[u8],
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<(), CodecError> {
    let mut ledger = CoverageLedger::new(document.occurrence_count);
    ledger
        .mark(document.root.occurrence)
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    let JsonKind::Object(members) = &mut document.root.kind else {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseLifecycle,
        ));
    };
    let mut saw_message = false;
    for member in members {
        ledger
            .mark(member.key_occurrence)
            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
        match member.key.value.as_str() {
            "type" => response_string_control(&mut member.value, ctx, &mut ledger),
            "message" => {
                saw_message = true;
                transform_response_root(&mut member.value, input, ctx, &mut ledger)
            }
            _ => Err(CodecErrorCode::UnsupportedResponseSurface),
        }
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    }
    if !saw_message {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseLifecycle,
        ));
    }
    ledger
        .finish()
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))
}

fn event_index(document: &JsonDocument) -> Result<u32, CodecError> {
    let JsonKind::Object(members) = &document.root.kind else {
        return Err(codec_error(
            CodecErrorCode::InvalidContentBlockIndex,
            CodecPhase::SseLifecycle,
        ));
    };
    let index = members
        .iter()
        .find(|member| member.key.value == "index")
        .ok_or_else(|| {
            codec_error(
                CodecErrorCode::InvalidContentBlockIndex,
                CodecPhase::SseLifecycle,
            )
        })?;
    let JsonKind::Number(raw) = &index.value.kind else {
        return Err(codec_error(
            CodecErrorCode::InvalidContentBlockIndex,
            CodecPhase::SseLifecycle,
        ));
    };
    if raw.starts_with('-') || raw.contains(['.', 'e', 'E']) {
        return Err(codec_error(
            CodecErrorCode::InvalidContentBlockIndex,
            CodecPhase::SseLifecycle,
        ));
    }
    raw.parse().map_err(|_| {
        codec_error(
            CodecErrorCode::InvalidContentBlockIndex,
            CodecPhase::SseLifecycle,
        )
    })
}

fn transform_block_start(
    document: &mut JsonDocument,
    input: &[u8],
    raw_frame: &[u8],
    start_slot: usize,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<BlockAccumulator, CodecError> {
    let mut ledger = CoverageLedger::new(document.occurrence_count);
    ledger
        .mark(document.root.occurrence)
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    let JsonKind::Object(members) = &mut document.root.kind else {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseLifecycle,
        ));
    };
    let mut content_block = None;
    for member in members {
        ledger
            .mark(member.key_occurrence)
            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
        match member.key.value.as_str() {
            "type" => response_string_control(&mut member.value, ctx, &mut ledger),
            "index" => {
                ledger
                    .mark(member.value.occurrence)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                if matches!(member.value.kind, JsonKind::Number(_)) {
                    Ok(())
                } else {
                    Err(CodecErrorCode::InvalidContentBlockIndex)
                }
            }
            "content_block" => {
                content_block = Some(&mut member.value);
                Ok(())
            }
            _ => Err(CodecErrorCode::InvalidSseFrame),
        }
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    }
    let block = content_block
        .ok_or_else(|| codec_error(CodecErrorCode::InvalidSseFrame, CodecPhase::SseLifecycle))?;
    let block_type = object_string(block, "type")
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?
        .to_owned();
    let mut logical = String::new();
    let mut signature = String::new();
    let kind = match block_type.as_str() {
        "text" => {
            ledger
                .mark(block.occurrence)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            let JsonKind::Object(block_members) = &mut block.kind else {
                return Err(codec_error(
                    CodecErrorCode::InvalidSseFrame,
                    CodecPhase::SseLifecycle,
                ));
            };
            let mut saw_text = false;
            for member in block_members {
                ledger
                    .mark(member.key_occurrence)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                match member.key.value.as_str() {
                    "type" => {
                        response_enum_string_control(&mut member.value, &["text"], ctx, &mut ledger)
                            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?
                    }
                    "text" => {
                        ledger
                            .mark(member.value.occurrence)
                            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                        let JsonKind::String(value) = &mut member.value.kind else {
                            return Err(codec_error(
                                CodecErrorCode::InvalidSseFrame,
                                CodecPhase::SseLifecycle,
                            ));
                        };
                        logical.push_str(&value.value);
                        value.value.clear();
                        saw_text = true;
                    }
                    _ => {
                        return Err(codec_error(
                            CodecErrorCode::InvalidSseFrame,
                            CodecPhase::SseLifecycle,
                        ));
                    }
                }
            }
            if !saw_text {
                return Err(codec_error(
                    CodecErrorCode::InvalidSseFrame,
                    CodecPhase::SseLifecycle,
                ));
            }
            BlockKind::Text
        }
        "tool_use" => {
            ledger
                .mark(block.occurrence)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            let JsonKind::Object(block_members) = &mut block.kind else {
                return Err(codec_error(
                    CodecErrorCode::InvalidSseFrame,
                    CodecPhase::SseLifecycle,
                ));
            };
            let mut saw_id = false;
            let mut saw_name = false;
            let mut saw_input = false;
            for member in block_members {
                ledger
                    .mark(member.key_occurrence)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                match member.key.value.as_str() {
                    "type" => response_enum_string_control(
                        &mut member.value,
                        &["tool_use"],
                        ctx,
                        &mut ledger,
                    )
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?,
                    "id" => {
                        response_nonempty_string_control(&mut member.value, ctx, &mut ledger)
                            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                        saw_id = true;
                    }
                    "name" => {
                        response_nonempty_string_control(&mut member.value, ctx, &mut ledger)
                            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                        saw_name = true;
                    }
                    "input" => {
                        if !matches!(&member.value.kind, JsonKind::Object(values) if values.is_empty())
                        {
                            return Err(codec_error(
                                CodecErrorCode::InvalidContentBlockDelta,
                                CodecPhase::SseLifecycle,
                            ));
                        }
                        account_subtree(&member.value, &mut ledger)
                            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                        saw_input = true;
                    }
                    _ => {
                        return Err(codec_error(
                            CodecErrorCode::InvalidSseFrame,
                            CodecPhase::SseLifecycle,
                        ));
                    }
                }
            }
            if !saw_id || !saw_name || !saw_input {
                return Err(codec_error(
                    CodecErrorCode::InvalidSseFrame,
                    CodecPhase::SseLifecycle,
                ));
            }
            BlockKind::InputJson
        }
        "thinking" | "redacted_thinking" => {
            let (initial_logical, initial_signature) =
                validate_signed_sse_block(block, &block_type, ctx)?;
            logical = initial_logical;
            signature = initial_signature;
            account_subtree(block, &mut ledger)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            if block_type == "thinking" {
                BlockKind::Thinking
            } else {
                BlockKind::RedactedThinking
            }
        }
        _ => {
            return Err(codec_error(
                CodecErrorCode::UnsupportedResponseSurface,
                CodecPhase::SseLifecycle,
            ));
        }
    };
    ledger
        .finish()
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    let start_frame = if matches!(kind, BlockKind::Thinking | BlockKind::RedactedThinking) {
        raw_frame.to_vec()
    } else {
        canonical_sse_frame("content_block_start", document, input)?.0
    };
    let mut signed_frames = Vec::new();
    if matches!(kind, BlockKind::Thinking | BlockKind::RedactedThinking) {
        signed_frames.push((start_slot, raw_frame.to_vec()));
    }
    Ok(BlockAccumulator {
        kind,
        start_slot,
        start_frame,
        delta_slots: Vec::new(),
        logical,
        signature,
        signed_frames,
        passthrough_frames: Vec::new(),
    })
}

fn validate_signed_sse_block(
    block: &JsonNode,
    block_type: &str,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<(String, String), CodecError> {
    let JsonKind::Object(members) = &block.kind else {
        return Err(codec_error(
            CodecErrorCode::SignedSurfaceMalformed,
            CodecPhase::SseLifecycle,
        ));
    };
    let mut logical = String::new();
    let mut signature = String::new();
    let mut has_payload = false;
    let mut has_type = false;
    for member in members {
        match member.key.value.as_str() {
            "type" if string_value(&member.value) == Ok(block_type) => {
                has_type = true;
            }
            "thinking" if block_type == "thinking" => {
                let value = string_value(&member.value)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                guard_mutable_text_response(value)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                let _ = ctx
                    .restore(value)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                ctx.validate(value, &[])
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                logical.push_str(value);
                has_payload = true;
            }
            "signature" if block_type == "thinking" => {
                let value = string_value(&member.value)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                let _ = ctx
                    .restore(value)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                signature.push_str(value);
            }
            "data" if block_type == "redacted_thinking" => {
                let value = string_value(&member.value)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                let _ = ctx
                    .restore(value)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                has_payload = true;
            }
            _ => {
                return Err(codec_error(
                    CodecErrorCode::SignedSurfaceMalformed,
                    CodecPhase::SseLifecycle,
                ));
            }
        }
    }
    if !has_type || !has_payload {
        return Err(codec_error(
            CodecErrorCode::SignedSurfaceMalformed,
            CodecPhase::SseLifecycle,
        ));
    }
    Ok((logical, signature))
}

fn transform_block_delta(
    accumulator: &mut BlockAccumulator,
    document: &mut JsonDocument,
    input: &[u8],
    raw_frame: &[u8],
    slot: usize,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<(), CodecError> {
    let mut ledger = CoverageLedger::new(document.occurrence_count);
    ledger
        .mark(document.root.occurrence)
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    let JsonKind::Object(members) = &mut document.root.kind else {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseLifecycle,
        ));
    };
    let mut delta = None;
    for member in members {
        ledger
            .mark(member.key_occurrence)
            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
        match member.key.value.as_str() {
            "type" => response_string_control(&mut member.value, ctx, &mut ledger)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?,
            "index" => account_subtree(&member.value, &mut ledger)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?,
            "delta" => delta = Some(&mut member.value),
            _ => {
                return Err(codec_error(
                    CodecErrorCode::InvalidSseFrame,
                    CodecPhase::SseLifecycle,
                ));
            }
        }
    }
    let delta = delta
        .ok_or_else(|| codec_error(CodecErrorCode::InvalidSseFrame, CodecPhase::SseLifecycle))?;
    let delta_type = object_string(delta, "type")
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?
        .to_owned();
    if delta_type == "citations_delta" {
        if !matches!(accumulator.kind, BlockKind::Text) {
            return Err(codec_error(
                CodecErrorCode::InvalidContentBlockDelta,
                CodecPhase::SseLifecycle,
            ));
        }
        transform_response_citation_delta(delta, ctx, &mut ledger)
            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
        ledger
            .finish()
            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
        let (bytes, provenance) = canonical_sse_frame("content_block_delta", document, input)?;
        accumulator
            .passthrough_frames
            .push((slot, bytes, provenance));
        return Ok(());
    }
    account_subtree(delta, &mut ledger)
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    let value = match delta_type.as_str() {
        "text_delta" => object_string(delta, "text"),
        "input_json_delta" => object_string(delta, "partial_json"),
        "thinking_delta" => object_string(delta, "thinking"),
        "signature_delta" => object_string(delta, "signature"),
        _ => Err(CodecErrorCode::InvalidContentBlockDelta),
    }
    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    match (&accumulator.kind, delta_type.as_str()) {
        (BlockKind::Text, "text_delta") | (BlockKind::InputJson, "input_json_delta") => {
            accumulator.logical.push_str(value);
            accumulator.delta_slots.push(slot);
        }
        (BlockKind::Thinking, "thinking_delta") => {
            accumulator.logical.push_str(value);
            accumulator.signed_frames.push((slot, raw_frame.to_vec()));
        }
        (BlockKind::Thinking, "signature_delta") => {
            accumulator.signature.push_str(value);
            accumulator.signed_frames.push((slot, raw_frame.to_vec()));
        }
        (BlockKind::RedactedThinking, "signature_delta") => {
            accumulator.signature.push_str(value);
            accumulator.signed_frames.push((slot, raw_frame.to_vec()));
        }
        _ => {
            return Err(codec_error(
                CodecErrorCode::InvalidContentBlockDelta,
                CodecPhase::SseLifecycle,
            ));
        }
    }
    if accumulator.logical.len() > ctx.limits().max_sse_accumulator_bytes()
        || accumulator.signature.len() > ctx.limits().max_sse_accumulator_bytes()
    {
        return Err(codec_error(
            CodecErrorCode::SseAccumulatorLimitExceeded,
            CodecPhase::SseLifecycle,
        ));
    }
    ledger
        .finish()
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))
}

fn transform_response_citation_delta(
    delta: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(delta.occurrence)?;
    let JsonKind::Object(members) = &mut delta.kind else {
        return Err(CodecErrorCode::InvalidContentBlockDelta);
    };
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "type" => response_string_control(&mut member.value, ctx, ledger)?,
            "citation" => transform_response_citation(&mut member.value, ctx, ledger)?,
            _ => return Err(CodecErrorCode::InvalidContentBlockDelta),
        }
    }
    Ok(())
}

struct StopFrame<'document, 'input> {
    slot: usize,
    document: &'document mut JsonDocument,
    input: &'input [u8],
}

fn finish_block(
    index: u32,
    accumulator: BlockAccumulator,
    slots: &mut [ReplaySlot],
    stop: StopFrame<'_, '_>,
    ctx: &mut ResponseTransformContext<'_>,
    concatenated_text: &mut String,
    concatenated_authorized: &mut Vec<Range<usize>>,
) -> Result<(), CodecError> {
    validate_stop_event(stop.document)?;
    let (stop_frame, stop_provenance) =
        canonical_sse_frame("content_block_stop", stop.document, stop.input)?;
    slots[stop.slot] = ReplaySlot {
        bytes: Some(stop_frame),
        provenance: stop_provenance,
    };
    match accumulator.kind {
        BlockKind::Text => {
            let restored = ctx
                .restore(&accumulator.logical)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            guard_mutable_text_response(&restored.text)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            ctx.validate(&restored.text, &restored.authorized_output_ranges)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            let base = concatenated_text.len();
            concatenated_text.push_str(&restored.text);
            concatenated_authorized.extend(
                restored
                    .authorized_output_ranges
                    .iter()
                    .map(|range| base + range.start..base + range.end),
            );
            slots[accumulator.start_slot].bytes = Some(accumulator.start_frame);
            if let Some(first) = accumulator.delta_slots.first().copied() {
                let (bytes, provenance) = built_delta_frame(
                    index,
                    "text_delta",
                    "text",
                    &restored.text,
                    restored.authorized_output_ranges,
                );
                slots[first] = ReplaySlot {
                    bytes: Some(bytes),
                    provenance,
                };
            } else if !restored.text.is_empty() {
                return Err(codec_error(
                    CodecErrorCode::InvalidContentBlockDelta,
                    CodecPhase::SseLifecycle,
                ));
            }
            for (slot, bytes, provenance) in accumulator.passthrough_frames {
                slots[slot] = ReplaySlot {
                    bytes: Some(bytes),
                    provenance,
                };
            }
        }
        BlockKind::InputJson => {
            let partial_input =
                if accumulator.delta_slots.is_empty() && accumulator.logical.is_empty() {
                    "{}"
                } else {
                    accumulator.logical.as_str()
                };
            let mut partial = parse_json(
                partial_input.as_bytes(),
                ctx.limits(),
                CodecPhase::SseLifecycle,
            )?;
            let mut ledger = CoverageLedger::new(partial.occurrence_count);
            transform_response_mutable(&mut partial.root, ctx, &mut ledger)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            ledger
                .finish()
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            let (partial_bytes, partial_provenance) =
                write_document(&partial, partial_input.as_bytes())
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            let partial_text = String::from_utf8(partial_bytes)
                .map_err(|_| codec_error(CodecErrorCode::InvalidUtf8, CodecPhase::SseLifecycle))?;
            slots[accumulator.start_slot].bytes = Some(accumulator.start_frame);
            if let Some(first) = accumulator.delta_slots.first().copied() {
                let (bytes, provenance) = built_delta_frame(
                    index,
                    "input_json_delta",
                    "partial_json",
                    &partial_text,
                    partial_provenance.authorized_output_ranges().to_vec(),
                );
                slots[first] = ReplaySlot {
                    bytes: Some(bytes),
                    provenance,
                };
            } else if partial_text != "{}" {
                return Err(codec_error(
                    CodecErrorCode::InvalidContentBlockDelta,
                    CodecPhase::SseLifecycle,
                ));
            }
        }
        BlockKind::Thinking => {
            guard_mutable_text_response(&accumulator.logical)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            let _ = ctx
                .restore(&accumulator.logical)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            ctx.validate(&accumulator.logical, &[])
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            let _ = ctx
                .restore(&accumulator.signature)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            for (slot, raw) in accumulator.signed_frames {
                let mut provenance = OutputProvenance::default();
                provenance.record_signed(0..raw.len());
                slots[slot] = ReplaySlot {
                    bytes: Some(raw),
                    provenance,
                };
            }
        }
        BlockKind::RedactedThinking => {
            let _ = ctx
                .restore(&accumulator.signature)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
            for (slot, raw) in accumulator.signed_frames {
                let mut provenance = OutputProvenance::default();
                provenance.record_signed(0..raw.len());
                slots[slot] = ReplaySlot {
                    bytes: Some(raw),
                    provenance,
                };
            }
        }
    }
    Ok(())
}

fn validate_stop_event(document: &JsonDocument) -> Result<(), CodecError> {
    let JsonKind::Object(members) = &document.root.kind else {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseLifecycle,
        ));
    };
    let valid = members.len() == 2
        && members.iter().any(|member| member.key.value == "type")
        && members.iter().any(|member| member.key.value == "index");
    if valid {
        validate_full_coverage(document)
    } else {
        Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseLifecycle,
        ))
    }
}

fn transform_message_delta(
    document: &mut JsonDocument,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<(), CodecError> {
    let mut ledger = CoverageLedger::new(document.occurrence_count);
    ledger
        .mark(document.root.occurrence)
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
    let JsonKind::Object(members) = &mut document.root.kind else {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseLifecycle,
        ));
    };
    let mut saw_delta = false;
    let mut saw_usage = false;
    for member in members {
        ledger
            .mark(member.key_occurrence)
            .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
        match member.key.value.as_str() {
            "type" => response_string_control(&mut member.value, ctx, &mut ledger)
                .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?,
            "delta" => {
                transform_message_delta_controls(&mut member.value, ctx, &mut ledger)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                saw_delta = true;
            }
            "usage" => {
                response_usage(&mut member.value, &mut ledger, false)
                    .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))?;
                saw_usage = true;
            }
            _ => {
                return Err(codec_error(
                    CodecErrorCode::InvalidSseFrame,
                    CodecPhase::SseLifecycle,
                ));
            }
        }
    }
    if !saw_delta || !saw_usage {
        return Err(codec_error(
            CodecErrorCode::InvalidSseFrame,
            CodecPhase::SseLifecycle,
        ));
    }
    ledger
        .finish()
        .map_err(|code| codec_error(code, CodecPhase::SseLifecycle))
}

fn transform_message_delta_controls(
    delta: &mut JsonNode,
    ctx: &mut ResponseTransformContext<'_>,
    ledger: &mut CoverageLedger,
) -> Result<(), CodecErrorCode> {
    ledger.mark(delta.occurrence)?;
    let JsonKind::Object(members) = &mut delta.kind else {
        return Err(CodecErrorCode::InvalidSseFrame);
    };
    let mut saw_reason = false;
    let mut saw_sequence = false;
    for member in members {
        ledger.mark(member.key_occurrence)?;
        match member.key.value.as_str() {
            "stop_reason" => {
                response_enum_string_or_null(
                    &mut member.value,
                    &[
                        "end_turn",
                        "max_tokens",
                        "stop_sequence",
                        "tool_use",
                        "pause_turn",
                        "refusal",
                        "model_context_window_exceeded",
                    ],
                    ctx,
                    ledger,
                )?;
                saw_reason = true;
            }
            "stop_sequence" => {
                response_string_or_null(&mut member.value, ctx, ledger)?;
                saw_sequence = true;
            }
            _ => return Err(CodecErrorCode::InvalidSseFrame),
        }
    }
    if saw_reason && saw_sequence {
        Ok(())
    } else {
        Err(CodecErrorCode::InvalidSseFrame)
    }
}

fn validate_future_event(
    document: &JsonDocument,
    event: &str,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<(), CodecError> {
    let JsonKind::Object(members) = &document.root.kind else {
        return Err(codec_error(
            CodecErrorCode::UnsafeFutureEvent,
            CodecPhase::SseLifecycle,
        ));
    };
    validate_future_atom(event, ctx)?;
    for member in members {
        validate_future_atom(&member.key.value, ctx)?;
        match (&*member.key.value, &member.value.kind) {
            ("type", JsonKind::String(value)) if value.value == event => {}
            (_, JsonKind::Bool(_) | JsonKind::Null) => {}
            _ => {
                return Err(codec_error(
                    CodecErrorCode::UnsafeFutureEvent,
                    CodecPhase::SseLifecycle,
                ));
            }
        }
    }
    validate_full_coverage(document)
}

fn validate_future_atom(
    value: &str,
    ctx: &mut ResponseTransformContext<'_>,
) -> Result<(), CodecError> {
    let unsafe_event = || codec_error(CodecErrorCode::UnsafeFutureEvent, CodecPhase::SseLifecycle);
    guard_mutable_text_response(value).map_err(|_| unsafe_event())?;
    let restored = ctx.restore(value).map_err(|_| unsafe_event())?;
    if restored.text != value || !restored.authorized_output_ranges.is_empty() {
        return Err(unsafe_event());
    }
    ctx.validate(value, &[]).map_err(|_| unsafe_event())
}

fn canonical_sse_frame(
    event: &str,
    document: &JsonDocument,
    input: &[u8],
) -> Result<(Vec<u8>, OutputProvenance), CodecError> {
    let (json, json_provenance) =
        write_document(document, input).map_err(|code| codec_error(code, CodecPhase::SseReplay))?;
    let mut output = Vec::with_capacity(event.len() + json.len() + 16);
    output.extend_from_slice(b"event: ");
    output.extend_from_slice(event.as_bytes());
    output.extend_from_slice(b"\ndata: ");
    let offset = output.len();
    output.extend_from_slice(&json);
    output.extend_from_slice(b"\n\n");
    let mut provenance = OutputProvenance::default();
    for range in json_provenance.authorized_output_ranges() {
        provenance.record_authorized(offset + range.start..offset + range.end);
    }
    for range in json_provenance.signed_opaque_ranges() {
        provenance.record_signed(offset + range.start..offset + range.end);
    }
    Ok((output, provenance))
}

fn built_delta_frame(
    index: u32,
    delta_type: &str,
    field: &str,
    value: &str,
    authorized: Vec<Range<usize>>,
) -> (Vec<u8>, OutputProvenance) {
    let mut output = format!(
        "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":{index},\"delta\":{{\"type\":\"{delta_type}\",\"{field}\":"
    )
    .into_bytes();
    let mut provenance = OutputProvenance::default();
    write_json_string(
        &JsonString {
            value: value.to_owned(),
            authorized,
        },
        &mut output,
        &mut provenance,
    );
    output.extend_from_slice(b"}}\n\n");
    (output, provenance)
}

#[cfg(test)]
mod inspection_parity_tests {
    use gaze::{Scope, Session};

    use super::*;
    use crate::codec::ResponseResidualValidator;

    struct CleanResidual;

    impl ResponseResidualValidator for CleanResidual {
        fn validate(
            &mut self,
            _value: &str,
            _authorized_output_ranges: &[Range<usize>],
        ) -> Result<(), CodecErrorCode> {
            Ok(())
        }
    }

    fn malformed_sse_error(inspection_configured: bool) -> CodecError {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let snapshot = session.begin_transaction().commit().unwrap();
        let mut residual = CleanResidual;
        let mut context = ResponseTransformContext::new(
            &snapshot,
            &mut residual,
            WireFormat::Sse,
            CodecLimits::default(),
        );
        if inspection_configured {
            context.request_inspection_projection();
        }
        AnthropicMessagesCodec
            .restore_response(
                b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                &mut context,
            )
            .unwrap_err()
    }

    #[test]
    fn malformed_sse_error_is_identical_with_and_without_inspection() {
        let without = malformed_sse_error(false);
        let with = malformed_sse_error(true);
        assert_eq!(with.code(), without.code());
        assert_eq!(with.phase(), without.phase());
    }
}
