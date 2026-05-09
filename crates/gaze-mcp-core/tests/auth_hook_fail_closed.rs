//! Asserts that the operator-tier surface fails closed when the adopter has
//! not wired up an authenticated [`AuthHook`].
//!
//! The test uses an inline operator-tier `Tool` rather than the built-in
//! `RestoreTool` so the path is exercised regardless of which crate features
//! are enabled.

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::json;

use gaze_mcp_core::auth::DenyAllAuthHook;
use gaze_mcp_core::ctx::ToolCtx;
use gaze_mcp_core::manifest::{
    BeginCallContext, CallHandle, FailureReason, ManifestError, ManifestStore, SnapshotRef,
};
use gaze_mcp_core::registry::ToolRegistry;
use gaze_mcp_core::session_id::SessionIdPolicy;
use gaze_mcp_core::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};
use gaze_mcp_core::{DispatchError, PiiEnvelope, Principal};

#[derive(Default)]
struct CountingManifest {
    begins: Mutex<u32>,
    finishes: Mutex<u32>,
    fails: Mutex<u32>,
}

#[async_trait]
impl ManifestStore for CountingManifest {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        *self.begins.lock().unwrap() += 1;
        Ok(CallHandle::new(ctx.call_id))
    }

    async fn finish_call(
        &self,
        _handle: CallHandle,
        _snapshot: SnapshotRef,
    ) -> Result<(), ManifestError> {
        *self.finishes.lock().unwrap() += 1;
        Ok(())
    }

    async fn fail_call(
        &self,
        _handle: CallHandle,
        _reason: FailureReason,
    ) -> Result<(), ManifestError> {
        *self.fails.lock().unwrap() += 1;
        Ok(())
    }
}

struct OperatorTool {
    descriptor: ToolDescriptor,
}

#[async_trait]
impl Tool for OperatorTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, _ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        Ok(ToolResponse::text("should never reach here"))
    }
}

#[tokio::test]
async fn operator_tool_dispatch_with_default_hook_returns_missing_hook() {
    let manifest = CountingManifest::default();
    let auth = DenyAllAuthHook;
    let policy = SessionIdPolicy::default_strict();
    let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
    let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");

    let mut registry = ToolRegistry::new();
    registry
        .register(OperatorTool {
            descriptor: ToolDescriptor::operator("restore", json!({"type": "object"})),
        })
        .unwrap();

    let envelope = PiiEnvelope::new(&registry, &auth, &manifest, &pipeline, &session, &policy);
    let err = envelope
        .dispatch(
            &Principal::new("test"),
            "restore",
            json!({"token": "<X:EMAIL_0>"}),
            None,
        )
        .await
        .expect_err("operator dispatch must fail closed without auth");

    // Auth failure surfaces as DispatchError::Auth(MissingHook).
    let display = err.to_string();
    assert!(
        display.contains("authorization"),
        "expected authorization error, got: {display}"
    );
    assert!(matches!(err, DispatchError::Auth(_)));

    // Manifest was never opened (auth failures stay pre-manifest).
    assert_eq!(*manifest.begins.lock().unwrap(), 0);
    assert_eq!(*manifest.finishes.lock().unwrap(), 0);
    assert_eq!(*manifest.fails.lock().unwrap(), 0);
}
