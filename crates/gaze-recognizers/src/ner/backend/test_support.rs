use std::path::Path;
use std::sync::Arc;

use gaze_types::PiiClass;

use super::NerBackend;
use crate::ner::detector::NerDetector;
use crate::ner::error::NerRuntimeError;
use crate::ner::recognizer::NerRecognizer;
use crate::ner::types::{NerBackendKind, NerOptions, NerSpanResult};

struct TestSupportBackend;

impl NerBackend for TestSupportBackend {
    fn detect(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
        Ok(input
            .find("Alice Example")
            .map(|start| NerSpanResult {
                span: start..start + "Alice Example".len(),
                class: PiiClass::Name,
                score: 0.40,
            })
            .into_iter()
            .collect())
    }
}

pub(crate) fn load_test_support_recognizer(
    model_dir: &Path,
    options: &NerOptions,
) -> Option<NerRecognizer> {
    if model_dir.file_name().and_then(|name| name.to_str()) != Some("__gaze_test_fixed_ner") {
        return None;
    }

    Some(NerRecognizer {
        detector: NerDetector {
            model_dir: model_dir.to_path_buf(),
            backend_kind: NerBackendKind::Ort,
            locale: options.locale.clone(),
            threshold: options.threshold,
            backend: Arc::new(TestSupportBackend),
        },
    })
}
