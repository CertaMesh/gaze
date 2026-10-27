use gaze_types::{
    DetectContext, Detection, Detector, DictionaryBundle, LocaleTag, RecognizerRuntimeError,
};

struct FailingBackend;
impl Detector for FailingBackend {
    fn detect(&self, _: &str) -> Vec<Detection> {
        panic!("pipeline context must preserve fallible detection")
    }
    fn try_detect(&self, _: &str) -> Result<Vec<Detection>, RecognizerRuntimeError> {
        Err(RecognizerRuntimeError::new(
            "test.backend",
            "synthetic failure",
        ))
    }
}

#[test]
fn context_default_preserves_existing_backend_failure() {
    let dictionaries = DictionaryBundle::default();
    let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
    assert_eq!(
        FailingBackend
            .try_detect_with_context("public text", &ctx)
            .unwrap_err(),
        RecognizerRuntimeError::new("test.backend", "synthetic failure")
    );
}
