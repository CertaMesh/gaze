//! In-process ONNX Runtime backend for the pinned Nym-small int8 model.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gaze_types::nym::NymOperatingPoint;
use gaze_types::SafetyNetError;

use super::artifacts::{
    verify_id2label, verify_nym_bundle_with_digest, NYM_SMALL_CONFIG_FILE,
    NYM_SMALL_INT8_BUNDLE_SHA256, NYM_SMALL_MODEL_FILE, NYM_SMALL_TOKENIZER_FILE,
};
use super::decode::{
    check_char_coverage, decode_pieces, plan_windows, softmax_row, NymSpan, PieceScore, RowMerger,
    NUM_LABELS,
};

const DEFAULT_MAX_INPUT_BYTES: usize = 1024 * 1024;
/// One intra-op thread by default, matching the in-process Kiji backend: deterministic compute
/// and no contention with the caller's own thread pool. Raise it with
/// [`NymConfig::with_intra_threads`] when latency matters more.
pub const DEFAULT_NYM_INTRA_THREADS: NonZeroUsize = NonZeroUsize::MIN;

/// Configuration for the Nym-small safety net.
#[derive(Debug, Clone)]
pub struct NymConfig {
    model_dir: PathBuf,
    operating_point: NymOperatingPoint,
    intra_threads: NonZeroUsize,
    max_input_bytes: usize,
    expected_bundle_sha256: &'static str,
}

impl NymConfig {
    /// A config for the bundle at `model_dir` with the op-B operating point.
    pub fn new(model_dir: impl Into<PathBuf>) -> Self {
        Self {
            model_dir: model_dir.into(),
            operating_point: NymOperatingPoint::op_b(),
            intra_threads: DEFAULT_NYM_INTRA_THREADS,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            expected_bundle_sha256: NYM_SMALL_INT8_BUNDLE_SHA256,
        }
    }

    /// Reads `GAZE_NYM_MODEL_DIR` (required) and `GAZE_NYM_INTRA_THREADS` (optional).
    pub fn from_env() -> Result<Self, SafetyNetError> {
        let model_dir =
            std::env::var_os("GAZE_NYM_MODEL_DIR").ok_or_else(|| SafetyNetError::Unavailable {
                reason: "GAZE_NYM_MODEL_DIR is not set".to_string(),
            })?;
        let mut config = Self::new(model_dir);
        if let Some(raw) = std::env::var_os("GAZE_NYM_INTRA_THREADS") {
            let threads = raw
                .to_str()
                .and_then(|value| value.parse::<NonZeroUsize>().ok())
                .ok_or_else(|| SafetyNetError::Unavailable {
                    reason: "GAZE_NYM_INTRA_THREADS must be a positive integer".to_string(),
                })?;
            config = config.with_intra_threads(threads);
        }
        Ok(config)
    }

    pub fn with_operating_point(mut self, operating_point: NymOperatingPoint) -> Self {
        self.operating_point = operating_point;
        self
    }

    pub fn with_intra_threads(mut self, intra_threads: NonZeroUsize) -> Self {
        self.intra_threads = intra_threads;
        self
    }

    pub fn with_max_input_bytes(mut self, max_input_bytes: usize) -> Self {
        self.max_input_bytes = max_input_bytes;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn with_expected_bundle_sha256_for_tests(mut self, sha256: &'static str) -> Self {
        self.expected_bundle_sha256 = sha256;
        self
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    pub fn operating_point(&self) -> &NymOperatingPoint {
        &self.operating_point
    }

    pub fn intra_threads(&self) -> NonZeroUsize {
        self.intra_threads
    }
}

/// Loaded tokenizer and ORT session.
pub(crate) struct NymOrtBackend {
    config: NymConfig,
    tokenizer: tokenizers::Tokenizer,
    session: Mutex<ort::session::Session>,
    bos: u32,
    eos: u32,
}

impl std::fmt::Debug for NymOrtBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NymOrtBackend")
            .field("intra_threads", &self.config.intra_threads)
            .finish_non_exhaustive()
    }
}

impl NymOrtBackend {
    pub(crate) fn new(config: NymConfig) -> Result<Self, SafetyNetError> {
        verify_nym_bundle_with_digest(&config.model_dir, config.expected_bundle_sha256)?;
        let config_json =
            std::fs::read(config.model_dir.join(NYM_SMALL_CONFIG_FILE)).map_err(|_| {
                SafetyNetError::WeightsMissing {
                    path: format!("<missing:{NYM_SMALL_CONFIG_FILE}>"),
                }
            })?;
        verify_id2label(&config_json)?;

        let unavailable =
            |what: &str, err: &dyn std::fmt::Display| SafetyNetError::ModelUnavailable {
                reason: format!("nym {what}: {}", sanitize_error(&err.to_string())),
            };
        let mut tokenizer =
            tokenizers::Tokenizer::from_file(config.model_dir.join(NYM_SMALL_TOKENIZER_FILE))
                .map_err(|err| unavailable("tokenizer failed to load", &err))?;
        tokenizer
            .with_truncation(None)
            .map_err(|err| unavailable("tokenizer truncation could not be disabled", &err))?;
        tokenizer.with_padding(None);
        let special = |token: &str| {
            tokenizer
                .token_to_id(token)
                .ok_or_else(|| SafetyNetError::ModelUnavailable {
                    reason: format!("nym tokenizer has no {token} token"),
                })
        };
        let (bos, eos) = (special("<bos>")?, special("<eos>")?);

        let session = ort::session::Session::builder()
            .map_err(|err| unavailable("ort session failed to initialize", &err))?
            .with_intra_threads(config.intra_threads.get())
            .map_err(|err| unavailable("ort intra-op threads failed", &err))?
            .with_inter_threads(1)
            .map_err(|err| unavailable("ort inter-op threads failed", &err))?
            .with_parallel_execution(false)
            .map_err(|err| unavailable("ort sequential execution failed", &err))?
            .with_deterministic_compute(true)
            .map_err(|err| unavailable("ort deterministic compute failed", &err))?
            .commit_from_file(config.model_dir.join(NYM_SMALL_MODEL_FILE))
            .map_err(|err| unavailable("ort model failed to load", &err))?;
        Ok(Self {
            config,
            tokenizer,
            session: Mutex::new(session),
            bos,
            eos,
        })
    }

    pub(crate) fn operating_point(&self) -> &NymOperatingPoint {
        &self.config.operating_point
    }

    pub(crate) fn infer(&self, clean: &str) -> Result<Vec<NymSpan>, SafetyNetError> {
        let scores = self.score_pieces(clean)?;
        decode_pieces(clean, &scores.0, &scores.1, &self.config.operating_point)
    }

    /// Tokenizes `clean` and scores every piece: `(char offsets, per-piece scores)`.
    pub(crate) fn score_pieces(
        &self,
        clean: &str,
    ) -> Result<(Vec<(usize, usize)>, Vec<PieceScore>), SafetyNetError> {
        if clean.len() > self.config.max_input_bytes {
            return Err(SafetyNetError::InputTooLarge {
                limit: self.config.max_input_bytes,
                actual: clean.len(),
            });
        }
        let encoding = self
            .tokenizer
            .encode_char_offsets(clean, false)
            .map_err(|err| SafetyNetError::Runtime {
                message: format!("nym tokenizer failed: {}", sanitize_error(&err.to_string())),
            })?;
        let ids = encoding.get_ids();
        let offsets = encoding.get_offsets().to_vec();
        let chars = clean.chars().collect::<Vec<_>>();
        check_char_coverage(&chars, &offsets)?;
        let mut merger = RowMerger::new(ids.len());
        for window in plan_windows(ids.len()) {
            let probs = self.run_window(&ids[window.clone()])?;
            merger.add_window(window, &probs);
        }
        let scores = merger.finish()?.iter().map(PieceScore::from_row).collect();
        Ok((offsets, scores))
    }

    fn run_window(&self, ids: &[u32]) -> Result<Vec<[f32; NUM_LABELS]>, SafetyNetError> {
        let runtime = |what: &str, err: &dyn std::fmt::Display| SafetyNetError::Runtime {
            message: format!("nym {what}: {}", sanitize_error(&err.to_string())),
        };
        let input_ids = std::iter::once(self.bos)
            .chain(ids.iter().copied())
            .chain(std::iter::once(self.eos))
            .map(i64::from)
            .collect::<Vec<_>>();
        let seq_len = input_ids.len();
        let shape = [1usize, seq_len];
        let ids_tensor = ort::value::Tensor::from_array((shape, input_ids))
            .map_err(|err| runtime("input_ids tensor failed", &err))?;
        let mask_tensor = ort::value::Tensor::from_array((shape, vec![1i64; seq_len]))
            .map_err(|err| runtime("attention_mask tensor failed", &err))?;

        let mut session = self.session.lock().map_err(|_| SafetyNetError::Runtime {
            message: "nym ort session lock poisoned".to_string(),
        })?;
        let outputs = session
            .run(ort::inputs![
                "input_ids" => ids_tensor,
                "attention_mask" => mask_tensor,
            ])
            .map_err(|err| runtime("ort inference failed", &err))?;
        let (_, logits) = outputs
            .iter()
            .next()
            .ok_or_else(|| SafetyNetError::InvalidOutput {
                message: "nym ort returned no output tensor".to_string(),
            })?;
        let (dims, flat) =
            logits
                .try_extract_tensor::<f32>()
                .map_err(|_| SafetyNetError::InvalidOutput {
                    message: "nym ort returned invalid output tensor".to_string(),
                })?;
        if dims.len() != 3
            || dims[0] != 1
            || dims[1] as usize != seq_len
            || dims[2] as usize != NUM_LABELS
            || flat.len() != seq_len * NUM_LABELS
        {
            return Err(SafetyNetError::InvalidOutput {
                message: "nym ort returned invalid logits shape".to_string(),
            });
        }
        // Drop the <bos>/<eos> rows; row k + 1 belongs to content piece k.
        (1..seq_len - 1)
            .map(|position| softmax_row(&flat[position * NUM_LABELS..(position + 1) * NUM_LABELS]))
            .collect()
    }
}

/// Drops tokens that could echo input (emails, long digit runs) from runtime error text.
fn sanitize_error(message: &str) -> String {
    message
        .split_ascii_whitespace()
        .map(|token| {
            if token.contains('@') || token.bytes().filter(u8::is_ascii_digit).count() >= 7 {
                "<redacted>"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
