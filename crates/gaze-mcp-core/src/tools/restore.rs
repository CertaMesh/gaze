//! `restore` operator-tier tool. Inverts the manifest: takes a redacted
//! string and returns the original PII for an authenticated operator.
//!
//! ## v0.1 scope
//!
//! Body is a stub returning `ToolError::Internal` with class
//! `"not-yet-implemented"`. The descriptor + tier + auth-hook routing is
//! wired and exercised by the Phase 9 fixtures, so adopters can register
//! the tool today and the upgraded body lands in a follow-up before v0.7
//! release.
//!
//! ## Why restore is hard inside the chokepoint
//!
//! `PiiEnvelope::dispatch` redacts the tool's args BEFORE invoke. A naive
//! restore tool would receive `{ "token": "<re-redacted>" }` — pointless,
//! because the original token has been mapped to a fresh token by the
//! incoming pass. Production restore therefore requires either (a) a
//! dispatcher path that opts out of arg-redaction for operator-tier tools,
//! or (b) an out-of-band channel for the token-to-restore. Locking that
//! design is the deferred work; everything else (tier gating, auth-hook
//! routing, fail-closed manifest) is solid and tested.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{ResponseRedaction, Tool, ToolDescriptor, ToolError, ToolResponse};

/// `restore` operator-tier tool. See module docs.
#[derive(Debug)]
pub struct RestoreTool {
    descriptor: ToolDescriptor,
}

impl RestoreTool {
    /// Construct a `RestoreTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::operator(
                "restore",
                json!({
                    "type": "object",
                    "properties": {
                        "token": { "type": "string", "description": "Token to restore." }
                    },
                    "required": ["token"]
                }),
            )
            .with_description("Operator-only: restore PII from a manifest token.")
            .with_response_redaction(ResponseRedaction::BypassByOperator),
        }
    }
}

impl Default for RestoreTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RestoreTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        let token = ctx
            .redacted_args()
            .get("token")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing required field `token`".to_string()))?;
        let raw = ctx
            .resources()
            .session()
            .restore(token)
            .ok_or_else(|| ToolError::NotFound(format!("token {token} not in session")))?;
        let class = classify_token(token)?;
        Ok(ToolResponse::json(json!({
            "raw": raw,
            "class": class,
        })))
    }
}

fn classify_token(token: &str) -> Result<String, ToolError> {
    if token.starts_with("email") && token.ends_with("@gaze-fake.invalid") {
        return Ok("Email".to_string());
    }
    let core = token
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(token);
    let after_session = if core.len() > 9
        && core.as_bytes()[8] == b':'
        && core[..8].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        &core[9..]
    } else {
        core
    };
    let class_part = after_session
        .rsplit_once('_')
        .map(|(class, _)| class)
        .ok_or_else(|| ToolError::internal(TokenClassError(token.to_string())))?;
    if let Some(custom) = class_part
        .strip_prefix("Custom:")
        .or_else(|| class_part.strip_prefix("custom:"))
    {
        return Ok(format!("Custom:{custom}"));
    }
    Ok(match class_part {
        "email" => "Email".to_string(),
        "name" => "Name".to_string(),
        "location" => "Location".to_string(),
        "organization" => "Organization".to_string(),
        class => class.to_string(),
    })
}

#[derive(Debug, thiserror::Error)]
#[error("could not classify token `{0}`")]
struct TokenClassError(String);

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
            "restore",
            "principal",
        )
    }

    #[tokio::test]
    async fn restore_emits_raw_payload_for_known_operator_token() {
        let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let token = session
            .tokenize(&gaze::PiiClass::Email, "alice@example.invalid")
            .expect("token");
        let manifest = NullManifest;
        let tool = RestoreTool::new();

        let response = tool
            .invoke(&ctx(
                &pipeline,
                &session,
                &manifest,
                json!({ "token": token }),
            ))
            .await
            .expect("restore response");

        assert_eq!(
            response.payload,
            json!({
                "raw": "alice@example.invalid",
                "class": "Email",
            })
        );
        assert_eq!(
            tool.descriptor().response_redaction(),
            ResponseRedaction::BypassByOperator
        );
    }
}
