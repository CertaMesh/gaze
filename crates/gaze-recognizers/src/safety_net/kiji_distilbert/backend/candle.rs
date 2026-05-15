use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use candle_core::{Device, Tensor};
use gaze_types::SafetyNetError;

use super::artifacts::{verify_model_dir, KIJI_DISTILBERT_BUNDLE_SHA256};
use super::decode::decode_logits;
use super::{normalize_raw_spans, KijiDistilbertBackend, RawSpan};

const DEFAULT_MAX_INPUT_BYTES: usize = 1024 * 1024;
const MODEL_FILE: &str = "model.onnx";
const TOKENIZER_FILE: &str = "tokenizer.json";

#[derive(Debug, Clone)]
pub struct CandleKijiConfig {
    model_dir: PathBuf,
    max_input_bytes: usize,
    version: String,
    decoding_params: Vec<(&'static str, String)>,
    #[cfg(any(test, feature = "test-support"))]
    expected_bundle_sha256: String,
}

impl CandleKijiConfig {
    pub fn new(model_dir: impl Into<PathBuf>) -> Self {
        Self {
            model_dir: model_dir.into(),
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            version: "kiji/distilbert:candle-onnx".to_string(),
            decoding_params: vec![("runtime", "candle-onnx".to_string())],
            #[cfg(any(test, feature = "test-support"))]
            expected_bundle_sha256: KIJI_DISTILBERT_BUNDLE_SHA256.to_string(),
        }
    }

    pub fn with_max_input_bytes(mut self, max_input_bytes: usize) -> Self {
        self.max_input_bytes = max_input_bytes;
        self
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    fn expected_bundle_sha256(&self) -> &str {
        #[cfg(any(test, feature = "test-support"))]
        {
            &self.expected_bundle_sha256
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            KIJI_DISTILBERT_BUNDLE_SHA256
        }
    }
}

#[derive(Debug)]
pub struct CandleKijiBackend {
    config: CandleKijiConfig,
    tokenizer: tokenizers::Tokenizer,
    model: Mutex<candle_onnx::onnx::ModelProto>,
    has_token_type_ids: bool,
}

impl CandleKijiBackend {
    pub fn new(config: CandleKijiConfig) -> Result<Self, SafetyNetError> {
        verify_model_dir(Some(config.model_dir()), config.expected_bundle_sha256())?;
        let tokenizer = tokenizers::Tokenizer::from_file(config.model_dir.join(TOKENIZER_FILE))
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to load kiji tokenizer: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;
        let model = candle_onnx::read_file(config.model_dir.join(MODEL_FILE)).map_err(|err| {
            SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to load kiji candle onnx model: {}",
                    sanitize_error(&err.to_string())
                ),
            }
        })?;
        let has_token_type_ids = model
            .graph
            .as_ref()
            .map(|graph| {
                graph
                    .input
                    .iter()
                    .any(|input| input.name == "token_type_ids")
            })
            .unwrap_or(false);
        Ok(Self {
            config,
            tokenizer,
            model: Mutex::new(model),
            has_token_type_ids,
        })
    }
}

impl KijiDistilbertBackend for CandleKijiBackend {
    fn id(&self) -> &str {
        "kiji-distilbert-candle"
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
        let shape = (1usize, seq_len);
        let mut inputs = HashMap::new();
        inputs.insert(
            "input_ids".to_string(),
            Tensor::from_vec(input_ids, shape, &Device::Cpu).map_err(|err| {
                SafetyNetError::Runtime {
                    message: format!(
                        "kiji candle input_ids tensor failed: {}",
                        sanitize_error(&err.to_string())
                    ),
                }
            })?,
        );
        inputs.insert(
            "attention_mask".to_string(),
            Tensor::from_vec(attn_mask, shape, &Device::Cpu).map_err(|err| {
                SafetyNetError::Runtime {
                    message: format!(
                        "kiji candle attention_mask tensor failed: {}",
                        sanitize_error(&err.to_string())
                    ),
                }
            })?,
        );
        if self.has_token_type_ids {
            inputs.insert(
                "token_type_ids".to_string(),
                Tensor::from_vec(vec![0i64; seq_len], shape, &Device::Cpu).map_err(|err| {
                    SafetyNetError::Runtime {
                        message: format!(
                            "kiji candle token_type_ids tensor failed: {}",
                            sanitize_error(&err.to_string())
                        ),
                    }
                })?,
            );
        }

        let model = self.model.lock().map_err(|err| SafetyNetError::Runtime {
            message: format!(
                "kiji candle model lock poisoned: {}",
                sanitize_error(&err.to_string())
            ),
        })?;
        let outputs =
            candle_onnx::simple_eval(&model, inputs).map_err(|err| SafetyNetError::Runtime {
                message: format!(
                    "kiji candle inference failed: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;
        let output = outputs
            .values()
            .next()
            .ok_or_else(|| SafetyNetError::InvalidOutput {
                message: "kiji candle returned no outputs".to_string(),
            })?;
        let shape = output.dims();
        if shape.len() != 3 || shape[0] != 1 || shape[1] != seq_len {
            return Err(SafetyNetError::InvalidOutput {
                message: "kiji candle returned invalid logits shape".to_string(),
            });
        }
        let num_labels = shape[2];
        let flat = output
            .flatten_all()
            .and_then(|tensor| tensor.to_vec1::<f32>())
            .map_err(|err| SafetyNetError::Runtime {
                message: format!(
                    "kiji candle output failed: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;

        normalize_raw_spans(
            decode_logits(clean, offsets, &flat, seq_len, num_labels),
            clean,
        )
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
