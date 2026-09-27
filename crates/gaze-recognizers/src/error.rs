/// Error type for recognizer construction and rulepack field validation.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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
    /// `on_fail = "record"` on a validator that must veto, or on a recognizer that scans whole
    /// digit runs for cards (every run would then be a candidate).
    #[error(
        "validator {kind} cannot keep a failed candidate on recognizer {recognizer_id}: {reason}"
    )]
    UnsupportedValidatorOnFail {
        /// Recognizer identifier.
        recognizer_id: String,
        /// Validator kind.
        kind: String,
        /// Why the combination is refused.
        reason: &'static str,
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

impl From<gaze_types::ValidatorKindParseError> for RecognizerError {
    fn from(value: gaze_types::ValidatorKindParseError) -> Self {
        match value {
            gaze_types::ValidatorKindParseError::UnsupportedValidator { kind } => {
                RecognizerError::UnsupportedValidator { kind }
            }
            _ => RecognizerError::UnsupportedValidator {
                kind: value.to_string(),
            },
        }
    }
}
