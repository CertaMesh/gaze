//! Sanitize orchestration.

use crate::blob::SessionBlob;
use crate::context::ContextDetector;
use crate::errors::SanitizeError;
use crate::index::IndexDetector;
use crate::types::{Context, Metadata, SanitizeRequest, SanitizeResponse};
use gaze::{Action, ClassRule, CleanDocument, DefaultRule, Pipeline, RawDocument, RegexDetector, Scope, Session};

pub fn sanitize(req: SanitizeRequest) -> Result<SanitizeResponse, SanitizeError> {
    if req.text.is_empty() {
        let session = Session::new(Scope::Conversation("ghostwriter".to_string()))
            .map_err(|e| SanitizeError::BlobEncoding(e.to_string()))?;
        // Empty input is valid per spec ("sanitize should not fail merely
        // because no PII was detected"). Return an empty response.
        return Ok(SanitizeResponse {
            clean_text: String::new(),
            session_blob: SessionBlob::new(
                session
                    .export()
                    .map_err(|e| SanitizeError::BlobEncoding(e.to_string()))?,
            )
            .encode()?,
            warnings: vec![],
            metadata: Metadata::default(),
        });
    }

    let pipeline = build_pipeline(&req.context)?;
    let session = Session::new(Scope::Conversation("ghostwriter".to_string()))
        .map_err(|e| SanitizeError::BlobEncoding(e.to_string()))?;
    let clean = pipeline
        .redact(&session, RawDocument::Text(req.text))
        .map_err(|e| SanitizeError::DetectorFailure(e.to_string()))?;
    let CleanDocument::Text(clean_text) = clean else {
        return Err(SanitizeError::PlaceholderMapping(
            "expected text clean document".to_string(),
        ));
    };

    let aliases = extract_aliases(&clean_text);
    let mut outward_text = clean_text;
    let snapshot = session
        .export()
        .map_err(|e| SanitizeError::BlobEncoding(e.to_string()))?;
    let mut blob = SessionBlob::new(snapshot);
    for (external, internal) in &aliases {
        outward_text = outward_text.replace(internal, external);
        blob.insert_alias(external.clone(), internal.clone());
    }
    let session_blob = blob.encode()?;

    Ok(SanitizeResponse {
        clean_text: outward_text,
        session_blob,
        warnings: vec![],
        metadata: Metadata {
            placeholders: aliases.into_iter().map(|(external, _)| external).collect(),
        },
    })
}

fn build_pipeline(context: &Context) -> Result<Pipeline, SanitizeError> {
    Pipeline::builder()
        .detector(ContextDetector::new(context))
        .detector(IndexDetector::new(context))
        .detector(
            RegexDetector::emails()
                .map_err(|e| SanitizeError::DetectorFailure(e.to_string()))?,
        )
        .rule(ClassRule::new(
            gaze::PiiClass::custom("customer_name"),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            gaze::PiiClass::custom("customer_email"),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            gaze::PiiClass::custom("customer_phone"),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            gaze::PiiClass::custom("order_id"),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            gaze::PiiClass::custom("song"),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            gaze::PiiClass::custom("artist"),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .map_err(|e| SanitizeError::DetectorFailure(e.to_string()))
}

fn extract_aliases(clean_text: &str) -> Vec<(String, String)> {
    let mut aliases = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut start = None;

    for (index, ch) in clean_text.char_indices() {
        if start.is_none() && ch.is_ascii_uppercase() {
            start = Some(index);
            continue;
        }

        let in_token = start.is_some();
        if in_token && !(ch.is_ascii_alphanumeric() || ch == '_') {
            let token = &clean_text[start.expect("token start")..index];
            maybe_push_alias(token, &mut aliases, &mut seen);
            start = None;
        }
    }

    if let Some(start) = start {
        maybe_push_alias(&clean_text[start..], &mut aliases, &mut seen);
    }

    aliases
}

fn maybe_push_alias(
    token: &str,
    aliases: &mut Vec<(String, String)>,
    seen: &mut std::collections::BTreeSet<String>,
) {
    if !token.contains('_') || !seen.insert(token.to_string()) {
        return;
    }
    if let Some(external) = outward_placeholder(token) {
        aliases.push((external, token.to_string()));
    }
}

fn outward_placeholder(token: &str) -> Option<String> {
    let (prefix, suffix) = token.rsplit_once('_')?;
    if suffix.parse::<usize>().is_err() {
        return None;
    }

    let semantic = match prefix {
        "CustomerName" => Some(crate::context::CUSTOMER_NAME.to_string()),
        "CustomerEmail" => Some(crate::context::CUSTOMER_EMAIL.to_string()),
        "CustomerPhone" => Some(crate::context::CUSTOMER_PHONE.to_string()),
        _ => None,
    };
    if let Some(semantic) = semantic {
        return Some(semantic);
    }

    let upper = prefix
        .chars()
        .enumerate()
        .fold(String::new(), |mut acc, (index, ch)| {
            if ch.is_ascii_uppercase() && index != 0 {
                acc.push('_');
            }
            acc.push(ch.to_ascii_uppercase());
            acc
        });
    Some(format!("<{}_{}>", upper, suffix))
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
            ..Context::default()
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
        assert_eq!(blob.aliases[0].external, "<CUSTOMER_NAME>");
        assert_eq!(blob.aliases[1].external, "<CUSTOMER_EMAIL>");
    }

    #[test]
    fn metadata_lists_placeholders_in_insertion_order() {
        let text = "Markus Mueller at mueller.markus@icloud.com and other@x.com";
        let resp = sanitize(req(text, ctx_markus())).unwrap();
        assert_eq!(resp.metadata.placeholders[0], "<CUSTOMER_NAME>");
        assert_eq!(resp.metadata.placeholders[1], "<CUSTOMER_EMAIL>");
        assert!(resp.metadata.placeholders.iter().any(|p| p == "<EMAIL_1>"));
    }

    #[test]
    fn converts_internal_tokens_to_outward_placeholders() {
        assert_eq!(outward_placeholder("CustomerName_1").unwrap(), "<CUSTOMER_NAME>");
        assert_eq!(outward_placeholder("OrderId_3").unwrap(), "<ORDER_ID_3>");
        assert_eq!(outward_placeholder("Email_2").unwrap(), "<EMAIL_2>");
    }
}
