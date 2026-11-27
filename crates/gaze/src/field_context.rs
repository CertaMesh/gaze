//! Structured-field context for whole text documents.
//!
//! Two producers locate scalar fields inside a document, in its source byte coordinates:
//! a strict parse of a document that is itself one JSON object or array
//! ([`FieldEvidence::ValidatedParse`]), and the caller's context `fields` map
//! ([`FieldEvidence::CallerDeclared`]). Context alone changes no detection. A document that
//! does not parse strictly gets no parse context; nothing is ever marked non-sensitive.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use gaze_types::json_tree::{self, JsonKind, JsonNode, JsonTreeLimits};
use gaze_types::{is_inside_word, FieldEvidence, FieldPath, FieldProvenance, JsonScalarType};
use serde_json::{Map, Value};

/// Caller-declared scalar fields, located in a document by their exact value.
///
/// Built from the context JSON `fields` map: object keys and array indices form each leaf's
/// path. String and number leaves are located wherever their exact spelling occurs without
/// cutting a word; booleans, nulls and empty strings have no locatable value. Values are
/// private: `Debug` reports only the field count.
#[derive(Clone, Default)]
pub struct CallerFields(Arc<[CallerField]>);

struct CallerField {
    provenance: FieldProvenance,
    value: String,
}

impl CallerFields {
    pub fn from_map(fields: &Map<String, Value>) -> Self {
        let mut leaves = Vec::new();
        let mut pending: Vec<(FieldPath, &Value)> = fields
            .iter()
            .rev()
            .map(|(key, value)| (FieldPath::default().child(key.clone()), value))
            .collect();
        // An explicit stack keeps document order without recursing on caller nesting.
        while let Some((path, value)) = pending.pop() {
            let (json_type, value) = match value {
                Value::Object(map) => {
                    pending.extend(
                        map.iter()
                            .rev()
                            .map(|(key, value)| (path.child(key.clone()), value)),
                    );
                    continue;
                }
                Value::Array(values) => {
                    pending.extend(
                        values
                            .iter()
                            .enumerate()
                            .rev()
                            .map(|(index, value)| (path.child(index.to_string()), value)),
                    );
                    continue;
                }
                Value::String(value) => (JsonScalarType::String, value.clone()),
                Value::Number(value) => (JsonScalarType::Number, value.to_string()),
                Value::Bool(_) | Value::Null => continue,
            };
            if value.is_empty() {
                continue;
            }
            leaves.push(CallerField {
                provenance: FieldProvenance::new(path, json_type, FieldEvidence::CallerDeclared),
                value,
            });
        }
        Self(leaves.into())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for CallerFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallerFields")
            .field("count", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// Every located scalar field in `text`, sorted by range, in source byte coordinates.
pub(crate) fn document_field_spans(
    text: &str,
    caller: &CallerFields,
) -> Vec<(Range<usize>, FieldProvenance)> {
    let mut spans = Vec::new();
    parsed_field_spans(text, &mut spans);
    for field in caller.0.iter() {
        for (start, matched) in text.match_indices(field.value.as_str()) {
            let end = start + matched.len();
            if !is_inside_word(text, start) && !is_inside_word(text, end) {
                spans.push((start..end, field.provenance.clone()));
            }
        }
    }
    spans.sort_by_key(|(range, _)| (range.start, range.end));
    spans
}

/// Scalar leaves of a JSON document carried as text inside one field, such as an OpenAI tool
/// call's `arguments`, in that text's byte coordinates. Each leaf's path is the carrier's path
/// followed by the leaf's own, so a labelled key inside the carried document binds exactly as
/// it would in a whole JSON document. Empty unless the text strictly parses as one JSON object
/// or array: arbitrary prose that merely contains JSON is not a carrier.
pub(crate) fn json_text_field_spans(
    text: &str,
    carrier: &FieldPath,
) -> Vec<(Range<usize>, FieldProvenance)> {
    let mut spans = Vec::new();
    parsed_field_spans(text, &mut spans);
    let prefix = carrier.segments();
    for (_, provenance) in &mut spans {
        provenance.path = FieldPath::new(
            prefix
                .iter()
                .chain(provenance.path.segments().iter())
                .map(|segment| (*segment).to_owned()),
        );
    }
    spans
}

/// The field context of one stretch of a leaf, from [`field_pieces`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldPiece {
    /// The complete scalars inside this piece, in piece coordinates; possibly none.
    Spans(Vec<(Range<usize>, FieldProvenance)>),
    /// The part of one scalar that an owned token cuts. It cannot prove the whole value.
    Fragment {
        /// The cut scalar's provenance.
        field: FieldProvenance,
        /// Byte offset of this part inside the scalar.
        offset: usize,
    },
}

/// Split `gap` (a stretch of a leaf between owned tokens) so each piece carries one honest
/// field context: scalars wholly inside the gap stay complete spans, and the part of a scalar
/// that an owned token cuts becomes its own fragment piece. `spans` are disjoint, in leaf
/// coordinates, as [`crate::Pipeline::json_text_field_spans`] returns them. Empty pieces are omitted.
pub fn field_pieces(
    gap: Range<usize>,
    spans: &[(Range<usize>, FieldProvenance)],
) -> Vec<(Range<usize>, FieldPiece)> {
    let mut pieces = Vec::new();
    let mut cursor = gap.start;
    let mut contained: Vec<(Range<usize>, FieldProvenance)> = Vec::new();
    let flush = |pieces: &mut Vec<_>,
                 contained: &mut Vec<(Range<usize>, FieldProvenance)>,
                 start: usize,
                 end: usize| {
        if start < end {
            let spans = contained
                .drain(..)
                .map(|(range, provenance)| (range.start - start..range.end - start, provenance))
                .collect();
            pieces.push((start..end, FieldPiece::Spans(spans)));
        }
    };
    let mut ordered: Vec<_> = spans.iter().collect();
    ordered.sort_by_key(|(range, _)| (range.start, range.end));
    for (range, provenance) in ordered {
        if range.is_empty() || range.end <= gap.start || gap.end <= range.start {
            continue;
        }
        if gap.start <= range.start && range.end <= gap.end {
            contained.push((range.clone(), provenance.clone()));
            continue;
        }
        let cut = range.start.max(gap.start)..range.end.min(gap.end);
        flush(&mut pieces, &mut contained, cursor, cut.start);
        pieces.push((
            cut.clone(),
            FieldPiece::Fragment {
                field: provenance.clone(),
                offset: cut.start - range.start,
            },
        ));
        cursor = cut.end;
    }
    flush(&mut pieces, &mut contained, cursor, gap.end);
    pieces
}

/// Scalar leaves of a document that strictly parses as one JSON object or array.
/// A string's range is its literal's interior, still in source (escaped) spelling.
fn parsed_field_spans(text: &str, spans: &mut Vec<(Range<usize>, FieldProvenance)>) {
    if !matches!(text.trim_start().as_bytes().first(), Some(b'{' | b'[')) {
        return;
    }
    gaze_types::perf_count!("document_json_parses", 1);
    // Unlike the proxy codec, keys may be non-ASCII: German and other locale keys are fields.
    let Ok(tree) = json_tree::parse(text.as_bytes(), JsonTreeLimits::default()) else {
        return;
    };
    let mut pending = vec![tree.root];
    while let Some(node) = pending.pop() {
        let JsonNode {
            path, span, kind, ..
        } = node;
        let (range, json_type) = match kind {
            JsonKind::Object(members) => {
                pending.extend(members.into_iter().rev().map(|member| member.value));
                continue;
            }
            JsonKind::Array(values) => {
                pending.extend(values.into_iter().rev());
                continue;
            }
            JsonKind::String(_) => (span.start + 1..span.end - 1, JsonScalarType::String),
            JsonKind::Number(_) => (span, JsonScalarType::Number),
            JsonKind::Bool(_) => (span, JsonScalarType::Bool),
            JsonKind::Null => (span, JsonScalarType::Null),
        };
        if !range.is_empty() {
            spans.push((
                range,
                FieldProvenance::new(path, json_type, FieldEvidence::ValidatedParse),
            ));
        }
    }
}

/// Carriers a token may not replace in a document that strictly parses as JSON: number
/// literals, and object keys (the interior of the key literal, in source spelling). Each
/// carries the path of the value it holds or names. `None` when the document does not parse.
pub(crate) struct JsonCarriers {
    pub(crate) numbers: Vec<(Range<usize>, FieldPath)>,
    pub(crate) keys: Vec<(Range<usize>, FieldPath)>,
}

pub(crate) fn json_carriers(text: &str) -> Option<JsonCarriers> {
    if !matches!(text.trim_start().as_bytes().first(), Some(b'{' | b'[')) {
        return None;
    }
    let tree = json_tree::parse(text.as_bytes(), JsonTreeLimits::default()).ok()?;
    let mut carriers = JsonCarriers {
        numbers: Vec::new(),
        keys: Vec::new(),
    };
    let mut pending = vec![tree.root];
    while let Some(node) = pending.pop() {
        match node.kind {
            JsonKind::Object(members) => {
                for member in members {
                    if let Some(key) = key_interior(text.as_bytes(), member.value.span.start) {
                        carriers.keys.push((key, member.value.path.clone()));
                    }
                    pending.push(member.value);
                }
            }
            JsonKind::Array(values) => pending.extend(values),
            JsonKind::Number(_) => carriers.numbers.push((node.span, node.path)),
            _ => {}
        }
    }
    Some(carriers)
}

/// The key literal's interior before a member value starting at `value_start`, walking back
/// over `"key" :` in a document the strict parser accepted.
fn key_interior(bytes: &[u8], value_start: usize) -> Option<Range<usize>> {
    let mut at = value_start;
    let skip_ws = |at: &mut usize| {
        while *at > 0 && bytes[*at - 1].is_ascii_whitespace() {
            *at -= 1;
        }
    };
    skip_ws(&mut at);
    (at > 0 && bytes[at - 1] == b':').then_some(())?;
    at -= 1;
    skip_ws(&mut at);
    (at > 0 && bytes[at - 1] == b'"').then_some(())?;
    let end = at - 1;
    let mut start = end;
    while start > 0 {
        start -= 1;
        if bytes[start] == b'"' {
            let escapes = bytes[..start]
                .iter()
                .rev()
                .take_while(|b| **b == b'\\')
                .count();
            if escapes % 2 == 0 {
                return Some(start + 1..end);
            }
        }
    }
    None
}

/// Translate source ranges into a normalized view of the same text.
///
/// `source_spans[i]` is the source range of normalized byte `i`; both ends are monotone. A
/// range keeps the normalized bytes whose source lies wholly inside it. Ranges that become
/// empty or would split a normalized character are dropped rather than guessed.
pub(crate) fn project_to_normalized(
    spans: &[(Range<usize>, FieldProvenance)],
    source_spans: &[(usize, usize)],
    normalized: &str,
) -> Vec<(Range<usize>, FieldProvenance)> {
    spans
        .iter()
        .filter_map(|(range, field)| {
            let start = source_spans.partition_point(|(from, _)| *from < range.start);
            let end = source_spans.partition_point(|(_, to)| *to <= range.end);
            (start < end && normalized.is_char_boundary(start) && normalized.is_char_boundary(end))
                .then(|| (start..end, field.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests;
