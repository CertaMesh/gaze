/// Error type for recognizer construction and rulepack field validation.
#[derive(Debug, thiserror::Error)]
pub enum RecognizerError {
    /// Regex pattern failed to compile.
    #[error("invalid regex: {0}")]
    InvalidRegex(#[source] regex::Error),
    /// Validator kind is unsupported by this recognizer build.
    #[error("unsupported validator: {kind}")]
    UnsupportedValidator {
        /// Unsupported validator kind.
        kind: String,
    },
    /// Normalizer kind is unsupported by this recognizer build.
    #[error("unsupported normalizer: {kind}")]
    UnsupportedNormalizer {
        /// Unsupported normalizer kind.
        kind: String,
    },
}

/// Result alias for recognizer-local operations.
pub type Result<T> = std::result::Result<T, RecognizerError>;
