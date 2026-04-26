use std::path::Path;

use gaze_types::{Candidate, ConflictTier, DetectContext, PiiClass, Recognizer};

#[cfg(feature = "test-support")]
use super::backend::test_support::load_test_support_recognizer;
use super::detector::NerDetector;
use super::error::NerLoadError;
use super::types::NerOptions;

pub struct NerRecognizer {
    pub(crate) detector: NerDetector,
}

impl NerRecognizer {
    pub fn load_with_options(model_dir: &Path, options: NerOptions) -> Result<Self, NerLoadError> {
        #[cfg(feature = "test-support")]
        if let Some(recognizer) = load_test_support_recognizer(model_dir, &options) {
            return Ok(recognizer);
        }

        Ok(Self {
            detector: NerDetector::load_with_options(model_dir, options)?,
        })
    }
}

impl Recognizer for NerRecognizer {
    fn id(&self) -> &str {
        "ner"
    }

    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }

    fn detect(&self, input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
        match self.detector.backend.detect(input) {
            Ok(spans) => spans
                .into_iter()
                .filter(|span| span.score >= self.detector.threshold)
                .map(|span| Candidate {
                    span: span.span,
                    class: span.class,
                    recognizer_id: self.id().to_string(),
                    score: span.score,
                    priority: 0,
                    canonical_form: None,
                    token_family: self.token_family().to_string(),
                    source: format!("ner/{}", self.detector.backend_kind.as_str()),
                    decided_by: ConflictTier::None,
                    merged_sources: Vec::new(),
                })
                .collect(),
            Err(err) => {
                tracing::warn!(backend = self.detector.backend_kind.as_str(), error = %err, "ner: backend detect failed");
                Vec::new()
            }
        }
    }

    fn token_family(&self) -> &str {
        "counter"
    }
}
