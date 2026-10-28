//! Typed protection decisions and structured-field evidence.
//!
//! These contracts carry reasons and locations, never scalar input values. They do not
//! activate detection or change policy. A hint alone cannot justify protection or a
//! non-sensitive classification.

use std::{fmt, ops::Range, sync::Arc};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A protection decision, separate from whether a value can be rendered safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProtectionOutcome {
    /// A sensitive value can be substituted reversibly in its carrier.
    Protected,
    /// Independent evidence proves this position is not a sensitive value.
    NonSensitive(NonSensitiveReason),
    /// Evidence suggests sensitivity but does not prove a safe binding.
    PossiblySensitive(PossiblySensitiveReason),
    /// Sensitivity is confirmed, but no safe reversible rendering is available.
    /// Privacy-enforcing consumers must refuse this outcome.
    ConfirmedUntransformable(UntransformableReason),
}

/// Value-free outcome category stored separately from its reason in audit rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProtectionOutcomeKind {
    Protected,
    NonSensitive,
    PossiblySensitive,
    ConfirmedUntransformable,
}

/// Closed audit reason, retaining the typed reason's canonical spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ProtectionOutcomeReason {
    NonSensitive(NonSensitiveReason),
    PossiblySensitive(PossiblySensitiveReason),
    Untransformable(UntransformableReason),
}

/// Audit outcome columns do not describe one valid protection decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid protection outcome and reason pairing")]
pub struct InvalidProtectionOutcome;

impl ProtectionOutcome {
    /// Split an outcome into the two value-free audit columns.
    pub fn audit_parts(self) -> (ProtectionOutcomeKind, Option<ProtectionOutcomeReason>) {
        use ProtectionOutcomeKind as Kind;
        use ProtectionOutcomeReason as Reason;
        match self {
            Self::Protected => (Kind::Protected, None),
            Self::NonSensitive(reason) => (Kind::NonSensitive, Some(Reason::NonSensitive(reason))),
            Self::PossiblySensitive(reason) => (
                Kind::PossiblySensitive,
                Some(Reason::PossiblySensitive(reason)),
            ),
            Self::ConfirmedUntransformable(reason) => (
                Kind::ConfirmedUntransformable,
                Some(Reason::Untransformable(reason)),
            ),
        }
    }
}

impl fmt::Display for ProtectionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protected => f.write_str("protected"),
            Self::NonSensitive(reason) => write!(f, "non_sensitive: {reason}"),
            Self::PossiblySensitive(reason) => write!(f, "possibly_sensitive: {reason}"),
            Self::ConfirmedUntransformable(reason) => {
                write!(f, "confirmed_untransformable: {reason}")
            }
        }
    }
}

/// Evidence that a position is not a sensitive scalar value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NonSensitiveReason {
    /// A validated parser identifies an object key.
    #[error("json_key")]
    JsonKey,
    /// A validated parser identifies a non-sensitive JSON literal.
    #[error("json_literal")]
    JsonLiteral,
    /// The position contains a known placeholder.
    #[error("placeholder")]
    Placeholder,
}

/// Why a possible sensitive value remains uncertain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PossiblySensitiveReason {
    /// The label is known, but the value shape is unclear.
    #[error("label_value_shape_unclear")]
    LabelValueShapeUnclear,
    /// Evidence does not prove that the label binds to this value.
    #[error("label_binding_unproven")]
    LabelBindingUnproven,
    /// A value repeats in a different field without a proven binding.
    #[error("other_field_repeat")]
    OtherFieldRepeat,
}

/// Why a confirmed sensitive value cannot be substituted safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UntransformableReason {
    /// A JSON number has no verified text-only destination.
    #[error("json_number_no_text_only_path")]
    JsonNumberNoTextOnlyPath,
    /// An encoded carrier has no verified reversible transformation.
    #[error("encoded_carrier")]
    EncodedCarrier,
    /// No safe reversible rendering is available.
    #[error("no_safe_rendering")]
    NoSafeRendering,
    /// A caller-declared value is too short for safe matching.
    #[error("caller_value_too_short")]
    CallerValueTooShort,
}

impl UntransformableReason {
    /// The canonical snake-case spelling boundaries report to clients.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JsonNumberNoTextOnlyPath => "json_number_no_text_only_path",
            Self::EncodedCarrier => "encoded_carrier",
            Self::NoSafeRendering => "no_safe_rendering",
            Self::CallerValueTooShort => "caller_value_too_short",
        }
    }
}

/// JSON-pointer segments locating a field, without its scalar value.
///
/// Segments are unescaped object keys or array indices in decimal form. The empty
/// path locates the root. Serialization preserves segments for trusted consumers;
/// do not expose serialized paths at untrusted boundaries without a separate policy.
/// Keys can themselves contain PII, so Debug and Display hide every segment.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct FieldPath(Option<Arc<FieldPathNode>>);

struct FieldPathNode {
    parent: FieldPath,
    segment: String,
    depth: usize,
}

impl FieldPath {
    /// Build a path from unescaped JSON-pointer segments, never scalar values.
    pub fn new(segments: impl IntoIterator<Item = String>) -> Self {
        segments
            .into_iter()
            .fold(Self::default(), |path, segment| path.child(segment))
    }

    /// Extend a path, sharing all ancestor storage with the parent.
    pub fn child(&self, segment: String) -> Self {
        Self(Some(Arc::new(FieldPathNode {
            parent: self.clone(),
            segment,
            depth: self.len() + 1,
        })))
    }

    fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |node| node.depth)
    }

    /// Borrow unescaped segments in root-to-leaf order for trusted matching.
    /// Only references are materialized; ancestor key bytes are never copied.
    pub fn segments(&self) -> Vec<&str> {
        let mut segments = Vec::with_capacity(self.len());
        let mut current = self.0.as_deref();
        while let Some(node) = current {
            segments.push(node.segment.as_str());
            current = node.parent.0.as_deref();
        }
        segments.reverse();
        segments
    }
}

// Release an unshared tail iteratively. Caller-built paths can exceed parser depth limits.
impl Drop for FieldPath {
    fn drop(&mut self) {
        let mut tail = self.0.take();
        while let Some(node) = tail {
            match Arc::try_unwrap(node) {
                Ok(mut node) => tail = node.parent.0.take(),
                Err(_) => break,
            }
        }
    }
}

impl PartialEq for FieldPath {
    fn eq(&self, other: &Self) -> bool {
        self.segments() == other.segments()
    }
}

impl Eq for FieldPath {}

impl Serialize for FieldPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.segments())
    }
}

impl<'de> Deserialize<'de> for FieldPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<String>::deserialize(deserializer).map(Self::new)
    }
}

impl fmt::Debug for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldPath")
            .field("segment_count", &self.len())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "field path ({} segments)", self.len())
    }
}

/// The JSON type of a scalar field. Containers are not scalar fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum JsonScalarType {
    /// A JSON string.
    String,
    /// A JSON number, with its original type retained.
    Number,
    /// A JSON boolean.
    Bool,
    /// A JSON null.
    Null,
}

/// An unvalidated clue about the input's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HintKind {
    /// A code fence language label.
    FenceLang,
    /// A filename extension.
    FileExt,
    /// A declared content type.
    ContentType,
    /// A tool name.
    ToolName,
}

/// The source of structured-field context.
///
/// Caller declarations and validated parsing are the strongest evidence. A Hint alone
/// can never yield Protected or NonSensitive; it can at most suggest PossiblySensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FieldEvidence {
    /// The caller explicitly supplied structured-field context.
    CallerDeclared,
    /// A validated parse proves the field's location and JSON type.
    ValidatedParse,
    /// An unvalidated clue, insufficient to prove a field binding.
    Hint(HintKind),
}

/// A scalar's location, JSON type, and evidence. Never includes the scalar's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FieldProvenance {
    /// Location within the structured input.
    pub path: FieldPath,
    /// Original JSON scalar type.
    pub json_type: JsonScalarType,
    /// What establishes this field context.
    pub evidence: FieldEvidence,
}

impl FieldProvenance {
    /// Describe a field without copying its scalar value.
    pub fn new(path: FieldPath, json_type: JsonScalarType, evidence: FieldEvidence) -> Self {
        Self {
            path,
            json_type,
            evidence,
        }
    }
}

/// Borrowed evidence locating complete scalars or fragments inside them.
///
/// Span ranges are half-open UTF-8 byte offsets in the current detection input.
/// Producers must supply valid ranges and translate them for normalized views.
/// Fragment offsets instead refer to the original leaf, before normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldContext<'a> {
    /// The complete detection input is one scalar field.
    WholeInput(&'a FieldProvenance),
    /// The detection input is a fragment inside a scalar, starting at `offset`
    /// UTF-8 bytes in the original leaf. It cannot establish a whole-value match.
    Fragment {
        /// The original scalar's provenance.
        field: &'a FieldProvenance,
        /// Original leaf byte offset, before normalization.
        offset: usize,
    },
    /// Each range identifies a scalar field inside the detection input.
    Spans(&'a [(Range<usize>, FieldProvenance)]),
}

#[cfg(test)]
mod tests;
