use std::path::PathBuf;

use thiserror::Error;

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
pub(crate) enum NerRuntimeError {
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
