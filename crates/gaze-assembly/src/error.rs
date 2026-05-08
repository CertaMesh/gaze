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
    #[error("recognizer error: {0}")]
    Recognizer(gaze_recognizers::RecognizerError),
}

impl From<gaze_recognizers::RecognizerError> for BuildError {
    fn from(err: gaze_recognizers::RecognizerError) -> Self {
        match err {
            gaze_recognizers::RecognizerError::InvalidRegex(err) => {
                Self::Pipeline(gaze::Error::InvalidRegex(err))
            }
            err => Self::Recognizer(err),
        }
    }
}
