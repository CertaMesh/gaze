//! Public JSON data contracts.
//!
//! These types mirror the spec exactly. Field names use snake_case so
//! they match the JSON on the wire without serde rename attributes.

use serde::{Deserialize, Serialize};

/// Known primary customer identity supplied by the caller.
/// Every field is optional — callers pass only what they know.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_phone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SanitizeRequest {
    pub text: String,
    #[serde(default)]
    pub context: Context,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SanitizeResponse {
    pub clean_text: String,
    pub session_blob: String,
    #[serde(default)]
    pub warnings: Vec<Warning>,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metadata {
    pub placeholders: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreRequest {
    pub text: String,
    pub session_blob: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreResponse {
    pub restored_text: String,
    #[serde(default)]
    pub warnings: Vec<Warning>,
}

/// Informational warning. Always serialized as a plain string so callers
/// can render them directly without schema knowledge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Warning(pub String);

impl Warning {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_request_roundtrip_matches_spec_example() {
        let json = r#"{
            "text": "Hi Markus Mueller here",
            "context": {
                "customer_name": "Markus Mueller",
                "customer_email": "mueller.markus@icloud.com",
                "customer_phone": "+49 151 23456789"
            }
        }"#;

        let req: SanitizeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hi Markus Mueller here");
        assert_eq!(req.context.customer_name.as_deref(), Some("Markus Mueller"));
        assert_eq!(
            req.context.customer_email.as_deref(),
            Some("mueller.markus@icloud.com")
        );
        assert_eq!(
            req.context.customer_phone.as_deref(),
            Some("+49 151 23456789")
        );
    }

    #[test]
    fn sanitize_response_serializes_placeholders_metadata() {
        let resp = SanitizeResponse {
            clean_text: "Hi <CUSTOMER_NAME>".into(),
            session_blob: "abc".into(),
            warnings: vec![],
            metadata: Metadata {
                placeholders: vec!["<CUSTOMER_NAME>".into()],
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["clean_text"], "Hi <CUSTOMER_NAME>");
        assert_eq!(json["session_blob"], "abc");
        assert_eq!(json["metadata"]["placeholders"][0], "<CUSTOMER_NAME>");
    }

    #[test]
    fn restore_request_requires_text_and_blob() {
        let json = r#"{"text":"Hi <CUSTOMER_NAME>","session_blob":"abc"}"#;
        let req: RestoreRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hi <CUSTOMER_NAME>");
        assert_eq!(req.session_blob, "abc");
    }

    #[test]
    fn context_fields_all_optional() {
        let json = r#"{}"#;
        let ctx: Context = serde_json::from_str(json).unwrap();
        assert!(ctx.customer_name.is_none());
        assert!(ctx.customer_email.is_none());
        assert!(ctx.customer_phone.is_none());
    }
}
