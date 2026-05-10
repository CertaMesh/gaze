//! `restore_strict` operator-tier tool. Like `restore` but rejects partial
//! restorations — every token in the input string must be in the manifest
//! or the call fails.
//!
//! The body takes a `text` argument, scans it with `gaze::token_shape::pattern`,
//! and delegates restoration to the private `restore_strict_text` helper below.
//! If any token-shaped span is not owned by the session, the whole call fails
//! closed with `ToolError::NotFound`; otherwise the response bypasses agent
//! redaction under the operator-tier contract.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{ResponseRedaction, Tool, ToolDescriptor, ToolError, ToolResponse};

/// `restore_strict` operator-tier tool. See module docs.
#[derive(Debug)]
pub struct RestoreStrictTool {
    descriptor: ToolDescriptor,
}

impl RestoreStrictTool {
    /// Construct a `RestoreStrictTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::operator(
                "restore_strict",
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Text containing tokens to restore." }
                    },
                    "required": ["text"]
                }),
            )
            .with_description(
                "Operator-only: strict restore that fails if any token is missing.",
            )
            .with_response_redaction(ResponseRedaction::BypassByOperator),
        }
    }
}

impl Default for RestoreStrictTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RestoreStrictTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        let text = ctx
            .redacted_args()
            .get("text")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing required field `text`".into()))?;
        let restored = restore_strict_text(ctx.resources().session(), text)?;
        Ok(ToolResponse::json(json!({ "text": restored })))
    }
}

fn restore_strict_text(session: &gaze::Session, text: &str) -> Result<String, ToolError> {
    let mut restored = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for token in gaze::token_shape::pattern().find_iter(text) {
        restored.push_str(&text[cursor..token.start()]);
        let raw = session.restore(token.as_str()).ok_or_else(|| {
            ToolError::NotFound(format!("token {} not in session", token.as_str()))
        })?;
        restored.push_str(&raw);
        cursor = token.end();
    }
    restored.push_str(&text[cursor..]);
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ulid::Ulid;

    use crate::ctx::{SessionHandle, ToolResources};
    use crate::manifest::{
        BeginCallContext, CallHandle, FailureReason, ManifestError, ManifestStore, SnapshotRef,
    };

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
            ToolResources::new(pipeline, session, manifest, &[]),
            args,
            Ulid::new(),
            "restore_strict",
            "principal",
        )
    }

    #[tokio::test]
    async fn restore_strict_round_trips_clean_then_restore() {
        let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let token = session
            .tokenize(&gaze::PiiClass::Email, "alice@example.invalid")
            .expect("token");
        let manifest = NullManifest;
        let tool = RestoreStrictTool::new();

        let response = tool
            .invoke(&ctx(
                &pipeline,
                &session,
                &manifest,
                json!({ "text": format!("Hi {token}") }),
            ))
            .await
            .expect("restore response");

        assert_eq!(
            response.payload,
            json!({ "text": "Hi alice@example.invalid" })
        );
    }

    #[tokio::test]
    async fn restore_strict_fails_closed_on_unknown_token() {
        let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let manifest = NullManifest;
        let tool = RestoreStrictTool::new();

        let err = tool
            .invoke(&ctx(
                &pipeline,
                &session,
                &manifest,
                json!({ "text": "Hi <deadbeef:Email_1>" }),
            ))
            .await
            .expect_err("unknown token must fail");

        assert_eq!(err.class(), "not-found");
    }
}
