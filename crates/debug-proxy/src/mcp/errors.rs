use gaze::{CleanDocument, Pipeline, RawDocument, Session};

pub const CANARY: &str = "CANARY_EMAIL_DO_NOT_LEAK@test.local";

#[derive(Default)]
pub struct ErrorSanitizer;

impl ErrorSanitizer {
    pub fn sanitize(
        &self,
        pipeline: &Pipeline,
        session: &Session,
        raw: &str,
    ) -> Result<String, gaze::Error> {
        let clean = pipeline.redact(session, RawDocument::Text(raw.to_string()))?;
        let CleanDocument::Text(text) = clean else {
            unreachable!("text input must produce text output");
        };
        assert!(!text.contains(CANARY), "canary survived error sanitization");
        Ok(text)
    }
}
