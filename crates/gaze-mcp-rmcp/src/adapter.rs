//! rmcp ↔ gaze-mcp-core wire-type translation.
//!
//! Pure translation only. No I/O, no state, no chokepoint. The chokepoint
//! lives in `gaze_mcp_core::PiiEnvelope::dispatch`; this module just shapes
//! requests and responses so the rmcp `Frontend` can hand them to the
//! `DispatchHost` and serialize results back over the wire.

use std::borrow::Cow;
use std::sync::Arc;

use gaze_mcp_core::{ToolDescriptor, ToolError, ToolResponse, ToolTier};
use rmcp::model::{
    AnnotateAble, CallToolRequestParams, CallToolResult, Content, JsonObject, RawContent, Tool,
};
use serde_json::Value;

use crate::error::RmcpFrontendError;

/// Argument key used to thread an MCP-side session id through `tools/call`.
///
/// rmcp's wire format does not natively carry a Gaze session identifier, so
/// the rmcp adapter looks for a top-level `_session_id` argument key. When
/// present, it is extracted before forwarding the remaining arguments to
/// `DispatchHost::dispatch` and the value is validated by
/// `gaze_mcp_core::SessionIdPolicy` inside dispatch.
pub const SESSION_ID_ARG_KEY: &str = "_session_id";

/// Translate a `gaze_mcp_core::ToolDescriptor` into an `rmcp::model::Tool`
/// suitable for inclusion in a `tools/list` response.
///
/// The `ToolTier` (Agent vs Operator) is intentionally *not* embedded in the
/// rmcp descriptor: tier is a server-side authorization concern enforced by
/// `AuthHook` before dispatch, and exposing it on the wire would leak the
/// existence of operator-tier tools to agent-tier clients. Frontend code is
/// responsible for filtering operator-tier descriptors out of `list_tools`
/// for unauthorized principals.
pub fn descriptor_to_rmcp_tool(descriptor: &ToolDescriptor) -> Tool {
    let mut tool = Tool::new_with_raw(
        descriptor.name().to_string(),
        descriptor
            .description()
            .map(|description| Cow::Owned::<str>(description.to_string())),
        Arc::new(json_value_to_object(descriptor.schema())),
    );
    if let Some(output_schema) = descriptor.output_schema() {
        tool = tool.with_raw_output_schema(Arc::new(json_value_to_object(output_schema)));
    }
    tool
}

/// Whether a descriptor should be visible on the wire to a given tier.
///
/// Operator-tier tools are filtered out of `tools/list` for agent-tier
/// principals. The dispatch path still enforces tier through `AuthHook`,
/// but list filtering avoids advertising tools the principal cannot call.
pub fn descriptor_visible_to(descriptor: &ToolDescriptor, principal_tier: ToolTier) -> bool {
    match (descriptor.tier(), principal_tier) {
        (ToolTier::Agent, _) => true,
        (ToolTier::Operator, ToolTier::Operator) => true,
        (ToolTier::Operator, ToolTier::Agent) => false,
        // ToolTier is `#[non_exhaustive]` in gaze-mcp-core. Default any
        // future tier to "operator-only" — fail closed on unknown tiers
        // rather than expose tools to agent-tier principals.
        (ToolTier::Operator, _) => false,
        (_, ToolTier::Operator) => false,
        (_, _) => false,
    }
}

/// Outcome of pulling apart an inbound `tools/call` request.
///
/// `tool_name` and `args` are forwarded to `DispatchHost::dispatch`.
/// `session_id`, if present, is supplied as the dispatch's external session
/// identifier and validated by `SessionIdPolicy` before the chokepoint runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchArgs {
    pub tool_name: String,
    pub args: Value,
    pub session_id: Option<String>,
}

/// Translate an rmcp `CallToolRequestParams` into the inputs `DispatchHost::dispatch` expects.
///
/// The argument map is converted into a `serde_json::Value::Object` so the
/// tool implementation sees the same shape it would see when accepting JSON
/// directly. If the request omits `arguments`, the call is treated as having
/// an empty object, which is what rmcp's spec implies.
pub fn rmcp_args_to_dispatch_args(
    request: CallToolRequestParams,
) -> Result<DispatchArgs, RmcpFrontendError> {
    let CallToolRequestParams {
        name, arguments, ..
    } = request;
    let mut map = arguments.unwrap_or_default();
    let session_id = match map.remove(SESSION_ID_ARG_KEY) {
        Some(Value::String(s)) => Some(s),
        Some(Value::Null) | None => None,
        Some(other) => {
            return Err(RmcpFrontendError::MalformedRequest(format!(
                "{SESSION_ID_ARG_KEY} must be a string, got {}",
                json_kind(&other)
            )));
        }
    };
    Ok(DispatchArgs {
        tool_name: name.into_owned(),
        args: Value::Object(map),
        session_id,
    })
}

/// Translate a successful `gaze_mcp_core::ToolResponse` into an rmcp
/// `CallToolResult { is_error: false }`.
///
/// String payloads are passed through as `Content::text` (the natural shape
/// for human-readable model output). All other JSON values are serialized
/// to a single text-content frame via `RawContent::json`, matching rmcp's
/// convention for structured tool output.
pub fn response_to_rmcp_call_tool_result(
    response: ToolResponse,
) -> Result<CallToolResult, RmcpFrontendError> {
    let content = match response.payload {
        Value::String(text) => Content::text(text),
        other => RawContent::json(other)
            .map_err(|err| RmcpFrontendError::Internal(Box::new(err)))?
            .no_annotation(),
    };
    Ok(CallToolResult::success(vec![content]))
}

/// Translate a `gaze_mcp_core::ToolError` into an rmcp
/// `CallToolResult { is_error: true }`.
///
/// The error message is rendered as a single text frame and prefixed with
/// the stable error class (`invalid-args`, `not-found`, `internal`) so
/// consumers can branch on class without parsing free text.
pub fn error_to_rmcp_call_tool_result(err: ToolError) -> CallToolResult {
    let class = err.class();
    let message = match err {
        ToolError::InvalidArgs(msg) => msg,
        ToolError::NotFound(msg) => msg,
        ToolError::Internal(source) => source.to_string(),
        // `ToolError` is `#[non_exhaustive]`. Future variants fall back to
        // their `class()` string — the chokepoint contract guarantees a
        // stable class for every variant, so this preserves observability
        // even before the adapter knows the variant by name.
        _ => class.to_string(),
    };
    CallToolResult::error(vec![Content::text(format!("{class}: {message}"))])
}

fn json_value_to_object(value: &Value) -> JsonObject {
    match value {
        Value::Object(map) => map.clone(),
        // MCP requires the input schema to be a JSON object; for non-object
        // schemas we wrap into a single-key envelope so the wire payload is
        // syntactically valid. Tool authors should always supply objects.
        other => {
            let mut map = JsonObject::new();
            map.insert("schema".to_string(), other.clone());
            map
        }
    }
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_mcp_core::ResponseRedaction;
    use serde_json::json;

    fn make_descriptor() -> ToolDescriptor {
        ToolDescriptor::agent("clean", json!({ "type": "object", "properties": {} }))
            .with_description("redact PII")
    }

    #[test]
    fn descriptor_round_trips_name_description_and_schema() {
        let d = make_descriptor();
        let t = descriptor_to_rmcp_tool(&d);
        assert_eq!(t.name, "clean");
        assert_eq!(t.description.as_deref(), Some("redact PII"));
        assert_eq!(t.input_schema.get("type"), Some(&json!("object")));
        assert!(t.annotations.is_none());
    }

    #[test]
    fn descriptor_does_not_leak_tier_to_wire() {
        let agent = ToolDescriptor::agent("a", json!({ "type": "object" }));
        let operator = ToolDescriptor::operator("o", json!({ "type": "object" }));
        let agent_wire = descriptor_to_rmcp_tool(&agent);
        let operator_wire = descriptor_to_rmcp_tool(&operator);
        // The wire-level tool descriptors should be structurally interchangeable
        // — tier lives only in the server-side ToolDescriptor, never on the wire.
        assert_eq!(agent_wire.annotations, operator_wire.annotations);
    }

    #[test]
    fn descriptor_preserves_output_schema_without_response_redaction_wire_field() {
        let d = ToolDescriptor::operator("restore", json!({ "type": "object" }))
            .with_output_schema(json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                }
            }))
            .with_response_redaction(ResponseRedaction::BypassByOperator);
        let t = descriptor_to_rmcp_tool(&d);
        assert_eq!(
            t.output_schema
                .as_ref()
                .and_then(|schema| schema.get("type")),
            Some(&json!("object"))
        );

        let wire = serde_json::to_value(&t).expect("tool serializes");
        assert!(wire.get("outputSchema").is_some());
        assert!(wire.get("responseRedaction").is_none());
    }

    #[test]
    fn descriptor_visibility_filters_operator_for_agent_principals() {
        let agent = ToolDescriptor::agent("a", json!({}));
        let operator = ToolDescriptor::operator("o", json!({}));
        assert!(descriptor_visible_to(&agent, ToolTier::Agent));
        assert!(descriptor_visible_to(&agent, ToolTier::Operator));
        assert!(!descriptor_visible_to(&operator, ToolTier::Agent));
        assert!(descriptor_visible_to(&operator, ToolTier::Operator));
    }

    #[test]
    fn rmcp_args_extract_tool_name_and_session_id() {
        let mut args = JsonObject::new();
        args.insert("text".to_string(), json!("hello"));
        args.insert(
            SESSION_ID_ARG_KEY.to_string(),
            json!("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
        );
        let req = CallToolRequestParams::new("clean").with_arguments(args);
        let out = rmcp_args_to_dispatch_args(req).expect("translation should succeed");
        assert_eq!(out.tool_name, "clean");
        assert_eq!(
            out.session_id.as_deref(),
            Some("01ARZ3NDEKTSV4RRFFQ69G5FAV")
        );
        assert_eq!(out.args, json!({ "text": "hello" }));
    }

    #[test]
    fn rmcp_args_default_to_empty_object_when_arguments_missing() {
        let req = CallToolRequestParams::new("clean");
        let out = rmcp_args_to_dispatch_args(req).unwrap();
        assert_eq!(out.session_id, None);
        assert_eq!(out.args, json!({}));
    }

    #[test]
    fn rmcp_args_reject_non_string_session_id() {
        let mut args = JsonObject::new();
        args.insert(SESSION_ID_ARG_KEY.to_string(), json!(42));
        let req = CallToolRequestParams::new("clean").with_arguments(args);
        let err = rmcp_args_to_dispatch_args(req).expect_err("number sid must be rejected");
        assert!(matches!(err, RmcpFrontendError::MalformedRequest(_)));
        assert_eq!(err.class(), "malformed-request");
    }

    #[test]
    fn response_string_payload_becomes_text_content() {
        let resp = ToolResponse::text("[EMAIL_1]");
        let out = response_to_rmcp_call_tool_result(resp).unwrap();
        assert_eq!(out.is_error, Some(false));
        assert_eq!(out.content.len(), 1);
        let text = out.content[0]
            .raw
            .as_text()
            .expect("string payload should serialize as text content")
            .text
            .clone();
        assert_eq!(text, "[EMAIL_1]");
    }

    #[test]
    fn response_object_payload_becomes_json_text_content() {
        let resp = ToolResponse::json(json!({ "redacted": "[NAME_1]" }));
        let out = response_to_rmcp_call_tool_result(resp).unwrap();
        assert_eq!(out.is_error, Some(false));
        // RawContent::json serializes the value into a text frame; assert
        // the round-trip survives by parsing the embedded JSON back.
        let text = &out.content[0].raw.as_text().unwrap().text;
        let parsed: Value = serde_json::from_str(text).expect("json round-trip");
        assert_eq!(parsed, json!({ "redacted": "[NAME_1]" }));
    }

    #[test]
    fn tool_error_translates_to_error_call_tool_result_with_class_prefix() {
        let err = ToolError::InvalidArgs("missing field text".into());
        let result = error_to_rmcp_call_tool_result(err);
        assert_eq!(result.is_error, Some(true));
        let text = &result.content[0].raw.as_text().unwrap().text;
        assert!(text.starts_with("invalid-args:"), "got: {text}");
        assert!(text.contains("missing field text"));
    }

    #[test]
    fn tool_error_internal_renders_source_message() {
        let inner: Box<dyn std::error::Error + Send + Sync> = "manifest persist failed".into();
        let err = ToolError::Internal(inner);
        let result = error_to_rmcp_call_tool_result(err);
        let text = &result.content[0].raw.as_text().unwrap().text;
        assert!(text.starts_with("internal:"), "got: {text}");
        assert!(text.contains("manifest persist failed"));
    }
}
