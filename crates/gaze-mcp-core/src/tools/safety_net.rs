//! `safety_net_check` agent-tier tool. Reports whether the redacted args
//! contain any residual PII the gaze safety-net pass-3 detected.
//!
use async_trait::async_trait;
use serde_json::json;
use std::collections::BTreeMap;

use crate::ctx::ToolCtx;
use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};

/// `safety_net_check` agent-tier tool. See module docs.
#[derive(Debug)]
pub struct SafetyNetCheckTool {
    descriptor: ToolDescriptor,
}

impl SafetyNetCheckTool {
    /// Construct a `SafetyNetCheckTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::agent(
                "safety_net_check",
                json!({
                    "type": "object",
                    "oneOf": [
                        { "required": ["text"], "not": { "required": ["document"] } },
                        { "required": ["document"], "not": { "required": ["text"] } }
                    ],
                    "properties": {
                        "text": { "type": "string", "description": "Text to scan." },
                        "document": { "type": "object", "description": "Structured document to scan." }
                    }
                }),
            )
            .with_description("Run the gaze safety-net pass against text and report residual PII."),
        }
    }
}

impl Default for SafetyNetCheckTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SafetyNetCheckTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        let resources = ctx.resources();
        let result = match (
            ctx.redacted_args().get("text"),
            ctx.redacted_args().get("document"),
        ) {
            (Some(text), None) => {
                let text = text
                    .as_str()
                    .ok_or_else(|| ToolError::InvalidArgs("`text` must be a string".into()))?;
                resources
                    .pipeline()
                    .scan_safety_nets(resources.session(), text, resources.locale_chain())
                    .map_err(ToolError::internal)?
            }
            (None, Some(document)) => {
                let document = decode_structured_document(document)?;
                resources
                    .pipeline()
                    .scan_safety_nets_structured(
                        resources.session(),
                        &document,
                        resources.locale_chain(),
                    )
                    .map_err(ToolError::internal)?
            }
            _ => {
                return Err(ToolError::InvalidArgs(
                    "exactly one of `text` or `document` is required".into(),
                ));
            }
        };
        let ok = if result.nets_run == 0 {
            serde_json::Value::String("unconfigured".into())
        } else {
            serde_json::Value::Bool(result.report.suspects.is_empty())
        };
        let leaks = result
            .report
            .suspects
            .iter()
            .map(|suspect| {
                json!({
                    "class": suspect.class.class_name(),
                    "score": suspect.score,
                    "kind": format!("{:?}", suspect.kind),
                    "safety_net_id": suspect.safety_net_id,
                    "field_path": suspect.field_path,
                    "span": [suspect.span.start, suspect.span.end],
                })
            })
            .collect::<Vec<_>>();
        let stats = serde_json::to_value(&result.report.stats).map_err(ToolError::internal)?;

        Ok(ToolResponse::json(json!({
            "ok": ok,
            "nets_run": result.nets_run,
            "leak_count": result.report.suspects.len(),
            "leaks": leaks,
            "stats": stats,
        })))
    }
}

fn decode_structured_document(
    value: &serde_json::Value,
) -> Result<BTreeMap<String, gaze::Value>, ToolError> {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(key, value)| Ok((key.clone(), decode_value(value)?)))
            .collect(),
        _ => Err(ToolError::InvalidArgs(
            "`document` must be an object".into(),
        )),
    }
}

fn decode_value(value: &serde_json::Value) -> Result<gaze::Value, ToolError> {
    match value {
        serde_json::Value::Null => Ok(gaze::Value::Null),
        serde_json::Value::Bool(value) => Ok(gaze::Value::Bool(*value)),
        serde_json::Value::String(value) => Ok(gaze::Value::String(value.clone())),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(gaze::Value::I64)
            .ok_or_else(|| ToolError::InvalidArgs("`document` numbers must be i64".into())),
        serde_json::Value::Array(values) => values
            .iter()
            .map(decode_value)
            .collect::<Result<Vec<_>, _>>()
            .map(gaze::Value::Array),
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(key, value)| Ok((key.clone(), decode_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(gaze::Value::Object),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use gaze::{
        DocumentKind, LeakKind, LeakSuspect, PiiClass, SafetyNet, SafetyNetContext, SafetyNetError,
    };
    use std::sync::{Arc, Mutex};
    use ulid::Ulid;

    use crate::ctx::{SessionHandle, ToolResources};
    use crate::manifest::{
        BeginCallContext, CallHandle, FailureReason, ManifestError, ManifestStore, SnapshotRef,
    };

    type SeenChecks = Arc<Mutex<Vec<(String, Option<String>, DocumentKind)>>>;

    #[derive(Clone)]
    struct MockNet {
        span: Option<std::ops::Range<usize>>,
        field_path: Option<&'static str>,
        seen: SeenChecks,
    }

    impl MockNet {
        fn new(span: Option<std::ops::Range<usize>>) -> Self {
            Self {
                span,
                field_path: None,
                seen: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_field_path(mut self, field_path: &'static str) -> Self {
            self.field_path = Some(field_path);
            self
        }
    }

    impl SafetyNet for MockNet {
        fn id(&self) -> &str {
            "mock"
        }

        fn supported_locales(&self) -> &[gaze::LocaleTag] {
            &[gaze::LocaleTag::Global]
        }

        fn check(
            &self,
            clean_text: &str,
            context: SafetyNetContext<'_>,
        ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
            self.seen.lock().unwrap().push((
                clean_text.to_string(),
                context.field_path.map(str::to_string),
                context.document_kind,
            ));
            if self.field_path.is_some() && self.field_path != context.field_path {
                return Ok(Vec::new());
            }
            let Some(span) = self.span.clone() else {
                return Ok(Vec::new());
            };
            Ok(vec![LeakSuspect::new(
                span,
                PiiClass::Email,
                self.id(),
                Some(0.99),
                LeakKind::Uncovered,
                "private_email",
                context.field_path.map(str::to_string),
            )])
        }
    }

    struct NullManifest;

    #[async_trait]
    impl ManifestStore for NullManifest {
        async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
            Ok(CallHandle::new(ctx.call_id))
        }

        async fn finish_call(
            &self,
            _handle: CallHandle,
            _snapshot: SnapshotRef,
        ) -> Result<(), ManifestError> {
            Ok(())
        }

        async fn fail_call(
            &self,
            _handle: CallHandle,
            _reason: FailureReason,
        ) -> Result<(), ManifestError> {
            Ok(())
        }
    }

    fn ctx<'a>(
        pipeline: &'a gaze::Pipeline,
        session: &'a gaze::Session,
        manifest: &'a dyn ManifestStore,
        args: serde_json::Value,
    ) -> ToolCtx<'a> {
        ToolCtx::new_with_resources(
            SessionHandle::new("audit"),
            ToolResources::new(pipeline, session, manifest, &[gaze::LocaleTag::Global]),
            args,
            Ulid::new(),
            "safety_net_check",
            "principal",
        )
    }

    #[tokio::test]
    async fn safety_net_check_returns_leaks_for_unprotected_email() {
        let pipeline = gaze::Pipeline::builder()
            .register_safety_net(MockNet::new(Some(0.."alice@example.invalid".len())))
            .build()
            .expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let manifest = NullManifest;
        let tool = SafetyNetCheckTool::new();

        let response = tool
            .invoke(&ctx(
                &pipeline,
                &session,
                &manifest,
                json!({"text": "alice@example.invalid"}),
            ))
            .await
            .expect("tool response");

        assert_eq!(response.payload["ok"], false);
        assert_eq!(response.payload["nets_run"], 1);
        assert_eq!(response.payload["leak_count"], 1);
        assert_eq!(response.payload["leaks"][0]["class"], "Email");
        assert!(response.payload["leaks"][0].get("raw_label").is_none());
    }

    #[tokio::test]
    async fn safety_net_check_returns_unconfigured_when_no_nets() {
        let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let manifest = NullManifest;
        let tool = SafetyNetCheckTool::new();

        let response = tool
            .invoke(&ctx(
                &pipeline,
                &session,
                &manifest,
                json!({"text": "alice@example.invalid"}),
            ))
            .await
            .expect("tool response");

        assert_eq!(response.payload["ok"], "unconfigured");
        assert_eq!(response.payload["nets_run"], 0);
    }

    #[tokio::test]
    async fn safety_net_check_structured_walks_value_leaves() {
        let net =
            MockNet::new(Some(0.."alice@example.invalid".len())).with_field_path("profile.email");
        let seen = net.seen.clone();
        let pipeline = gaze::Pipeline::builder()
            .register_safety_net(net)
            .build()
            .expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let manifest = NullManifest;
        let tool = SafetyNetCheckTool::new();

        let response = tool
            .invoke(&ctx(
                &pipeline,
                &session,
                &manifest,
                json!({"document": {"profile": {"email": "alice@example.invalid"}, "customer_id": 42}}),
            ))
            .await
            .expect("tool response");

        assert_eq!(response.payload["ok"], false);
        assert_eq!(response.payload["leaks"][0]["field_path"], "profile.email");
        let seen = seen.lock().unwrap();
        assert!(seen
            .iter()
            .any(|(value, path, _)| value == "42" && path.as_deref() == Some("customer_id")));
    }
}
