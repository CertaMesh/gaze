use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("no effective recognizers configured")]
    NoRecognizers,
    #[error("policy error: {0}")]
    Policy(#[from] gaze::PolicyError),
    #[error("rulepack error: {0}")]
    Rulepack(#[from] gaze::RulepackError),
    #[error("pipeline error: {0}")]
    Pipeline(#[from] gaze::Error),
    #[error("unknown locale bucket '{bucket}' for recognizer '{recognizer_id}'")]
    UnknownLocaleBucket {
        recognizer_id: String,
        bucket: String,
    },
}

impl From<gaze_recognizers::RecognizerError> for BuildError {
    fn from(err: gaze_recognizers::RecognizerError) -> Self {
        match err {
            gaze_recognizers::RecognizerError::InvalidRegex(err) => {
                Self::Pipeline(gaze::Error::InvalidRegex(err))
            }
            gaze_recognizers::RecognizerError::UnsupportedValidator { kind } => {
                Self::Rulepack(gaze::RulepackError::UnsupportedValidator { kind })
            }
            gaze_recognizers::RecognizerError::UnsupportedNormalizer { kind } => {
                Self::Rulepack(gaze::RulepackError::UnsupportedNormalizer { kind })
            }
        }
    }
}
