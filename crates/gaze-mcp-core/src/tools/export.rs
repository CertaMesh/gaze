//! `export_session_tokens` operator-tier tool. Dumps the current session's
//! token inventory, not the manifest/audit store.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{ResponseRedaction, Tool, ToolDescriptor, ToolError, ToolResponse};

/// `export_session_tokens` operator-tier tool. See module docs.
#[derive(Debug)]
#[non_exhaustive]
pub struct ExportSessionTokensTool {
    descriptor: ToolDescriptor,
}

impl ExportSessionTokensTool {
    /// Construct an `ExportSessionTokensTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::operator(
                "export_session_tokens",
                json!({
                    "type": "object",
                    "properties": {}
                }),
            )
            .with_description("Export the token/raw inventory for the current Session.")
            .with_response_redaction(ResponseRedaction::BypassByOperator),
        }
    }
}

impl Default for ExportSessionTokensTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExportSessionTokensTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        let session = ctx.resources().session();
        let mut entries = session
            .tokens()
            .into_iter()
            .map(|token| {
                let raw = session
                    .restore_strict(&token)
                    .map_err(ToolError::internal)?;
                let class = classify_token(&token)?;
                Ok(json!({
                    "token": token,
                    "raw": raw,
                    "class": class,
                }))
            })
            .collect::<Result<Vec<_>, ToolError>>()?;
        entries.sort_by(|left, right| left["token"].as_str().cmp(&right["token"].as_str()));
        let count = entries.len();
        Ok(ToolResponse::json(json!({
            "session_id": session.audit_session_id(),
            "entries": entries,
            "count": count,
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
    ) -> ToolCtx<'a> {
        ToolCtx::new_with_resources(
            SessionHandle::new("audit"),
            ToolResources::new(pipeline, session, manifest, &[]),
            json!({}),
            Ulid::new(),
            "export_session_tokens",
            "principal",
        )
    }

    #[tokio::test]
    async fn export_emits_session_token_inventory() {
        let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let token = session
            .tokenize(&gaze::PiiClass::Email, "alice@example.invalid")
            .expect("token");
        let manifest = NullManifest;
        let tool = ExportSessionTokensTool::new();

        let response = tool
            .invoke(&ctx(&pipeline, &session, &manifest))
            .await
            .expect("export response");

        assert_eq!(response.payload["count"], 1);
        assert_eq!(response.payload["entries"][0]["token"], token);
        assert_eq!(
            response.payload["entries"][0]["raw"],
            "alice@example.invalid"
        );
        assert_eq!(response.payload["entries"][0]["class"], "Email");
    }

    #[tokio::test]
    async fn export_returns_empty_for_fresh_session() {
        let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let manifest = NullManifest;
        let tool = ExportSessionTokensTool::new();

        let response = tool
            .invoke(&ctx(&pipeline, &session, &manifest))
            .await
            .expect("export response");

        assert_eq!(response.payload["count"], 0);
        assert_eq!(response.payload["entries"], json!([]));
    }

    #[tokio::test]
    async fn export_excludes_other_session_tokens() {
        let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let other = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let _other_token = other
            .tokenize(&gaze::PiiClass::Email, "bob@example.invalid")
            .expect("other token");
        let token = session
            .tokenize(&gaze::PiiClass::Email, "alice@example.invalid")
            .expect("token");
        let manifest = NullManifest;
        let tool = ExportSessionTokensTool::new();

        let response = tool
            .invoke(&ctx(&pipeline, &session, &manifest))
            .await
            .expect("export response");

        assert_eq!(response.payload["count"], 1);
        assert_eq!(response.payload["entries"][0]["token"], token);
    }
}
