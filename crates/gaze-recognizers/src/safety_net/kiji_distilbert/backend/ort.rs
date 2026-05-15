use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gaze_types::SafetyNetError;

use super::artifacts::{
    model_filename, verify_model_dir_for_precision, KIJI_DISTILBERT_BUNDLE_SHA256,
    KIJI_DISTILBERT_INT8_BUNDLE_SHA256,
};
use super::{normalize_raw_spans, KijiDistilbertBackend, KijiDistilbertPrecision, RawSpan};
use crate::ner::decode::{softmax_confidence, split_bio};

const DEFAULT_MAX_INPUT_BYTES: usize = 1024 * 1024;
const TOKENIZER_FILE: &str = "tokenizer.json";
const ID2LABEL: [&str; 9] = [
    "O", "B-PER", "I-PER", "B-LOC", "I-LOC", "B-ORG", "I-ORG", "B-MISC", "I-MISC",
];

/// Configuration for the in-process Kiji DistilBERT ONNX Runtime backend.
#[derive(Debug, Clone)]
pub struct OrtKijiConfig {
    model_dir: PathBuf,
    precision: KijiDistilbertPrecision,
    max_input_bytes: usize,
    version: String,
    decoding_params: Vec<(&'static str, String)>,
    #[cfg(any(test, feature = "test-support"))]
    expected_bundle_sha256: String,
}

impl OrtKijiConfig {
    pub fn new(model_dir: impl Into<PathBuf>) -> Self {
        Self {
            model_dir: model_dir.into(),
            precision: KijiDistilbertPrecision::Fp32,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            version: "kiji/distilbert:ort".to_string(),
            decoding_params: vec![
                ("runtime", "ort".to_string()),
                (
                    "precision",
                    KijiDistilbertPrecision::Fp32.as_str().to_string(),
                ),
            ],
            #[cfg(any(test, feature = "test-support"))]
            expected_bundle_sha256: KIJI_DISTILBERT_BUNDLE_SHA256.to_string(),
        }
    }

    pub fn from_env() -> Result<Self, SafetyNetError> {
        let model_dir = std::env::var_os("GAZE_KIJI_DISTILBERT_MODEL_DIR").ok_or_else(|| {
            SafetyNetError::Unavailable {
                reason: "GAZE_KIJI_DISTILBERT_MODEL_DIR is not set".to_string(),
            }
        })?;
        Ok(Self::new(model_dir))
    }

    pub fn with_max_input_bytes(mut self, max_input_bytes: usize) -> Self {
        self.max_input_bytes = max_input_bytes;
        self
    }

    pub fn with_precision(mut self, precision: KijiDistilbertPrecision) -> Self {
        self.precision = precision;
        self.version = format!("kiji/distilbert:ort-{}", precision.as_str());
        self.decoding_params.retain(|(key, _)| *key != "precision");
        self.decoding_params
            .push(("precision", precision.as_str().to_string()));
        #[cfg(any(test, feature = "test-support"))]
        {
            self.expected_bundle_sha256 = match precision {
                KijiDistilbertPrecision::Fp32 => KIJI_DISTILBERT_BUNDLE_SHA256.to_string(),
                KijiDistilbertPrecision::Int8 => KIJI_DISTILBERT_INT8_BUNDLE_SHA256.to_string(),
            };
        }
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    pub fn with_decoding_param(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.decoding_params.push((key, value.into()));
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn with_expected_bundle_sha256_for_tests(mut self, sha256: impl Into<String>) -> Self {
        self.expected_bundle_sha256 = sha256.into();
        self
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    pub fn precision(&self) -> KijiDistilbertPrecision {
        self.precision
    }

    fn expected_bundle_sha256(&self) -> &str {
        #[cfg(any(test, feature = "test-support"))]
        {
            &self.expected_bundle_sha256
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            match self.precision {
                KijiDistilbertPrecision::Fp32 => KIJI_DISTILBERT_BUNDLE_SHA256,
                KijiDistilbertPrecision::Int8 => KIJI_DISTILBERT_INT8_BUNDLE_SHA256,
            }
        }
    }
}

/// In-process Kiji DistilBERT ONNX Runtime backend.
#[derive(Debug)]
pub struct OrtKijiBackend {
    config: OrtKijiConfig,
    tokenizer: tokenizers::Tokenizer,
    session: Mutex<ort::session::Session>,
    has_token_type_ids: bool,
}

impl OrtKijiBackend {
    pub fn new(config: OrtKijiConfig) -> Result<Self, SafetyNetError> {
        verify_model_dir_for_precision(
            Some(config.model_dir()),
            config.precision(),
            config.expected_bundle_sha256(),
        )?;
        let tokenizer = tokenizers::Tokenizer::from_file(config.model_dir.join(TOKENIZER_FILE))
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to load kiji tokenizer: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;
        let session = ort::session::Session::builder()
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to initialize kiji ort session: {}",
                    sanitize_error(&err.to_string())
                ),
            })?
            .commit_from_file(config.model_dir.join(model_filename(config.precision())))
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to load kiji ort model: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;
        let has_token_type_ids = session
            .inputs()
            .iter()
            .any(|input| input.name() == "token_type_ids");
        Ok(Self {
            config,
            tokenizer,
            session: Mutex::new(session),
            has_token_type_ids,
        })
    }

    pub fn config(&self) -> &OrtKijiConfig {
        &self.config
    }
}

impl KijiDistilbertBackend for OrtKijiBackend {
    fn id(&self) -> &str {
        "kiji-distilbert-ort"
    }

    fn version(&self) -> &str {
        &self.config.version
    }

    fn decoding_params(&self) -> &[(&str, String)] {
        &self.config.decoding_params
    }

    fn infer(&self, clean: &str) -> Result<Vec<RawSpan>, SafetyNetError> {
        let actual = clean.len();
        if actual > self.config.max_input_bytes {
            return Err(SafetyNetError::InputTooLarge {
                limit: self.config.max_input_bytes,
                actual,
            });
        }

        let encoded =
            self.tokenizer
                .encode(clean, true)
                .map_err(|err| SafetyNetError::Runtime {
                    message: format!(
                        "kiji tokenizer failed: {}",
                        sanitize_error(&err.to_string())
                    ),
                })?;
        let offsets = encoded.get_offsets();
        let ids = encoded.get_ids();
        let attention = encoded.get_attention_mask();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let seq_len = ids.len();
        let input_ids: Vec<i64> = ids.iter().map(|&value| value as i64).collect();
        let attn_mask: Vec<i64> = attention.iter().map(|&value| value as i64).collect();
        let shape = [1usize, seq_len];
        let input_ids_tensor =
            ort::value::Tensor::from_array((shape, input_ids)).map_err(|err| {
                SafetyNetError::Runtime {
                    message: format!(
                        "kiji input_ids tensor failed: {}",
                        sanitize_error(&err.to_string())
                    ),
                }
            })?;
        let attn_tensor = ort::value::Tensor::from_array((shape, attn_mask)).map_err(|err| {
            SafetyNetError::Runtime {
                message: format!(
                    "kiji attention_mask tensor failed: {}",
                    sanitize_error(&err.to_string())
                ),
            }
        })?;
        let inputs = if self.has_token_type_ids {
            let token_type: Vec<i64> = vec![0; seq_len];
            let type_tensor =
                ort::value::Tensor::from_array((shape, token_type)).map_err(|err| {
                    SafetyNetError::Runtime {
                        message: format!(
                            "kiji token_type_ids tensor failed: {}",
                            sanitize_error(&err.to_string())
                        ),
                    }
                })?;
            ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attn_tensor,
                "token_type_ids" => type_tensor,
            ]
        } else {
            ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attn_tensor,
            ]
        };

        let mut session = self.session.lock().map_err(|err| SafetyNetError::Runtime {
            message: format!(
                "kiji ort session lock poisoned: {}",
                sanitize_error(&err.to_string())
            ),
        })?;
        let outputs = session.run(inputs).map_err(|err| SafetyNetError::Runtime {
            message: format!(
                "kiji ort inference failed: {}",
                sanitize_error(&err.to_string())
            ),
        })?;
        let logits = match outputs.iter().next() {
            Some((_, value)) => value,
            None => return Ok(Vec::new()),
        };
        let (shape_obj, flat) =
            logits
                .try_extract_tensor::<f32>()
                .map_err(|err| SafetyNetError::Runtime {
                    message: format!(
                        "kiji ort output failed: {}",
                        sanitize_error(&err.to_string())
                    ),
                })?;
        let shape: Vec<usize> = shape_obj.iter().map(|dim| *dim as usize).collect();
        if shape.len() != 3 || shape[0] != 1 || shape[1] != seq_len {
            return Err(SafetyNetError::InvalidOutput {
                message: "kiji ort returned invalid logits shape".to_string(),
            });
        }

        let num_labels = shape[2];
        let mut subword_labels: Vec<&str> = Vec::with_capacity(seq_len);
        let mut subword_scores = Vec::with_capacity(seq_len);
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
            subword_labels.push(ID2LABEL.get(argmax).copied().unwrap_or("O"));
            subword_scores.push(softmax_confidence(row, argmax));
        }

        normalize_raw_spans(
            merge_kiji_bio_spans(clean, offsets, &subword_labels, &subword_scores),
            clean,
        )
    }
}

fn merge_kiji_bio_spans(
    source: &str,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    subword_scores: &[f32],
) -> Vec<RawSpan> {
    let (effective_labels, effective_scores) =
        bridge_joiner_tokens(source, subword_spans, subword_labels, subword_scores);
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < effective_labels.len() {
        let tag = effective_labels[index].as_str();
        let (prefix, entity) = split_bio(tag);
        if prefix == 'O' || entity.is_empty() {
            index += 1;
            continue;
        }
        let Some(label) = kiji_entity_label(entity) else {
            index += 1;
            continue;
        };
        let (start, mut end) = subword_spans[index];
        if start == end {
            index += 1;
            continue;
        }
        let mut span_score = *effective_scores.get(index).unwrap_or(&0.0);
        let mut next = index + 1;
        while next < effective_labels.len() {
            let (next_prefix, next_entity) = split_bio(effective_labels[next].as_str());
            if next_prefix == 'I' && next_entity == entity {
                let (next_start, next_end) = subword_spans[next];
                if next_start != next_end {
                    end = next_end;
                    span_score = span_score.min(*effective_scores.get(next).unwrap_or(&0.0));
                }
                next += 1;
            } else {
                break;
            }
        }
        out.push(RawSpan::new(start, end, label, Some(span_score)));
        index = next;
    }
    out
}

fn bridge_joiner_tokens(
    source: &str,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    subword_scores: &[f32],
) -> (Vec<String>, Vec<f32>) {
    let mut effective_labels = subword_labels
        .iter()
        .map(|label| (*label).to_string())
        .collect::<Vec<_>>();
    let mut effective_scores = (0..subword_labels.len())
        .map(|index| *subword_scores.get(index).unwrap_or(&0.0))
        .collect::<Vec<_>>();

    for index in 1..subword_labels.len().saturating_sub(1) {
        let (prefix, _) = split_bio(subword_labels[index]);
        if prefix != 'O' {
            continue;
        }
        let (start, end) = subword_spans[index];
        let Some(token_text) = source.get(start..end) else {
            continue;
        };
        if !is_entity_joiner_token(token_text) {
            continue;
        }
        let (prev_prefix, prev_entity) = split_bio(subword_labels[index - 1]);
        if !matches!(prev_prefix, 'B' | 'I') || prev_entity.is_empty() {
            continue;
        }
        let (next_prefix, next_entity) = split_bio(subword_labels[index + 1]);
        if !matches!(next_prefix, 'B' | 'I') || next_entity != prev_entity {
            continue;
        }
        if next_prefix == 'B' && token_text.trim() == "," {
            continue;
        }
        effective_labels[index] = format!("I-{prev_entity}");
        if next_prefix == 'B' {
            effective_labels[index + 1] = format!("I-{prev_entity}");
        }
        let prev_score = *subword_scores.get(index - 1).unwrap_or(&0.0);
        let next_score = *subword_scores.get(index + 1).unwrap_or(&0.0);
        effective_scores[index] = (prev_score + next_score) / 2.0;
    }

    (effective_labels, effective_scores)
}

fn is_entity_joiner_token(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.chars().all(|ch| ".,@_-+:/#%&=".contains(ch))
}

fn kiji_entity_label(entity: &str) -> Option<&'static str> {
    match entity {
        "PER" => Some("person"),
        "LOC" => Some("location"),
        "ORG" => Some("organization"),
        "MISC" => Some("miscellaneous"),
        _ => None,
    }
}

fn sanitize_error(message: &str) -> String {
    message
        .split_ascii_whitespace()
        .map(sanitize_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_token(token: &str) -> String {
    if token.contains('@') {
        return "<redacted>".to_string();
    }

    let digit_count = token.bytes().filter(u8::is_ascii_digit).count();
    if digit_count >= 7 {
        return "<redacted>".to_string();
    }

    token.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_kiji_bio_to_private_labels() {
        let source = "Alice visits Berlin";
        let spans = [(0, 5), (6, 12), (13, 19)];
        let labels = ["B-PER", "O", "B-LOC"];
        let scores = [0.9, 0.1, 0.8];
        let out = merge_kiji_bio_spans(source, &spans, &labels, &scores);
        assert_eq!(
            out,
            vec![
                RawSpan::new(0, 5, "person", Some(0.9)),
                RawSpan::new(13, 19, "location", Some(0.8)),
            ]
        );
    }
}
