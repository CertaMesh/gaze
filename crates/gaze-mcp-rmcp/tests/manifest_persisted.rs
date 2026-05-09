use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use gaze_mcp_core::ctx::ToolCtx;
use gaze_mcp_core::manifest::{
    BeginCallContext, CallHandle, FailureReason, ManifestError, ManifestStore, SnapshotRef,
};
use gaze_mcp_core::registry::ToolRegistry;
use gaze_mcp_core::session_id::SessionIdPolicy;
use gaze_mcp_core::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};
use gaze_mcp_core::{AuthError, AuthHook, DispatchError, DispatchHost, PiiEnvelope, Principal};
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParam;
use serde_json::json;
use tokio::io::duplex;

struct EnvelopeHost {
    registry: ToolRegistry,
    auth: AllowAllAuthHook,
    manifest: FailingFinishManifest,
    pipeline: gaze::Pipeline,
    session: gaze::Session,
    session_id_policy: SessionIdPolicy,
}

#[async_trait]
impl DispatchHost for EnvelopeHost {
    async fn dispatch(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: serde_json::Value,
        external_session_id: Option<&str>,
    ) -> Result<ToolResponse, DispatchError> {
        let envelope = PiiEnvelope::new(
            &self.registry,
            &self.auth,
            &self.manifest,
            &self.pipeline,
            &self.session,
            &self.session_id_policy,
        );
        envelope
            .dispatch(principal, tool_name, raw_args, external_session_id)
            .await
    }

    fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.registry.list().into_iter().cloned().collect()
    }
}

struct FailingFinishManifest {
    begins: AtomicUsize,
    finishes: AtomicUsize,
    fails: AtomicUsize,
}

#[async_trait]
impl ManifestStore for FailingFinishManifest {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.begins.fetch_add(1, Ordering::SeqCst);
        Ok(CallHandle::new(ctx.call_id))
    }

    async fn finish_call(
        &self,
        _handle: CallHandle,
        _snapshot: SnapshotRef,
    ) -> Result<(), ManifestError> {
        self.finishes.fetch_add(1, Ordering::SeqCst);
        Err(ManifestError::backend(std::io::Error::other(
            "finish_call unavailable",
        )))
    }

    async fn fail_call(
        &self,
        _handle: CallHandle,
        _reason: FailureReason,
    ) -> Result<(), ManifestError> {
        self.fails.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct AllowAllAuthHook;

#[async_trait]
impl AuthHook for AllowAllAuthHook {
    async fn authorize_agent(
        &self,
        _principal: &Principal,
        _tool_name: &str,
    ) -> Result<(), AuthError> {
        Ok(())
    }

    async fn authorize_operator(
        &self,
        _principal: &Principal,
        _tool_name: &str,
    ) -> Result<(), AuthError> {
        Err(AuthError::Denied("operator tier disabled in test".into()))
    }
}

struct EchoTool {
    descriptor: ToolDescriptor,
}

#[async_trait]
impl Tool for EchoTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, _ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        Ok(ToolResponse::text("would-leak-if-returned"))
    }
}

#[tokio::test]
async fn finish_call_failure_returns_error_instead_of_tool_response() {
    let mut registry = ToolRegistry::new();
    registry
        .register(EchoTool {
            descriptor: ToolDescriptor::agent("echo", json!({ "type": "object" })),
        })
        .expect("register echo tool");

    let host = Arc::new(EnvelopeHost {
        registry,
        auth: AllowAllAuthHook,
        manifest: FailingFinishManifest {
            begins: AtomicUsize::new(0),
            finishes: AtomicUsize::new(0),
            fails: AtomicUsize::new(0),
        },
        pipeline: gaze::Pipeline::builder().build().expect("pipeline"),
        session: gaze::Session::new(gaze::Scope::Ephemeral).expect("session"),
        session_id_policy: SessionIdPolicy::default_strict(),
    });
    let frontend = RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::agent("stdio-test")));
    let handler = frontend.into_server_handler(host.clone());
    let (client_stream, server_stream) = duplex(16 * 1024);

    let server_task = tokio::spawn(async move {
        let running = rmcp::serve_server(handler, server_stream)
            .await
            .expect("server initializes");
        running.waiting().await.expect("server task joins")
    });

    let client = ().serve(client_stream).await.expect("client initializes");
    let result = client
        .call_tool(CallToolRequestParam {
            name: "echo".into(),
            arguments: json!({ "text": "hello" }).as_object().cloned(),
        })
        .await
        .expect("tool call returns rmcp result");

    assert_eq!(result.is_error, Some(true));
    let text = &result.content[0].raw.as_text().unwrap().text;
    assert_eq!(text, "manifest-persistence-failed");
    assert_ne!(text, "would-leak-if-returned");
    assert_eq!(host.manifest.begins.load(Ordering::SeqCst), 1);
    assert_eq!(host.manifest.finishes.load(Ordering::SeqCst), 1);
    assert_eq!(host.manifest.fails.load(Ordering::SeqCst), 0);

    client.cancel().await.expect("client cancels");
    server_task.await.expect("server finishes");
}
