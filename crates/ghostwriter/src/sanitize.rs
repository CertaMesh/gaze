//! Sanitize orchestration.

use crate::blob::SessionBlob;
use crate::detect::WorkaDetector;
use crate::errors::SanitizeError;
use crate::known_context;
use crate::placeholder::PlaceholderMap;
use crate::typed_unknown;
use crate::types::{Metadata, SanitizeRequest, SanitizeResponse};

pub fn sanitize(req: SanitizeRequest) -> Result<SanitizeResponse, SanitizeError> {
    let detector = WorkaDetector::new();
    sanitize_with_detector(req, &detector)
}

pub fn sanitize_with_detector(
    req: SanitizeRequest,
    detector: &WorkaDetector,
) -> Result<SanitizeResponse, SanitizeError> {
    if req.text.is_empty() {
        // Empty input is valid per spec ("sanitize should not fail merely
        // because no PII was detected"). Return an empty response.
        return Ok(SanitizeResponse {
            clean_text: String::new(),
            session_blob: SessionBlob::new().encode()?,
            warnings: vec![],
            metadata: Metadata::default(),
        });
    }

    let mut map = PlaceholderMap::new();

    // Stage 1: known context replacement.
    let stage1 = known_context::apply(&req.text, &req.context, &mut map);

    // Stage 2: worka detection on the stage-1 output.
    let detections = detector.detect(&stage1)?;
    let stage2 = typed_unknown::apply(&stage1, &detections, &mut map);

    // Assemble session blob in insertion order.
    let mut blob = SessionBlob::new();
    for (token, raw) in map.entries() {
        blob.insert(token.clone(), raw.clone());
    }
    let session_blob = blob.encode()?;

    Ok(SanitizeResponse {
        clean_text: stage2,
        session_blob,
        warnings: vec![],
        metadata: Metadata {
            placeholders: map.token_list(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::SessionBlob;
    use crate::types::Context;

    fn req(text: &str, ctx: Context) -> SanitizeRequest {
        SanitizeRequest {
            text: text.to_string(),
            context: ctx,
        }
    }

    fn ctx_markus() -> Context {
        Context {
            customer_name: Some("Markus Mueller".into()),
            customer_email: Some("mueller.markus@icloud.com".into()),
            customer_phone: Some("+49 151 23456789".into()),
        }
    }

    #[test]
    fn empty_text_returns_empty_response() {
        let resp = sanitize(req("", Context::default())).unwrap();
        assert_eq!(resp.clean_text, "");
        assert!(resp.metadata.placeholders.is_empty());
    }

    #[test]
    fn spec_example_stage1_replaces_known_fields() {
        let text = "Hi Artistfy, Markus Mueller here. Please resend to mueller.markus@icloud.com. If needed call +49 151 23456789. Alternate email: markus.mueller@example.de";
        let resp = sanitize(req(text, ctx_markus())).unwrap();
        assert!(resp.clean_text.contains("<CUSTOMER_NAME>"));
        assert!(resp.clean_text.contains("<CUSTOMER_EMAIL>"));
        assert!(resp.clean_text.contains("<CUSTOMER_PHONE>"));
        // Raw PII must not appear in clean_text.
        assert!(!resp.clean_text.contains("Markus Mueller"));
        assert!(!resp.clean_text.contains("mueller.markus@icloud.com"));
        assert!(!resp.clean_text.contains("+49 151 23456789"));
        // The alternate email should be tokenized by stage 2 into <EMAIL_1>
        // IF the pinned pii crate detects it. If not, the assertion below
        // may need relaxing — see plan self-review notes. Try it strict
        // first and relax only if detection fails.
        assert!(resp.clean_text.contains("<EMAIL_1>"));
        assert!(!resp.clean_text.contains("markus.mueller@example.de"));
    }

    #[test]
    fn session_blob_decodes_and_contains_known_tokens() {
        let text = "Markus Mueller at mueller.markus@icloud.com";
        let resp = sanitize(req(text, ctx_markus())).unwrap();
        let blob = SessionBlob::decode(&resp.session_blob).unwrap();
        assert_eq!(
            blob.placeholders.get("<CUSTOMER_NAME>").unwrap(),
            "Markus Mueller"
        );
        assert_eq!(
            blob.placeholders.get("<CUSTOMER_EMAIL>").unwrap(),
            "mueller.markus@icloud.com"
        );
    }

    #[test]
    fn metadata_lists_placeholders_in_insertion_order() {
        let text = "Markus Mueller at mueller.markus@icloud.com and other@x.com";
        let resp = sanitize(req(text, ctx_markus())).unwrap();
        assert_eq!(resp.metadata.placeholders[0], "<CUSTOMER_NAME>");
        assert_eq!(resp.metadata.placeholders[1], "<CUSTOMER_EMAIL>");
        assert!(resp.metadata.placeholders.iter().any(|p| p == "<EMAIL_1>"));
    }
}
