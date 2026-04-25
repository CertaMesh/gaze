use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("policy error: {0}")]
    Policy(#[from] gaze::PolicyError),
    #[error("rulepack error: {0}")]
    Rulepack(#[from] gaze::RulepackError),
    #[error("pipeline error: {0}")]
    Pipeline(#[from] gaze::Error),
}

pub fn build_pipeline(
    _policy: &gaze::Policy,
    _context: &gaze::TypedContext,
    _rulepacks: &[gaze::Rulepack],
) -> Result<gaze::Pipeline, BuildError> {
    gaze::Pipeline::builder().build().map_err(BuildError::from)
}
