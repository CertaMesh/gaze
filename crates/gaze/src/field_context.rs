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
