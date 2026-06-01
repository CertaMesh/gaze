use std::path::Path;

use gaze_types::{Candidate, ConflictTier, DetectContext, DetectError, PiiClass, Recognizer};

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

    fn detect(&self, input: &str, _ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        self.detector
            .detect_span_results(input)
            .map_err(|err| {
                tracing::error!(backend = self.detector.backend_kind.as_str(), error = %err, "ner: backend detect failed closed");
                DetectError::backend(self.id(), err.to_string())
            })
            .map(|spans| {
                spans
                    .into_iter()
                    .filter(|span| span.score >= self.detector.threshold)
                    .map(|span| {
                        Candidate::new(
                            span.span,
                            span.class,
                            self.id(),
                            span.score,
                            0,
                            None,
                            self.token_family(),
                            format!("ner/{}", self.detector.backend_kind.as_str()),
                            ConflictTier::None,
                            Vec::new(),
                        )
                        .with_recognizer_version_id(self.detector.recognizer_version_id())
                    })
                    .collect()
            })
    }

    fn token_family(&self) -> &str {
        "counter"
    }
}
