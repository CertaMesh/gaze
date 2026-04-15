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
use std::sync::Mutex;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::detector::{Detection, Detector, PiiClass};

/// Relative file names that must be present in a model directory.
pub const MODEL_FILE: &str = "model.onnx";
pub const TOKENIZER_FILE: &str = "tokenizer.json";
pub const CONFIG_FILE: &str = "config.json";
pub const LABELS_FILE: &str = "labels.json";
pub const CHECKSUMS_FILE: &str = "SHA256SUMS";

/// Labels file format: `{ "PER": "Name", "LOC": "Location", ... }`.
/// Values are matched against `PiiClass` variants by lowercase name,
/// falling back to `PiiClass::custom(value)` if no built-in matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelMap(pub BTreeMap<String, PiiClass>);

impl LabelMap {
    pub fn get(&self, conll_label: &str) -> Option<&PiiClass> {
        self.0.get(conll_label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NerBackendKind {
    Ort,
    Redact,
}

impl NerBackendKind {
    fn parse(raw: Option<&str>) -> Result<Self, NerLoadError> {
        match raw.map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(Self::Ort),
            Some("ort") | Some("onnxruntime") => Ok(Self::Ort),
            Some("redact") => Ok(Self::Redact),
            Some(other) => Err(NerLoadError::UnsupportedBackend {
                backend: other.to_string(),
            }),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ort => "ort",
            Self::Redact => "redact",
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

trait NerBackend: Send + Sync {
    fn detect(
        &self,
        input: &str,
        labels: &LabelMap,
        id2label: &[String],
        source: &str,
    ) -> Result<Vec<Detection>, NerRuntimeError>;
}

/// NER detector backed by a pinned local model artifact set.
pub struct NerDetector {
    #[allow(dead_code)]
    model_dir: PathBuf,
    #[allow(dead_code)]
    backend_kind: NerBackendKind,
    labels: LabelMap,
    id2label: Vec<String>,
    backend: Box<dyn NerBackend>,
}

impl fmt::Debug for NerDetector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NerDetector")
            .field("model_dir", &self.model_dir)
            .field("backend_kind", &self.backend_kind)
            .finish_non_exhaustive()
    }
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
        let verified = Self::verify_artifacts(model_dir)?;
        let backend = load_backend(&verified)?;

        Ok(Self {
            model_dir: verified.model_dir,
            backend_kind: verified.backend_kind,
            labels: verified.labels,
            id2label: verified.id2label,
            backend,
        })
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
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < subword_labels.len() {
            let tag = subword_labels[i];
            let (prefix, entity) = split_bio(tag);
            if prefix == 'O' || entity.is_empty() {
                i += 1;
                continue;
            }
            let Some(class) = labels.get(entity) else {
                i += 1;
                continue;
            };
            let (start, mut end) = subword_spans[i];
            if start == end {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < subword_labels.len() {
                let (p2, e2) = split_bio(subword_labels[j]);
                if p2 == 'I' && e2 == entity {
                    let (s, e) = subword_spans[j];
                    if s != e {
                        end = e;
                    }
                    j += 1;
                } else {
                    break;
                }
            }
            out.push(Detection {
                span: start..end,
                class: class.clone(),
                source: source.to_string(),
            });
            i = j;
        }
        out
    }
}

impl Detector for NerDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        let source = format!("ner/{}", self.backend_kind.as_str());
        match self.backend.detect(input, &self.labels, &self.id2label, &source) {
            Ok(detections) => detections,
            Err(err) => {
                tracing::warn!(backend = self.backend_kind.as_str(), error = %err, "ner: backend detect failed");
                Vec::new()
            }
        }
    }
}

struct OrtBackend {
    tokenizer: tokenizers::Tokenizer,
    session: Mutex<ort::session::Session>,
}

impl OrtBackend {
    fn load(model_dir: &Path) -> Result<Self, NerLoadError> {
        let tokenizer = tokenizers::Tokenizer::from_file(model_dir.join(TOKENIZER_FILE))
            .map_err(|err| NerLoadError::Tokenizer(err.to_string()))?;
        let session = ort::session::Session::builder()
            .map_err(|err| NerLoadError::Runtime(err.to_string()))?
            .commit_from_file(model_dir.join(MODEL_FILE))
            .map_err(|err| NerLoadError::Runtime(err.to_string()))?;
        Ok(Self {
            tokenizer,
            session: Mutex::new(session),
        })
    }
}

impl NerBackend for OrtBackend {
    fn detect(
        &self,
        input: &str,
        labels: &LabelMap,
        id2label: &[String],
        source: &str,
    ) -> Result<Vec<Detection>, NerRuntimeError> {
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
        }

        Ok(NerDetector::merge_bio_spans(
            labels,
            offsets,
            &subword_labels,
            source,
        )
        .into_iter()
        .filter(|detection| detection.span.end <= input.len())
        .collect())
    }
}

fn load_backend(verified: &VerifiedArtifacts) -> Result<Box<dyn NerBackend>, NerLoadError> {
    match verified.backend_kind {
        NerBackendKind::Ort => Ok(Box::new(OrtBackend::load(&verified.model_dir)?)),
        NerBackendKind::Redact => Err(NerLoadError::UnsupportedBackend {
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
            br#"{"backend":"redact","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let verified = NerDetector::verify_artifacts(dir.path()).expect("verify");
        assert_eq!(verified.backend_kind, NerBackendKind::Redact);
    }

    #[test]
    fn load_fails_closed_for_unsupported_backend() {
        let dir = setup_dir_with_config(
            br#"{"backend":"redact","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let err = NerDetector::load(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::UnsupportedBackend { .. }),
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
}
