//! Transformer-based NER detector with pluggable backends.
//!
//! Model artifacts live out-of-repo at `${XDG_DATA_HOME:-~/.local/share}/gaze/models/davlan-mbert-ner-hrl/`
//! by default, or at an operator-supplied `[ner] model_dir` in `policy.toml`.
//!
//! Load contract:
//! - `SHA256SUMS` lists every required artifact by relative path. Every listed
//!   file is hashed at load time and compared to the pinned hash. Mismatch or
//!   missing file fails closed.
//! - `labels.json` maps CoNLL-style model label strings (e.g. `B-PER`) to
//!   Gaze `PiiClass` values. Labels absent from the map are dropped.
//! - `config.json` provides `id2label` so we can translate model output logits
//!   back to label strings. It may also specify the `backend` driver; omitted
//!   backends default to `ort`.
//! - `tokenizer.json` is consumed by HuggingFace `tokenizers` with
//!   byte-offset reconstruction enabled.
//!
//! Tokenizer offsets are produced against the input string supplied to
//! `Detector::detect`, which is the NFC-normalized pipeline pre-pass output.
//! Spans are merged across adjacent same-entity subwords using BIO/IOB2 rules
//! and emitted as byte `Range<usize>` against that pre-pass string.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use gaze::{Candidate, ConflictTier, DetectContext, Detection, Detector, PiiClass, Recognizer};

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
    fn parse(raw: Option<&str>) -> Result<Self, NerLoadError> {
        match raw.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Ort),
            Some("ort") | Some("onnxruntime") | Some("bert-ort") => Ok(Self::Ort),
            Some("gliner") | Some("gliner-ort") => Ok(Self::Gliner),
            Some(other) => Err(NerLoadError::UnsupportedBackend {
                backend: other.to_string(),
            }),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ort => "ort",
            Self::Gliner => "gliner",
        }
    }
}

#[derive(Debug, Error)]
pub enum NerLoadError {
    #[error("model directory not found: {path}")]
    ModelDirMissing { path: PathBuf },
    #[error("SHA256SUMS not found at {path}")]
    ChecksumsMissing { path: PathBuf },
    #[error("SHA256SUMS malformed at line {line}: {reason}")]
    ChecksumsMalformed { line: usize, reason: String },
    #[error("required artifact missing: {path}")]
    MissingArtifact { path: PathBuf },
    #[error("checksum mismatch for {path}: expected {expected}, got {got}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        got: String,
    },
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("labels.json parse error: {0}")]
    LabelsParse(String),
    #[error("config.json parse error: {0}")]
    ConfigParse(String),
    #[error("unsupported ner backend: {backend}")]
    UnsupportedBackend { backend: String },
    #[error("tokenizer load error: {0}")]
    Tokenizer(String),
    #[error("onnx runtime load error: {0}")]
    Runtime(String),
}

#[derive(Debug, Error)]
enum NerRuntimeError {
    #[error("tokenizer encode error: {0}")]
    Tokenizer(String),
    #[error("input tensor build error: {0}")]
    InputTensor(String),
    #[error("session mutex poisoned: {0}")]
    Poisoned(String),
    #[error("inference failed: {0}")]
    Inference(String),
    #[error("logits extract failed: {0}")]
    Output(String),
}

/// Driver contract: produce byte-offset detections against a pre-normalized
/// input string. Each backend owns its own model-specific state (label map,
/// id2label for BERT, entity-type list for GLiNER, etc.) — the trait stays
/// shape-agnostic so new backends plug in without changing `NerDetector`.
trait NerBackend: Send + Sync {
    fn detect(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError>;
}

/// NER detector backed by a pinned local model artifact set. Multiple
/// `NerDetector` instances with different backends may be stacked in the
/// same `Pipeline`; span-conflict resolution picks winners across detectors.
pub struct NerDetector {
    #[allow(dead_code)]
    model_dir: PathBuf,
    backend_kind: NerBackendKind,
    locale: Option<String>,
    threshold: f32,
    backend: Arc<dyn NerBackend>,
}

impl fmt::Debug for NerDetector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NerDetector")
            .field("model_dir", &self.model_dir)
            .field("backend_kind", &self.backend_kind)
            .field("locale", &self.locale)
            .field("threshold", &self.threshold)
            .finish_non_exhaustive()
    }
}

pub struct NerRecognizer {
    detector: NerDetector,
}

/// Verified artifact handles. Produced by `verify_artifacts`, consumed by
/// `NerDetector::load`. Split out so the load contract can be exercised by
/// unit tests without initializing a backend runtime.
#[derive(Debug, Clone)]
pub struct VerifiedArtifacts {
    pub model_dir: PathBuf,
    pub backend_kind: NerBackendKind,
    pub labels: LabelMap,
    pub id2label: Vec<String>,
}

impl NerDetector {
    /// Verify SHA256SUMS, parse labels.json and config.json. No backend
    /// initialization. Used both by `load` and by unit tests.
    pub fn verify_artifacts(model_dir: &Path) -> Result<VerifiedArtifacts, NerLoadError> {
        if !model_dir.is_dir() {
            return Err(NerLoadError::ModelDirMissing {
                path: model_dir.to_path_buf(),
            });
        }

        let sums_path = model_dir.join(CHECKSUMS_FILE);
        if !sums_path.exists() {
            return Err(NerLoadError::ChecksumsMissing { path: sums_path });
        }

        let entries = parse_checksums(&sums_path)?;
        for required in [MODEL_FILE, TOKENIZER_FILE, CONFIG_FILE, LABELS_FILE] {
            if !entries.iter().any(|(name, _)| name == required) {
                return Err(NerLoadError::MissingArtifact {
                    path: model_dir.join(required),
                });
            }
        }

        for (rel, expected) in &entries {
            let path = model_dir.join(rel);
            if !path.exists() {
                return Err(NerLoadError::MissingArtifact { path });
            }
            let got = hash_file(&path)?;
            if !got.eq_ignore_ascii_case(expected) {
                return Err(NerLoadError::ChecksumMismatch {
                    path,
                    expected: expected.clone(),
                    got,
                });
            }
        }

        let labels = parse_labels(&model_dir.join(LABELS_FILE))?;
        let config = parse_config(&model_dir.join(CONFIG_FILE))?;
        let backend_kind = NerBackendKind::parse(config.backend.as_deref())?;
        let id2label = config_to_id2label(config.id2label)?;

        Ok(VerifiedArtifacts {
            model_dir: model_dir.to_path_buf(),
            backend_kind,
            labels,
            id2label,
        })
    }

    /// Full load: verify artifacts, initialize the configured backend.
    /// Fails closed on any load error.
    pub fn load(model_dir: &Path) -> Result<Self, NerLoadError> {
        Self::load_with_options(model_dir, NerOptions::default())
    }

    pub fn load_with_options(model_dir: &Path, options: NerOptions) -> Result<Self, NerLoadError> {
        let verified = Self::verify_artifacts(model_dir)?;
        let backend_kind = verified.backend_kind;
        let model_dir_path = verified.model_dir.clone();
        let label_count = verified.labels.len();
        let id2label_len = verified.id2label.len();
        warn_on_label_vocab_mismatch(&verified.labels, &verified.id2label, model_dir);
        let backend = load_backend(verified)?;

        tracing::info!(
            backend = backend_kind.as_str(),
            labels = label_count,
            id2label_size = id2label_len,
            locale = options.locale.as_deref().unwrap_or(""),
            threshold = options.threshold,
            model_dir = %model_dir_path.display(),
            "ner: detector registered"
        );

        Ok(Self {
            model_dir: model_dir_path,
            backend_kind,
            locale: options.locale,
            threshold: options.threshold,
            backend,
        })
    }

    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    pub fn backend_kind(&self) -> NerBackendKind {
        self.backend_kind
    }

    /// Label/offset reconstruction helper. Public for testing the BIO merge.
    /// `subword_spans` are byte ranges against the tokenizer input string,
    /// `subword_labels` are CoNLL-style labels per subword (e.g. `O`, `B-PER`,
    /// `I-PER`). Returns merged detections, dropping labels absent from the
    /// label map and subword spans overlapping special tokens (empty ranges).
    pub fn merge_bio_spans(
        labels: &LabelMap,
        subword_spans: &[(usize, usize)],
        subword_labels: &[&str],
        source: &str,
    ) -> Vec<Detection> {
        let scores = vec![1.0; subword_labels.len()];
        Self::merge_bio_span_results(labels, subword_spans, subword_labels, &scores, source)
            .into_iter()
            .map(|span| Detection {
                span: span.span,
                class: span.class,
                source: source.to_string(),
            })
            .collect()
    }

    pub fn merge_bio_span_results(
        labels: &LabelMap,
        subword_spans: &[(usize, usize)],
        subword_labels: &[&str],
        subword_scores: &[f32],
        _source: &str,
    ) -> Vec<NerSpanResult> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < subword_labels.len() {
            let tag = subword_labels[i];
            let (prefix, entity) = split_bio(tag);
            if prefix == 'O' || entity.is_empty() {
                i += 1;
                continue;
            }
            let Some(class) = labels.resolve(tag, entity) else {
                i += 1;
                continue;
            };
            let (start, mut end) = subword_spans[i];
            if start == end {
                i += 1;
                continue;
            }
            let mut span_score = *subword_scores.get(i).unwrap_or(&0.0);
            let mut j = i + 1;
            while j < subword_labels.len() {
                let (p2, e2) = split_bio(subword_labels[j]);
                if p2 == 'I' && e2 == entity {
                    let (s, e) = subword_spans[j];
                    if s != e {
                        end = e;
                        span_score = span_score.min(*subword_scores.get(j).unwrap_or(&0.0));
                    }
                    j += 1;
                } else {
                    break;
                }
            }
            out.push(NerSpanResult {
                span: start..end,
                class: class.clone(),
                score: span_score,
            });
            i = j;
        }
        out
    }
}

impl NerRecognizer {
    pub fn load_with_options(model_dir: &Path, options: NerOptions) -> Result<Self, NerLoadError> {
        Ok(Self {
            detector: NerDetector::load_with_options(model_dir, options)?,
        })
    }
}

impl Detector for NerDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        match self.backend.detect(input) {
            Ok(detections) => detections
                .into_iter()
                .map(|span| Detection {
                    span: span.span,
                    class: span.class,
                    source: format!("ner/{}", self.backend_kind.as_str()),
                })
                .collect(),
            Err(err) => {
                tracing::warn!(backend = self.backend_kind.as_str(), error = %err, "ner: backend detect failed");
                Vec::new()
            }
        }
    }
}

impl Recognizer for NerRecognizer {
    fn id(&self) -> &str {
        "ner"
    }

    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }

    fn detect(&self, input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
        match self.detector.backend.detect(input) {
            Ok(spans) => spans
                .into_iter()
                .filter(|span| span.score >= self.detector.threshold)
                .map(|span| Candidate {
                    span: span.span,
                    class: span.class,
                    recognizer_id: self.id().to_string(),
                    score: span.score,
                    priority: 0,
                    canonical_form: None,
                    token_family: self.token_family().to_string(),
                    source: format!("ner/{}", self.detector.backend_kind.as_str()),
                    decided_by: ConflictTier::None,
                    merged_sources: Vec::new(),
                })
                .collect(),
            Err(err) => {
                tracing::warn!(backend = self.detector.backend_kind.as_str(), error = %err, "ner: backend detect failed");
                Vec::new()
            }
        }
    }

    fn token_family(&self) -> &str {
        "counter"
    }
}

/// BERT-family token-classification backend. Owns its tokenizer, ONNX session,
/// label map, `id2label` vocab, and pre-computed source tag. BIO/IOB2 subword
/// tags are merged via `NerDetector::merge_bio_spans`.
struct OrtBackend {
    tokenizer: tokenizers::Tokenizer,
    session: Mutex<ort::session::Session>,
    labels: LabelMap,
    id2label: Vec<String>,
    source: String,
}

impl OrtBackend {
    fn load(model_dir: &Path, labels: LabelMap, id2label: Vec<String>) -> Result<Self, NerLoadError> {
        let tokenizer = tokenizers::Tokenizer::from_file(model_dir.join(TOKENIZER_FILE))
            .map_err(|err| NerLoadError::Tokenizer(err.to_string()))?;
        let session = ort::session::Session::builder()
            .map_err(|err| NerLoadError::Runtime(err.to_string()))?
            .commit_from_file(model_dir.join(MODEL_FILE))
            .map_err(|err| NerLoadError::Runtime(err.to_string()))?;
        Ok(Self {
            tokenizer,
            session: Mutex::new(session),
            labels,
            id2label,
            source: format!("ner/{}", NerBackendKind::Ort.as_str()),
        })
    }
}

impl NerBackend for OrtBackend {
    fn detect(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
        let labels = &self.labels;
        let id2label: &[String] = &self.id2label;
        let source = self.source.as_str();
        let encoded = self
            .tokenizer
            .encode(input, true)
            .map_err(|err| NerRuntimeError::Tokenizer(err.to_string()))?;
        let offsets = encoded.get_offsets();
        let ids = encoded.get_ids();
        let attention = encoded.get_attention_mask();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let seq_len = ids.len();
        let input_ids: Vec<i64> = ids.iter().map(|&v| v as i64).collect();
        let attn_mask: Vec<i64> = attention.iter().map(|&v| v as i64).collect();
        let token_type: Vec<i64> = vec![0i64; seq_len];

        let shape = [1usize, seq_len];
        let input_ids_tensor = ort::value::Tensor::from_array((shape, input_ids))
            .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;
        let attn_tensor = ort::value::Tensor::from_array((shape, attn_mask))
            .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;
        let type_tensor = ort::value::Tensor::from_array((shape, token_type))
            .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;

        let inputs = ort::inputs![
            "input_ids" => input_ids_tensor,
            "attention_mask" => attn_tensor,
            "token_type_ids" => type_tensor,
        ];

        let mut session = self
            .session
            .lock()
            .map_err(|err| NerRuntimeError::Poisoned(err.to_string()))?;
        let outputs = session
            .run(inputs)
            .map_err(|err| NerRuntimeError::Inference(err.to_string()))?;

        let logits = match outputs.iter().next() {
            Some((_, value)) => value,
            None => return Ok(Vec::new()),
        };
        let (shape_obj, flat) = logits
            .try_extract_tensor::<f32>()
            .map_err(|err| NerRuntimeError::Output(err.to_string()))?;
        let shape: Vec<usize> = shape_obj.iter().map(|d| *d as usize).collect();
        if shape.len() != 3 || shape[0] != 1 || shape[1] != seq_len {
            return Ok(Vec::new());
        }

        let num_labels = shape[2];
        let mut subword_labels: Vec<&str> = Vec::with_capacity(seq_len);
        let mut subword_scores: Vec<f32> = Vec::with_capacity(seq_len);
        for pos in 0..seq_len {
            let base = pos * num_labels;
            let row = &flat[base..base + num_labels];
            let (argmax, _) =
                row.iter()
                    .enumerate()
                    .fold((0usize, f32::NEG_INFINITY), |acc, (index, &value)| {
                        if value > acc.1 {
                            (index, value)
                        } else {
                            acc
                        }
                    });
            let label = id2label.get(argmax).map(String::as_str).unwrap_or("O");
            subword_labels.push(label);
            subword_scores.push(softmax_confidence(row, argmax));
        }

        Ok(NerDetector::merge_bio_span_results(
            labels,
            offsets,
            &subword_labels,
            &subword_scores,
            source,
        )
        .into_iter()
        .filter(|span| span.span.end <= input.len())
        .collect())
    }
}

fn softmax_confidence(row: &[f32], index: usize) -> f32 {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let denom = row.iter().map(|value| (*value - max).exp()).sum::<f32>();
    if denom == 0.0 {
        return 0.0;
    }
    row.get(index)
        .map(|value| (*value - max).exp() / denom)
        .unwrap_or(0.0)
}

/// Emit a loud `tracing::warn!` at load time when `labels.json` contains no
/// keys that can resolve any entity type produced by the model's `id2label`
/// vocab. Historically this was the silent-no-op signature: load succeeds,
/// every inference runs, but every subword lookup misses so zero detections
/// are emitted. We now surface it explicitly so operators can diagnose
/// misaligned label files without reading source.
fn warn_on_label_vocab_mismatch(labels: &LabelMap, id2label: &[String], model_dir: &Path) {
    let mut usable = 0usize;
    for tag in id2label {
        let (_, entity) = split_bio(tag);
        if entity.is_empty() {
            continue;
        }
        if labels.resolve(tag, entity).is_some() {
            usable += 1;
        }
    }
    if usable == 0 {
        let sample_label: String = labels.keys().take(5).collect::<Vec<_>>().join(",");
        let sample_id: String = id2label.iter().take(5).cloned().collect::<Vec<_>>().join(",");
        tracing::warn!(
            model_dir = %model_dir.display(),
            label_keys = %sample_label,
            id2label_sample = %sample_id,
            "ner: labels.json has zero overlap with model id2label — detector will emit zero detections. Expected keys like 'PER'/'LOC' or 'B-PER'/'I-PER'."
        );
    }
}

fn load_backend(verified: VerifiedArtifacts) -> Result<Arc<dyn NerBackend>, NerLoadError> {
    match verified.backend_kind {
        NerBackendKind::Ort => Ok(Arc::new(OrtBackend::load(
            &verified.model_dir,
            verified.labels,
            verified.id2label,
        )?)),
        NerBackendKind::Gliner => Err(NerLoadError::UnsupportedBackend {
            backend: verified.backend_kind.as_str().to_string(),
        }),
    }
}

/// `B-PER` → ('B', "PER"); `O` → ('O', ""); `PER` (no prefix) → ('B', "PER").
fn split_bio(tag: &str) -> (char, &str) {
    if tag == "O" || tag.is_empty() {
        return ('O', "");
    }
    if let Some(rest) = tag.strip_prefix("B-") {
        return ('B', rest);
    }
    if let Some(rest) = tag.strip_prefix("I-") {
        return ('I', rest);
    }
    ('B', tag)
}

fn parse_checksums(path: &Path) -> Result<Vec<(String, String)>, NerLoadError> {
    let file = fs::File::open(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line_no = idx + 1;
        let line = line.map_err(|source| NerLoadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let Some(hash) = parts.next() else {
            return Err(NerLoadError::ChecksumsMalformed {
                line: line_no,
                reason: "missing hash".into(),
            });
        };
        let Some(rest) = parts.next() else {
            return Err(NerLoadError::ChecksumsMalformed {
                line: line_no,
                reason: "missing path".into(),
            });
        };
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(NerLoadError::ChecksumsMalformed {
                line: line_no,
                reason: format!("invalid sha256 hex: {hash}"),
            });
        }
        let file = rest.trim_start().trim_start_matches('*').trim().to_string();
        if file.is_empty() {
            return Err(NerLoadError::ChecksumsMalformed {
                line: line_no,
                reason: "empty path".into(),
            });
        }
        entries.push((file, hash.to_ascii_lowercase()));
    }
    Ok(entries)
}

fn hash_file(path: &Path) -> Result<String, NerLoadError> {
    let bytes = fs::read(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn parse_labels(path: &Path) -> Result<LabelMap, NerLoadError> {
    let bytes = fs::read(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let raw: BTreeMap<String, String> = serde_json::from_slice(&bytes)
        .map_err(|err| NerLoadError::LabelsParse(err.to_string()))?;
    let mut map = BTreeMap::new();
    for (key, value) in raw {
        let class = match value.to_ascii_lowercase().as_str() {
            "name" | "per" | "person" => PiiClass::Name,
            "location" | "loc" => PiiClass::Location,
            "organization" | "org" => PiiClass::Organization,
            "email" => PiiClass::Email,
            "drop" | "ignore" | "" => continue,
            other => PiiClass::custom(other),
        };
        map.insert(key, class);
    }
    if map.is_empty() {
        return Err(NerLoadError::LabelsParse(
            "labels.json produced no usable mappings".into(),
        ));
    }
    Ok(LabelMap(map))
}

#[derive(Deserialize)]
struct ConfigFile {
    backend: Option<String>,
    id2label: Option<BTreeMap<String, String>>,
}

fn parse_config(path: &Path) -> Result<ConfigFile, NerLoadError> {
    let bytes = fs::read(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|err| NerLoadError::ConfigParse(err.to_string()))
}

fn config_to_id2label(
    id2label: Option<BTreeMap<String, String>>,
) -> Result<Vec<String>, NerLoadError> {
    let map =
        id2label.ok_or_else(|| NerLoadError::ConfigParse("config.json missing id2label".to_string()))?;
    let mut pairs: Vec<(usize, String)> = map
        .into_iter()
        .map(|(key, value)| {
            key.parse::<usize>()
                .map(|index| (index, value))
                .map_err(|err| NerLoadError::ConfigParse(format!("id2label key {key}: {err}")))
        })
        .collect::<Result<_, _>>()?;
    pairs.sort_by_key(|(index, _)| *index);
    let max_idx = pairs.last().map(|(index, _)| *index).unwrap_or(0);
    let mut out = vec!["O".to_string(); max_idx + 1];
    for (index, label) in pairs {
        out[index] = label;
    }
    Ok(out)
}

/// Test-only helpers for stacking multiple `NerDetector` instances with
/// in-memory fake backends. Lets pipeline tests verify Layer-1 stackability
/// without real ONNX artifacts.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    struct FixedBackend {
        detections: Vec<NerSpanResult>,
    }

    impl NerBackend for FixedBackend {
        fn detect(&self, _input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
            Ok(self.detections.clone())
        }
    }

    /// Build a `NerDetector` that emits a fixed detection set, bypassing the
    /// SHA256-pinned artifact contract. For tests only.
    pub(crate) fn detector_with_detections(
        source: &str,
        detections: Vec<Detection>,
    ) -> NerDetector {
        let kind = match source {
            "gliner" => NerBackendKind::Gliner,
            _ => NerBackendKind::Ort,
        };
        NerDetector {
            model_dir: PathBuf::from("/test/fake"),
            backend_kind: kind,
            locale: None,
            threshold: 0.3,
            backend: Arc::new(FixedBackend {
                detections: detections
                    .into_iter()
                    .map(|detection| NerSpanResult {
                        span: detection.span,
                        class: detection.class,
                        score: 1.0,
                    })
                    .collect(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(path: &Path, content: &[u8]) {
        fs::write(path, content).unwrap();
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    fn good_labels() -> &'static [u8] {
        br#"{"PER":"Name","LOC":"Location","ORG":"Organization"}"#
    }

    fn good_config() -> &'static [u8] {
        br#"{"id2label":{"0":"O","1":"B-PER","2":"I-PER","3":"B-LOC","4":"I-LOC","5":"B-ORG","6":"I-ORG","7":"B-MISC","8":"I-MISC"}}"#
    }

    fn setup_good_dir() -> tempfile::TempDir {
        setup_dir_with_config(good_config())
    }

    fn setup_dir_with_config(config: &[u8]) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let path = dir.path();
        let model_bytes = b"fake-onnx";
        let tokenizer_bytes = b"fake-tokenizer";
        write(&path.join(MODEL_FILE), model_bytes);
        write(&path.join(TOKENIZER_FILE), tokenizer_bytes);
        write(&path.join(CONFIG_FILE), config);
        write(&path.join(LABELS_FILE), good_labels());
        let sums = format!(
            "{}  {}\n{}  {}\n{}  {}\n{}  {}\n",
            sha256_hex(model_bytes),
            MODEL_FILE,
            sha256_hex(tokenizer_bytes),
            TOKENIZER_FILE,
            sha256_hex(config),
            CONFIG_FILE,
            sha256_hex(good_labels()),
            LABELS_FILE,
        );
        write(&path.join(CHECKSUMS_FILE), sums.as_bytes());
        dir
    }

    #[test]
    fn verify_artifacts_succeeds_on_matching_checksums() {
        let dir = setup_good_dir();
        let verified = NerDetector::verify_artifacts(dir.path()).expect("verify");
        assert_eq!(verified.backend_kind, NerBackendKind::Ort);
        assert!(verified.labels.get("PER").is_some());
        assert_eq!(verified.id2label[1], "B-PER");
    }

    #[test]
    fn verify_artifacts_honors_explicit_backend_selection() {
        let dir = setup_dir_with_config(
            br#"{"backend":"gliner","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let verified = NerDetector::verify_artifacts(dir.path()).expect("verify");
        assert_eq!(verified.backend_kind, NerBackendKind::Gliner);
    }

    #[test]
    fn load_fails_closed_for_gliner_backend_until_impl_lands() {
        let dir = setup_dir_with_config(
            br#"{"backend":"gliner","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let err = NerDetector::load(dir.path()).unwrap_err();
        assert!(
            matches!(&err, NerLoadError::UnsupportedBackend { backend } if backend == "gliner"),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn load_fails_closed_for_unknown_backend() {
        let dir = setup_dir_with_config(
            br#"{"backend":"nonesuch","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let err = NerDetector::load(dir.path()).unwrap_err();
        assert!(
            matches!(&err, NerLoadError::UnsupportedBackend { backend } if backend == "nonesuch"),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn checksum_mismatch_fails_closed() {
        let dir = setup_good_dir();
        fs::write(dir.path().join(MODEL_FILE), b"tampered").unwrap();
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::ChecksumMismatch { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn missing_artifact_fails_closed() {
        let dir = setup_good_dir();
        fs::remove_file(dir.path().join(TOKENIZER_FILE)).unwrap();
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::MissingArtifact { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn missing_sums_fails_closed() {
        let dir = tempdir().unwrap();
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::ChecksumsMissing { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn missing_model_dir_fails_closed() {
        let path = PathBuf::from("/definitely/not/a/path/gaze-ner-xyz");
        let err = NerDetector::verify_artifacts(&path).unwrap_err();
        assert!(
            matches!(err, NerLoadError::ModelDirMissing { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn label_map_parse_error_fails_closed() {
        let dir = setup_good_dir();
        fs::write(dir.path().join(LABELS_FILE), b"{not-json").unwrap();
        let labels_bytes = fs::read(dir.path().join(LABELS_FILE)).unwrap();
        let model_bytes = fs::read(dir.path().join(MODEL_FILE)).unwrap();
        let tokenizer_bytes = fs::read(dir.path().join(TOKENIZER_FILE)).unwrap();
        let config_bytes = fs::read(dir.path().join(CONFIG_FILE)).unwrap();
        let sums = format!(
            "{}  {}\n{}  {}\n{}  {}\n{}  {}\n",
            sha256_hex(&model_bytes),
            MODEL_FILE,
            sha256_hex(&tokenizer_bytes),
            TOKENIZER_FILE,
            sha256_hex(&config_bytes),
            CONFIG_FILE,
            sha256_hex(&labels_bytes),
            LABELS_FILE,
        );
        fs::write(dir.path().join(CHECKSUMS_FILE), sums.as_bytes()).unwrap();
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::LabelsParse(_)),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn malformed_checksums_fail_closed() {
        let dir = tempdir().unwrap();
        write(&dir.path().join(CHECKSUMS_FILE), b"not-a-hash  model.onnx\n");
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::ChecksumsMalformed { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn merge_bio_merges_adjacent_i_tags() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 6), (7, 13)];
        let tags = vec!["B-PER", "I-PER"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..13);
        assert_eq!(out[0].class, PiiClass::Name);
    }

    #[test]
    fn merge_bio_splits_on_new_b_tag() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 3), (4, 7)];
        let tags = vec!["B-PER", "B-PER"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].span, 0..3);
        assert_eq!(out[1].span, 4..7);
    }

    #[test]
    fn merge_bio_drops_unmapped_labels() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 4)];
        let tags = vec!["B-MISC"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner");
        assert!(out.is_empty());
    }

    /// Regression: adopters who follow `labels.example.json` ship labels.json
    /// keyed by full BIO tags (`B-PER`, `I-PER`, …). Until v0.3.1 the
    /// post-process only looked up the stripped entity (`PER`), so every
    /// detection silently dropped — reported by Markus (lord-eagle) against
    /// v0.3.0 aarch64-apple-darwin. Both shapes must emit detections.
    #[test]
    fn merge_bio_accepts_bio_prefixed_label_keys() {
        let mut map = BTreeMap::new();
        map.insert("B-PER".to_string(), PiiClass::Name);
        map.insert("I-PER".to_string(), PiiClass::Name);
        map.insert("B-LOC".to_string(), PiiClass::Location);
        map.insert("I-LOC".to_string(), PiiClass::Location);
        let labels = LabelMap(map);
        let spans = vec![(0, 4), (5, 9), (10, 13), (14, 22), (23, 26), (27, 30), (31, 36), (37, 39), (40, 46)];
        let tags = vec!["O", "O", "O", "B-PER", "O", "O", "O", "O", "B-LOC"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner/ort");
        assert_eq!(out.len(), 2, "both Wolfgang + Berlin must emit: {out:?}");
        assert_eq!(out[0].span, 14..22);
        assert_eq!(out[0].class, PiiClass::Name);
        assert_eq!(out[1].span, 40..46);
        assert_eq!(out[1].class, PiiClass::Location);
    }

    /// Mixing both key shapes (bare entity + BIO-prefixed) in one labels.json
    /// must keep working — we don't want adopters editing a mixed file to
    /// discover a silent regression. BIO wins when both present.
    #[test]
    fn merge_bio_accepts_mixed_key_shapes() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        map.insert("B-LOC".to_string(), PiiClass::Location);
        map.insert("I-LOC".to_string(), PiiClass::Location);
        let labels = LabelMap(map);
        let spans = vec![(0, 4), (5, 11)];
        let tags = vec!["B-PER", "B-LOC"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner/ort");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].class, PiiClass::Name);
        assert_eq!(out[1].class, PiiClass::Location);
    }

    #[test]
    fn merge_bio_skips_special_token_empty_offsets() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 0), (0, 5)];
        let tags = vec!["B-PER", "B-PER"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..5);
    }

    #[test]
    fn merge_bio_spans_returns_min_confidence_with_one_low_token() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 4), (5, 10), (11, 16)];
        let tags = vec!["B-PER", "I-PER", "I-PER"];
        let scores = vec![0.91, 0.34, 0.88];

        let out = NerDetector::merge_bio_span_results(&labels, &spans, &tags, &scores, "ner");

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..16);
        assert_eq!(out[0].score, 0.34);
    }

    #[test]
    fn ner_recognizer_filters_below_threshold() {
        struct FixedBackend {
            spans: Vec<NerSpanResult>,
        }

        impl NerBackend for FixedBackend {
            fn detect(&self, _input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
                Ok(self.spans.clone())
            }
        }

        let recognizer = NerRecognizer {
            detector: NerDetector {
                model_dir: PathBuf::from("/test/fake"),
                backend_kind: NerBackendKind::Ort,
                locale: None,
                threshold: 0.5,
                backend: Arc::new(FixedBackend {
                    spans: vec![
                        NerSpanResult {
                            span: 0..5,
                            class: PiiClass::Name,
                            score: 0.49,
                        },
                        NerSpanResult {
                            span: 6..11,
                            class: PiiClass::Name,
                            score: 0.50,
                        },
                    ],
                }),
            },
        };
        let dictionaries = gaze::DictionaryBundle::default();
        let fields = serde_json::Map::new();
        let ctx = DetectContext {
            locale_chain: &[gaze::LocaleTag::Global],
            dictionaries: &dictionaries,
            fields: &fields,
            degraded: std::cell::Cell::new(false),
        };

        let candidates = Recognizer::detect(&recognizer, "alpha bravo", &ctx);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].span, 6..11);
        assert_eq!(candidates[0].score, 0.50);
    }
}
