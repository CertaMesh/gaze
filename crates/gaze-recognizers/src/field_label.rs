//! `field_labelled`: protect a scalar because its structured field label names a
//! person-linked identifier.
//!
//! The recognizer reads only [`DetectContext::fields`]. It never guesses labels in prose and
//! never treats a value's shape alone as evidence. A field binds when its final key is a
//! declared label, or when a declared label is the parent of an `id`/`value`/`number`/`no`/`nr`
//! key, or when parent and child spell a label together (`{"customer": {"id": …}}`). Array
//! indices are transparent anywhere in the path, so `{"customer_id": [{"value": …}]}` binds,
//! but only the nearest object key is ever the parent: a label never reaches through an
//! unrelated object (`{"customer_id": [{"meta": {"value": …}}]}` does not bind). Labels compare
//! through the shared detection normalization (fullwidth folded, default-ignorable code points
//! such as U+200B removed), then without case, `_`, `-`, `.` and whitespace, so snake, camel,
//! kebab and fullwidth spellings agree. Paths keep their original keys.
//!
//! | Carrier | Evidence | Outcome |
//! |---|---|---|
//! | string, digit-bearing value ≤ 40 bytes and ≤ 4 groups | validated parse or caller | `Protected` |
//! | string, other value | validated parse or caller | `PossiblySensitive(LabelValueShapeUnclear)` |
//! | any | hint only | `PossiblySensitive(LabelBindingUnproven)` |
//! | JSON number | validated parse or caller | `ConfirmedUntransformable(JsonNumberNoTextOnlyPath)` |
//!
//! Booleans and nulls carry no identifier bytes, so they produce no candidate. A fragment of a
//! labelled leaf (the text beside an owned token) cannot establish the complete value, so it is
//! never `Protected`; its identifier bytes are still `PossiblySensitive(LabelValueShapeUnclear)`
//! (or `LabelBindingUnproven` for a hint), so the rest of a labelled value never ships raw.
//! Unlabelled values never produce a candidate; other recognizers keep their own contracts.

use std::collections::BTreeSet;
use std::ops::Range;

use gaze_types::{
    Candidate, ConflictTier, DetectContext, FieldContext, FieldEvidence, FieldProvenance,
    JsonScalarType, LocaleBasis, LocaleTag, PiiClass, PossiblySensitiveReason, ProtectionOutcome,
    Recognizer, UntransformableReason,
};

/// Keys that hold the identifier of their parent label: `{"customer_id": {"value": …}}`.
const VALUE_KEYS: [&str; 5] = ["id", "value", "number", "no", "nr"];
/// Keys that complete a parent label: `{"customer": {"id": …}}` spells `customerid`.
const SUFFIX_KEYS: [&str; 4] = ["id", "number", "no", "nr"];
/// The labelled-identifier bounds shared with `complete_labelled_value`.
const MAX_VALUE_BYTES: usize = 40;
const MAX_VALUE_GROUPS: usize = 4;

/// Recognizer for scalars whose structured field label names a person-linked identifier.
pub struct FieldLabelRecognizer {
    id: String,
    class: PiiClass,
    labels: BTreeSet<String>,
    score: f32,
    priority: i32,
    token_family: String,
    locales: Vec<LocaleTag>,
    locale_basis: LocaleBasis,
}

impl FieldLabelRecognizer {
    /// Build from declared labels; each label is normalized and empty labels are dropped.
    pub fn new(
        id: impl Into<String>,
        class: PiiClass,
        labels: impl IntoIterator<Item = impl AsRef<str>>,
        score: f32,
        priority: i32,
        token_family: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            class,
            labels: labels
                .into_iter()
                .map(|label| normalize_label(label.as_ref()))
                .filter(|label| !label.is_empty())
                .collect(),
            score,
            priority,
            token_family: token_family.into(),
            locales: vec![LocaleTag::Global],
            locale_basis: LocaleBasis::Format,
        }
    }

    pub fn with_locale_metadata(
        mut self,
        locales: Vec<LocaleTag>,
        locale_basis: LocaleBasis,
    ) -> Self {
        self.locales = locales;
        self.locale_basis = locale_basis;
        self
    }

    /// Whether a field path binds to one of this recognizer's labels.
    pub fn binds(&self, path: &[&str]) -> bool {
        // Array indices are transparent: `customer_id[0]` is still the label, and
        // `customer_id[0].value` has `customer_id` as its parent. Only object keys remain, so
        // the parent is always the nearest object key, never a more distant one.
        let keys: Vec<String> = path
            .iter()
            .filter(|segment| !is_index(segment))
            .map(|key| normalize_label(key))
            .collect();
        let Some(last) = keys.last() else {
            return false;
        };
        if self.labels.contains(last) {
            return true;
        }
        let Some(parent) = keys.len().checked_sub(2).map(|at| &keys[at]) else {
            return false;
        };
        (VALUE_KEYS.contains(&last.as_str()) && self.labels.contains(parent))
            || (SUFFIX_KEYS.contains(&last.as_str())
                && self.labels.contains(&format!("{parent}{last}")))
    }

    fn candidate(
        &self,
        span: Range<usize>,
        outcome: ProtectionOutcome,
        field: &FieldProvenance,
    ) -> Candidate {
        Candidate::new(
            span,
            self.class.clone(),
            self.id.clone(),
            self.score,
            self.priority,
            None,
            self.token_family.clone(),
            "field_labelled",
            ConflictTier::None,
            Vec::new(),
        )
        .with_outcome(outcome)
        .with_field(field.clone())
    }
}

/// Apply the shared detection normalization, then lowercase and drop `_`, `-`, `.` and
/// whitespace, so `Customer-ID`, `customerId`, `customer_id`, a fullwidth spelling and one with
/// a zero-width space all agree.
pub fn normalize_label(label: &str) -> String {
    gaze_types::normalize::normalize(label)
        .text
        .chars()
        .filter(|ch| !matches!(ch, '_' | '-' | '.') && !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_index(segment: &str) -> bool {
    !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit())
}

/// A complete identifier: at least one digit, at most 40 bytes and four alphanumeric groups.
fn id_shaped(value: &str) -> bool {
    value.chars().any(|ch| ch.is_ascii_digit())
        && value.len() <= MAX_VALUE_BYTES
        && value
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|group| !group.is_empty())
            .count()
            <= MAX_VALUE_GROUPS
}

/// What carries a located value in the detection input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Carrier<'a> {
    /// Text that tolerates a quoted token: a JSON string interior or plain text.
    /// The range is the enclosing JSON string interior, when there is one.
    Text(Option<&'a Range<usize>>),
    /// A JSON number literal: a token would change its type.
    Number,
    /// A boolean or null literal: no identifier bytes.
    Literal,
}

fn carrier<'a>(
    range: &'a Range<usize>,
    field: &FieldProvenance,
    spans: &'a [(Range<usize>, FieldProvenance)],
) -> Carrier<'a> {
    // A validated parse locates the carrier itself; a caller declaration only locates the
    // value, so the parse decides what carries it.
    let parsed = spans.iter().find(|(parsed, provenance)| {
        provenance.evidence == FieldEvidence::ValidatedParse
            && parsed.start <= range.start
            && range.end <= parsed.end
    });
    let (parsed, json_type) = match (field.evidence, parsed) {
        (FieldEvidence::ValidatedParse, _) => (Some(range), field.json_type),
        (_, Some((parsed, provenance))) => (Some(parsed), provenance.json_type),
        (_, None) => (None, JsonScalarType::String),
    };
    match json_type {
        JsonScalarType::Number => Carrier::Number,
        JsonScalarType::Bool | JsonScalarType::Null => Carrier::Literal,
        _ => Carrier::Text(parsed),
    }
}

/// The value as written, decoding JSON escapes when it sits inside a JSON string.
fn decoded(input: &str, range: &Range<usize>, in_json_string: bool) -> Option<String> {
    let raw = input.get(range.clone())?;
    if !in_json_string || !raw.contains('\\') {
        return Some(raw.to_owned());
    }
    serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()
}

impl FieldLabelRecognizer {
    fn outcome(
        &self,
        input: &str,
        range: &Range<usize>,
        field: &FieldProvenance,
        carrier: Carrier<'_>,
        complete: bool,
    ) -> Option<ProtectionOutcome> {
        if !self.binds(&field.path.segments()) {
            return None;
        }
        let proven = matches!(
            field.evidence,
            FieldEvidence::ValidatedParse | FieldEvidence::CallerDeclared
        );
        match carrier {
            Carrier::Literal => None,
            Carrier::Number if proven => Some(ProtectionOutcome::ConfirmedUntransformable(
                UntransformableReason::JsonNumberNoTextOnlyPath,
            )),
            _ if !proven => Some(ProtectionOutcome::PossiblySensitive(
                PossiblySensitiveReason::LabelBindingUnproven,
            )),
            Carrier::Number => None,
            Carrier::Text(json_string) => {
                // A parse failure never proves a value harmless: unclear, so still sensitive.
                // A fragment never proves a complete identifier, whatever its shape.
                let shaped = complete
                    && decoded(input, range, json_string.is_some())
                        .is_some_and(|value| id_shaped(&value));
                Some(if shaped {
                    ProtectionOutcome::Protected
                } else {
                    ProtectionOutcome::PossiblySensitive(
                        PossiblySensitiveReason::LabelValueShapeUnclear,
                    )
                })
            }
        }
    }
}

fn strength(outcome: &Option<ProtectionOutcome>) -> u8 {
    match outcome {
        Some(ProtectionOutcome::ConfirmedUntransformable(_)) => 3,
        Some(ProtectionOutcome::Protected) => 2,
        Some(ProtectionOutcome::PossiblySensitive(_)) => 1,
        _ => 0,
    }
}

impl Recognizer for FieldLabelRecognizer {
    fn evidence(&self) -> gaze_types::EvidenceKind {
        gaze_types::EvidenceKind::Rule
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn supported_class(&self) -> &PiiClass {
        &self.class
    }

    fn detect(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        let mut candidates: Vec<Candidate> = Vec::new();
        match ctx.fields {
            None => {}
            Some(FieldContext::Fragment { field, .. }) => {
                // Owned tokens split this labelled leaf. The gap's identifier bytes, trimmed of
                // separators, stay sensitive; the owned tokens keep their own authority.
                let start = input
                    .find(|ch: char| ch.is_alphanumeric())
                    .unwrap_or(input.len());
                let end = input
                    .rfind(|ch: char| ch.is_alphanumeric())
                    .map_or(start, |at| {
                        at + input[at..].chars().next().map_or(0, char::len_utf8)
                    });
                let range = start..end;
                let carrier = match field.json_type {
                    JsonScalarType::Number => Carrier::Number,
                    JsonScalarType::Bool | JsonScalarType::Null => Carrier::Literal,
                    _ => Carrier::Text(None),
                };
                if !range.is_empty() {
                    if let Some(outcome) = self.outcome(input, &range, field, carrier, false) {
                        candidates.push(self.candidate(range, outcome, field));
                    }
                }
            }
            Some(FieldContext::WholeInput(field)) => {
                let range = 0..input.len();
                let carrier = match field.json_type {
                    JsonScalarType::Number => Carrier::Number,
                    JsonScalarType::Bool | JsonScalarType::Null => Carrier::Literal,
                    // The whole input is the decoded leaf, never JSON source spelling.
                    _ => Carrier::Text(None),
                };
                if !range.is_empty() {
                    if let Some(outcome) = self.outcome(input, &range, field, carrier, true) {
                        candidates.push(self.candidate(range, outcome, field));
                    }
                }
            }
            Some(FieldContext::Spans(spans)) => {
                for (range, field) in spans {
                    if range.is_empty() || input.get(range.clone()).is_none() {
                        continue;
                    }
                    let carrier = carrier(range, field, spans);
                    let Some(outcome) = self.outcome(input, range, field, carrier, true) else {
                        continue;
                    };
                    // A caller declaration and the parse can locate the same bytes: keep the
                    // strictest outcome once.
                    if let Some(existing) = candidates.iter_mut().find(|c| c.span == *range) {
                        if strength(&Some(outcome)) > strength(&existing.outcome) {
                            *existing = self.candidate(range.clone(), outcome, field);
                        }
                        continue;
                    }
                    candidates.push(self.candidate(range.clone(), outcome, field));
                }
            }
            Some(_) => {}
        }
        candidates.sort_by_key(|candidate| (candidate.span.start, candidate.span.end));
        Ok(candidates)
    }

    fn token_family(&self) -> &str {
        &self.token_family
    }

    fn locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn locale_basis(&self) -> LocaleBasis {
        self.locale_basis
    }

    fn detect_is_locale_invariant(&self) -> bool {
        true
    }

    fn binds_structured_fields(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
