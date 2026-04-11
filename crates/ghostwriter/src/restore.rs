//! Restore: strict exact token substitution.
//!
//! Per spec: restore only replaces exact placeholder tokens that exist in
//! the session blob. Paraphrased or invented text stays as written. Unused
//! placeholders in the blob become informational warnings.

use crate::blob::SessionBlob;
use crate::errors::RestoreError;
use crate::types::{RestoreRequest, RestoreResponse, Warning};

pub fn restore(req: RestoreRequest) -> Result<RestoreResponse, RestoreError> {
    let blob = SessionBlob::decode(&req.session_blob)?;

    let mut restored = req.text.clone();
    let mut used: Vec<String> = Vec::new();

    // Replace longest tokens first so that e.g. <EMAIL_11> is replaced
    // before <EMAIL_1> would be considered.
    let mut tokens: Vec<(&String, &String)> = blob.placeholders.iter().collect();
    tokens.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (token, raw) in tokens {
        if restored.contains(token.as_str()) {
            restored = restored.replace(token.as_str(), raw);
            used.push(token.clone());
        }
    }

    let mut warnings: Vec<Warning> = blob
        .placeholders
        .keys()
        .filter(|t| !used.contains(t))
        .map(|t| Warning::new(format!("placeholder {t} was not used")))
        .collect();

    // Look for placeholder-shaped tokens that survived because they are
    // NOT in the blob (e.g. the model invented <EMAIL_9>).
    for token in find_placeholder_tokens(&restored) {
        if !blob.placeholders.contains_key(&token) {
            warnings.push(Warning::new(format!(
                "unknown placeholder {token} left unchanged"
            )));
        }
    }

    Ok(RestoreResponse {
        restored_text: restored,
        warnings,
    })
}

/// Very small placeholder finder: matches `<UPPERCASE[_DIGITS_OR_UPPERCASE]*>`.
fn find_placeholder_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(end) = text[i..].find('>') {
                let token = &text[i..i + end + 1];
                let inner = &token[1..token.len() - 1];
                if !inner.is_empty()
                    && inner
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                {
                    out.push(token.to_string());
                }
                i += end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::SessionBlob;

    fn blob_with(pairs: &[(&str, &str)]) -> String {
        let mut b = SessionBlob::new();
        for (t, r) in pairs {
            b.insert(*t, *r);
        }
        b.encode().unwrap()
    }

    #[test]
    fn missing_session_blob_errors() {
        let err = restore(RestoreRequest {
            text: "hi".into(),
            session_blob: String::new(),
        })
        .unwrap_err();
        assert!(matches!(err, RestoreError::MissingSessionBlob));
    }

    #[test]
    fn corrupt_blob_errors() {
        let err = restore(RestoreRequest {
            text: "hi".into(),
            session_blob: "!!!not-base64!!!".into(),
        })
        .unwrap_err();
        assert!(matches!(err, RestoreError::InvalidSessionBlob(_)));
    }

    #[test]
    fn replaces_all_exact_placeholders() {
        let blob = blob_with(&[
            ("<CUSTOMER_NAME>", "Markus Mueller"),
            ("<CUSTOMER_EMAIL>", "mueller.markus@icloud.com"),
        ]);
        let resp = restore(RestoreRequest {
            text: "Hello <CUSTOMER_NAME>, we'll resend to <CUSTOMER_EMAIL>.".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(
            resp.restored_text,
            "Hello Markus Mueller, we'll resend to mueller.markus@icloud.com."
        );
        assert!(resp.warnings.is_empty());
    }

    #[test]
    fn unused_blob_placeholder_produces_warning() {
        let blob = blob_with(&[
            ("<CUSTOMER_NAME>", "Markus"),
            ("<EMAIL_1>", "a@x.com"),
        ]);
        let resp = restore(RestoreRequest {
            text: "Hello <CUSTOMER_NAME>".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(resp.restored_text, "Hello Markus");
        assert!(resp
            .warnings
            .iter()
            .any(|w| w.0.contains("<EMAIL_1>") && w.0.contains("not used")));
    }

    #[test]
    fn unknown_placeholder_shape_is_left_unchanged_with_warning() {
        let blob = blob_with(&[("<CUSTOMER_NAME>", "Markus")]);
        let resp = restore(RestoreRequest {
            text: "Hi <CUSTOMER_NAME>, contact <EMAIL_9>".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(resp.restored_text, "Hi Markus, contact <EMAIL_9>");
        assert!(resp
            .warnings
            .iter()
            .any(|w| w.0.contains("<EMAIL_9>") && w.0.contains("unknown")));
    }

    #[test]
    fn does_not_infer_from_nearby_words() {
        // The model drops the placeholder and writes "Markus" directly.
        // Restore must not touch "Markus" because it is not a token.
        let blob = blob_with(&[("<CUSTOMER_NAME>", "Markus Mueller")]);
        let resp = restore(RestoreRequest {
            text: "Hello Markus, see attached.".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(resp.restored_text, "Hello Markus, see attached.");
        assert!(resp.warnings.iter().any(|w| w.0.contains("not used")));
    }

    #[test]
    fn longer_tokens_replaced_before_shorter_prefixes() {
        let blob = blob_with(&[
            ("<EMAIL_1>", "a@x.com"),
            ("<EMAIL_11>", "k@y.com"),
        ]);
        let resp = restore(RestoreRequest {
            text: "write <EMAIL_11> then <EMAIL_1>".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(resp.restored_text, "write k@y.com then a@x.com");
    }
}
