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
    #[error("nym safety net requires the safety-net-nym feature; install with `gaze setup --safety-net nym`")]
    NymFeatureDisabled,
    #[error("nym model_dir is missing; install with `gaze setup --safety-net nym`")]
    NymModelDirMissing,
    #[error("nym bundle: {0}; install with `gaze setup --safety-net nym`")]
    NymBundle(#[source] gaze::SafetyNetError),
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
