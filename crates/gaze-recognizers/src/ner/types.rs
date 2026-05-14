use std::collections::BTreeMap;
use std::path::PathBuf;

use gaze_types::PiiClass;

use super::error::NerLoadError;

/// Relative file names that must be present in a model directory.
pub const MODEL_FILE: &str = "model.onnx";
pub const TOKENIZER_FILE: &str = "tokenizer.json";
pub const CONFIG_FILE: &str = "config.json";
pub const LABELS_FILE: &str = "labels.json";
pub const CHECKSUMS_FILE: &str = "SHA256SUMS";

/// Labels file format.
///
/// Accepts two equivalent shapes so adopters aren't silently blocked by a
/// keying convention mismatch:
///
/// - **Bare entity keys** (preferred, short): `{ "PER": "Name", "LOC": "Location" }`.
/// - **BIO-prefixed keys** (mirrors CoNLL / HuggingFace `id2label` shape):
///   `{ "B-PER": "Name", "I-PER": "Name", "B-LOC": "Location", "I-LOC": "Location" }`.
///
/// Values are matched against `PiiClass` variants by lowercase name, falling
/// back to `PiiClass::custom(value)` if no built-in matches. The sentinel
/// value `"drop"` (or `"ignore"`, `""`) removes the entry entirely so the
/// detector silently skips that label.
///
/// Lookup (`resolve`) tries the full BIO tag first, then the bare entity
/// type. Mixing both key shapes in a single file is allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelMap(pub BTreeMap<String, PiiClass>);

impl LabelMap {
    pub fn get(&self, conll_label: &str) -> Option<&PiiClass> {
        self.0.get(conll_label)
    }

    /// Resolve a CoNLL subword tag (e.g. `"B-PER"`, `"I-LOC"`, `"PER"`) to a
    /// `PiiClass`. Accepts both BIO-prefixed labels.json entries and bare
    /// entity-type entries; tries the full tag first, then the stripped
    /// entity.
    pub fn resolve(&self, tag: &str, entity: &str) -> Option<&PiiClass> {
        self.0.get(tag).or_else(|| self.0.get(entity))
    }

    /// Number of retained mappings (after `"drop"`/`"ignore"` sentinels are
    /// filtered out by the parser). Used by the NER bootstrap `tracing::info!`
    /// so adopters can tell at a glance whether their labels.json is empty
    /// or misaligned.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate the retained label keys. Used at load time to warn when
    /// labels.json has zero overlap with the model's `id2label` vocab — a
    /// silent-no-op symptom adopters otherwise only catch via missing
    /// detections at runtime.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NerOptions {
    pub locale: Option<String>,
    pub threshold: f32,
}

impl Default for NerOptions {
    fn default() -> Self {
        Self {
            locale: None,
            threshold: 0.3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NerSpanResult {
    pub span: std::ops::Range<usize>,
    pub class: PiiClass,
    pub score: f32,
}

/// Driver-style enum for NER backends. Backends are swappable under a common
/// `NerBackend` trait; each owns its own model-specific state. Multiple
/// `NerDetector` instances (e.g. a BERT token-classifier plus a GLiNER
/// zero-shot model) can be stacked in the same `Pipeline` — span-conflict
/// resolution picks winners across all detectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NerBackendKind {
    /// Standard BERT-family token classifier: fixed label vocabulary, BIO/IOB2
    /// subword tagging, merged via `merge_bio_spans`. Driven by ONNX Runtime.
    Ort,
    /// GLiNER-family zero-shot / open-schema extractor: entity type strings
    /// passed at inference, output is a span-score matrix. Shape reserved;
    /// backend implementation lands when GLiNER artifacts are pinned.
    Gliner,
}

impl NerBackendKind {
    pub(crate) fn parse(raw: Option<&str>) -> Result<Self, NerLoadError> {
        match raw.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Ort),
            Some("ort") | Some("onnxruntime") | Some("bert-ort") => Ok(Self::Ort),
            Some("gliner") | Some("gliner-ort") => Ok(Self::Gliner),
            Some(other) => Err(NerLoadError::UnsupportedBackend {
                backend: other.to_string(),
            }),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ort => "ort",
            Self::Gliner => "gliner",
        }
    }
}

/// Verified artifact handles. Produced by `verify_artifacts`, consumed by
/// `NerDetector::load`. Split out so the load contract can be exercised by
/// unit tests without initializing a backend runtime.
#[derive(Debug, Clone)]
pub struct VerifiedArtifacts {
    pub model_dir: PathBuf,
    pub backend_kind: NerBackendKind,
    pub recognizer_model_id: String,
    pub recognizer_model_version: String,
    pub labels: LabelMap,
    pub id2label: Vec<String>,
}
