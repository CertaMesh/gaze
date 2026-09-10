//! Synthetic Gaze-owned UTF-16 flat post-decoding contract, not a vendor API.
//! These input/batch/metadata caps are test-only, not reusable corpus limits.
//! Trusted callbacks terminate: no hard timeout, producer allocation bound,
//! pre-filter completeness, same-input inference proof, or telemetry guarantee.
//! No production API, model, normalization, token lookup, or retained text here.

use gaze_types::*;
use std::{fmt, ops::Range};

pub const INPUT_LIMIT: usize = 65_536;
pub const BATCH_LIMIT: usize = 4_096;
pub const LABEL_LIMIT: usize = 64;
pub const ORIGIN_LIMIT: usize = 32;
const ID: &str = "probe-span-v1";
const NET_ID: &str = "probe-span-v1/net";

// Deliberately no Debug: unchecked metadata can contain source text.
#[derive(Clone)]
pub struct RawSpanV1 {
    pub start_utf16: i64,
    pub end_utf16: i64,
    pub label: String,
    pub score: f64,
    pub origin: String,
}
pub type RawBatchV1 = Vec<RawSpanV1>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeError {
    InputTooLarge,
    BatchTooLarge,
    MetadataTooLarge,
    InvalidBounds,
    SplitSurrogate,
    InvalidScore,
    UnknownLabel,
    UnknownOrigin,
    Overlap,
    BackendFailure,
}
impl ProbeError {
    fn code(self) -> &'static str {
        match self {
            Self::InputTooLarge => "input_too_large",
            Self::BatchTooLarge => "batch_too_large",
            Self::MetadataTooLarge => "metadata_too_large",
            Self::InvalidBounds => "invalid_bounds",
            Self::SplitSurrogate => "split_surrogate",
            Self::InvalidScore => "invalid_score",
            Self::UnknownLabel => "unknown_label",
            Self::UnknownOrigin => "unknown_origin",
            Self::Overlap => "overlap",
            Self::BackendFailure => "backend_failure",
        }
    }
}
impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    Person,
    Organization,
    StreetAddress,
    HouseNumber,
    Email,
}
impl Label {
    fn parse(value: &str) -> Result<Self, ProbeError> {
        match value {
            "PERSON" => Ok(Self::Person),
            "ORG" => Ok(Self::Organization),
            "STREET_ADDRESS" => Ok(Self::StreetAddress),
            "HOUSE_NUMBER" => Ok(Self::HouseNumber),
            "EMAIL" => Ok(Self::Email),
            _ => Err(ProbeError::UnknownLabel),
        }
    }
    pub fn class(self) -> PiiClass {
        match self {
            Self::Person => PiiClass::Name,
            Self::Organization => PiiClass::Custom("organization".into()),
            Self::StreetAddress => PiiClass::Location,
            Self::HouseNumber => PiiClass::Custom("house_number".into()),
            Self::Email => PiiClass::Email,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Person => "PERSON",
            Self::Organization => "ORG",
            Self::StreetAddress => "STREET_ADDRESS",
            Self::HouseNumber => "HOUSE_NUMBER",
            Self::Email => "EMAIL",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Neural,
    Rule,
    Context,
}
impl Origin {
    fn parse(value: &str) -> Result<Self, ProbeError> {
        match value {
            "neural" => Ok(Self::Neural),
            "rule" => Ok(Self::Rule),
            "context" => Ok(Self::Context),
            _ => Err(ProbeError::UnknownOrigin),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Neural => "neural",
            Self::Rule => "rule",
            Self::Context => "context",
        }
    }
}
#[derive(Debug, PartialEq)]
pub struct ValidatedSpan {
    pub span: Range<usize>,
    pub label: Label,
    pub score: f64,
    pub origin: Origin,
}
impl ValidatedSpan {
    fn provenance(&self) -> String {
        format!("{ID}/{}/{}", self.origin.name(), self.label.name())
    }
}

pub fn validate(input: &str, batch: RawBatchV1) -> Result<Vec<ValidatedSpan>, ProbeError> {
    if input.len() > INPUT_LIMIT {
        return Err(ProbeError::InputTooLarge);
    }
    if batch.len() > BATCH_LIMIT {
        return Err(ProbeError::BatchTooLarge);
    }
    let mut boundaries = Vec::with_capacity(input.len() + 1);
    for (byte, scalar) in input.char_indices() {
        boundaries.push(Some(byte));
        if scalar.len_utf16() == 2 {
            boundaries.push(None);
        }
    }
    boundaries.push(Some(input.len()));
    let mut validated = Vec::with_capacity(batch.len());
    for raw in batch {
        if raw.label.len() > LABEL_LIMIT || raw.origin.len() > ORIGIN_LIMIT {
            return Err(ProbeError::MetadataTooLarge);
        }
        let start = usize::try_from(raw.start_utf16).map_err(|_| ProbeError::InvalidBounds)?;
        let end = usize::try_from(raw.end_utf16).map_err(|_| ProbeError::InvalidBounds)?;
        if start >= end || end >= boundaries.len() {
            return Err(ProbeError::InvalidBounds);
        }
        if !raw.score.is_finite() || !(0.0..=1.0).contains(&raw.score) {
            return Err(ProbeError::InvalidScore);
        }
        let label = Label::parse(&raw.label)?;
        let origin = Origin::parse(&raw.origin)?;
        let start = boundaries[start].ok_or(ProbeError::SplitSurrogate)?;
        let end = boundaries[end].ok_or(ProbeError::SplitSurrogate)?;
        validated.push(ValidatedSpan {
            span: start..end,
            label,
            score: raw.score,
            origin,
        });
    }
    validated.sort_by_key(|item| (item.span.start, item.span.end));
    // Flat decoded batches reject ALL overlap, including nested address labels.
    if validated
        .windows(2)
        .any(|pair| pair[0].span.end > pair[1].span.start)
    {
        return Err(ProbeError::Overlap);
    }
    Ok(validated)
}

type Callback = dyn Fn(&str) -> Result<RawBatchV1, ProbeError> + Send + Sync;
pub struct Backend(Box<Callback>);
impl Backend {
    pub fn new(f: impl Fn(&str) -> Result<RawBatchV1, ProbeError> + Send + Sync + 'static) -> Self {
        Self(Box::new(f))
    }
    fn scan(&self, input: &str) -> Result<Vec<ValidatedSpan>, ProbeError> {
        if input.len() > INPUT_LIMIT {
            return Err(ProbeError::InputTooLarge);
        }
        validate(input, (self.0)(input)?)
    }
}

pub struct ProbeRecognizer {
    backend: Backend,
    threshold: f64,
}
impl ProbeRecognizer {
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            threshold: 0.5,
        }
    }
}
impl Recognizer for ProbeRecognizer {
    fn id(&self) -> &str {
        ID
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(&self, input: &str, _ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        let spans = self
            .backend
            .scan(input)
            .map_err(|error| DetectError::backend(ID, error.code()))?;
        Ok(spans
            .into_iter()
            .filter(|item| item.label == Label::Person && item.score >= self.threshold)
            .map(|item| {
                let source = item.provenance();
                let mut candidate = Candidate::new(
                    item.span,
                    item.label.class(),
                    ID,
                    item.score as f32,
                    0,
                    None,
                    "counter",
                    source,
                    ConflictTier::None,
                    vec![],
                );
                candidate.recognizer_version_id = Some(ID.into());
                candidate
            })
            .collect())
    }
}

pub struct ProbeSafetyNet(pub Backend);
impl SafetyNet for ProbeSafetyNet {
    fn id(&self) -> &str {
        NET_ID
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _ctx: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let spans = self.0.scan(text).map_err(|error| match error {
            ProbeError::InputTooLarge => SafetyNetError::InputTooLarge {
                limit: INPUT_LIMIT,
                actual: text.len(),
            },
            ProbeError::BackendFailure => SafetyNetError::Runtime {
                message: error.code().into(),
            },
            _ => SafetyNetError::InvalidOutput {
                message: error.code().into(),
            },
        })?;
        Ok(spans
            .into_iter()
            .map(|item| {
                let source = item.provenance();
                LeakSuspect::new(
                    item.span,
                    item.label.class(),
                    NET_ID,
                    Some(item.score as f32),
                    LeakKind::Uncovered,
                    source,
                    None,
                )
            })
            .collect())
    }
}
