use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gaze_types::SafetyNetError;
use tract_onnx::prelude::*;

use super::artifacts::{verify_model_dir, KIJI_DISTILBERT_BUNDLE_SHA256};
use super::decode::decode_logits;
use super::{normalize_raw_spans, KijiDistilbertBackend, RawSpan};

const DEFAULT_MAX_INPUT_BYTES: usize = 1024 * 1024;
const MODEL_FILE: &str = "model.onnx";
const TOKENIZER_FILE: &str = "tokenizer.json";

#[derive(Debug, Clone)]
pub struct TractKijiConfig {
    model_dir: PathBuf,
    max_input_bytes: usize,
    version: String,
    decoding_params: Vec<(&'static str, String)>,
    #[cfg(any(test, feature = "test-support"))]
    expected_bundle_sha256: String,
}

impl TractKijiConfig {
    pub fn new(model_dir: impl Into<PathBuf>) -> Self {
        Self {
            model_dir: model_dir.into(),
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            version: "kiji/distilbert:tract".to_string(),
            decoding_params: vec![("runtime", "tract".to_string())],
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
pub struct TractKijiBackend {
    config: TractKijiConfig,
    tokenizer: tokenizers::Tokenizer,
    model: Mutex<Arc<TypedRunnableModel>>,
    has_token_type_ids: bool,
}

impl TractKijiBackend {
    pub fn new(config: TractKijiConfig) -> Result<Self, SafetyNetError> {
        verify_model_dir(Some(config.model_dir()), config.expected_bundle_sha256())?;
        let tokenizer = tokenizers::Tokenizer::from_file(config.model_dir.join(TOKENIZER_FILE))
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to load kiji tokenizer: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;
        let model = tract_onnx::onnx()
            .model_for_path(config.model_dir.join(MODEL_FILE))
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to load kiji tract model: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;
        let has_token_type_ids = model
            .input_outlets()
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to inspect kiji tract model: {}",
                    sanitize_error(&err.to_string())
                ),
            })?
            .iter()
            .any(|outlet| model.node(outlet.node).name == "token_type_ids");
        let model = model
            .into_optimized()
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to optimize kiji tract model: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;
        let model = model
            .into_runnable()
            .map_err(|err| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to prepare kiji tract model: {}",
                    sanitize_error(&err.to_string())
                ),
            })?;
        Ok(Self {
            config,
            tokenizer,
            model: Mutex::new(model),
            has_token_type_ids,
        })
    }
}

impl KijiDistilbertBackend for TractKijiBackend {
    fn id(&self) -> &str {
        "kiji-distilbert-tract"
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
        let shape = [1usize, seq_len];
        let input_ids: Vec<i64> = ids.iter().map(|&value| value as i64).collect();
        let attn_mask: Vec<i64> = attention.iter().map(|&value| value as i64).collect();
        let input_ids_tensor = Tensor::from_shape(&shape, &input_ids)
            .map_err(|err| SafetyNetError::Runtime {
                message: format!(
                    "kiji tract input_ids tensor failed: {}",
                    sanitize_error(&err.to_string())
                ),
            })?
            .into_tvalue();
        let attn_tensor = Tensor::from_shape(&shape, &attn_mask)
            .map_err(|err| SafetyNetError::Runtime {
                message: format!(
                    "kiji tract attention_mask tensor failed: {}",
                    sanitize_error(&err.to_string())
                ),
            })?
            .into_tvalue();
        let inputs = if self.has_token_type_ids {
            let token_type = vec![0i64; seq_len];
            let type_tensor = Tensor::from_shape(&shape, &token_type)
                .map_err(|err| SafetyNetError::Runtime {
                    message: format!(
                        "kiji tract token_type_ids tensor failed: {}",
                        sanitize_error(&err.to_string())
                    ),
                })?
                .into_tvalue();
            tvec!(input_ids_tensor, attn_tensor, type_tensor)
        } else {
            tvec!(input_ids_tensor, attn_tensor)
        };

        let model = self.model.lock().map_err(|err| SafetyNetError::Runtime {
            message: format!(
                "kiji tract model lock poisoned: {}",
                sanitize_error(&err.to_string())
            ),
        })?;
        let outputs = model.run(inputs).map_err(|err| SafetyNetError::Runtime {
            message: format!(
                "kiji tract inference failed: {}",
                sanitize_error(&err.to_string())
            ),
        })?;
        decode_first_output(outputs.as_slice(), |output| {
            let tensor: &Tensor = output;
            let shape = tensor.shape();
            if shape.len() != 3 || shape[0] != 1 || shape[1] != seq_len {
                return Err(SafetyNetError::InvalidOutput {
                    message: "kiji tract returned invalid logits shape".to_string(),
                });
            }
            let num_labels = shape[2];
            let flat = tensor
                .try_as_plain()
                .and_then(|view| view.as_slice::<f32>())
                .map_err(|err| SafetyNetError::Runtime {
                    message: format!(
                        "kiji tract output slice failed: {}",
                        sanitize_error(&err.to_string())
                    ),
                })?;

            normalize_raw_spans(
                decode_logits(clean, offsets, flat, seq_len, num_labels)?,
                clean,
            )
        })
    }
}

fn decode_first_output<T, U>(
    outputs: &[T],
    decode: impl FnOnce(&T) -> Result<Vec<U>, SafetyNetError>,
) -> Result<Vec<U>, SafetyNetError> {
    let output = outputs
        .first()
        .ok_or_else(|| SafetyNetError::InvalidOutput {
            message: "kiji tract returned no output".to_string(),
        })?;
    decode(output)
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
    fn missing_output_fails_closed() {
        let outputs: [(); 0] = [];
        let err = decode_first_output(&outputs, |_| Ok(Vec::<()>::new()))
            .expect_err("missing output must fail closed");

        match err {
            SafetyNetError::InvalidOutput { message } => {
                assert_eq!(message, "kiji tract returned no output");
            }
            other => panic!("expected InvalidOutput, got {other:?}"),
        }
    }
}
