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
use rmcp::model::CallToolRequestParams;
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
            &[],
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
    reject_failure: bool,
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
        if self.reject_failure {
            return Err(ManifestError::backend(std::io::Error::other(ERROR_DETAIL)));
        }
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
            descriptor: ToolDescriptor::agent("echo", json!({ "type": "object" })).with_carriers(
                gaze_mcp_core::CarrierDeclaration::text_fields(&["text"]),
                Default::default(),
            ),
        })
        .expect("register echo tool");

    let host = Arc::new(EnvelopeHost {
        registry,
        auth: AllowAllAuthHook,
        manifest: FailingFinishManifest {
            begins: AtomicUsize::new(0),
            finishes: AtomicUsize::new(0),
            fails: AtomicUsize::new(0),
            reject_failure: false,
        },
        pipeline: gaze_assembly::CorePipelineConfig::new()
            .build()
            .expect("pipeline")
            .into_pipeline(),
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
    let args = json!({ "text": "hello" })
        .as_object()
        .cloned()
        .expect("arguments are object");
    let result = client
        .call_tool(CallToolRequestParams::new("echo").with_arguments(args))
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

const ERROR_DETAIL: &str =
    "/synthetic/alice@example.invalid/private.txt: Dr. Schmidt backend failed";
const ERROR_CLASSES: [&str; 6] = [
    "invalid-args",
    "not-found",
    "limit-exceeded",
    "backend-unavailable",
    "backend-failure",
    "internal",
];

struct ErrorTool {
    descriptor: ToolDescriptor,
    variant: usize,
}

#[async_trait]
impl Tool for ErrorTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, _ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        Err(match self.variant {
            0 => ToolError::InvalidArgs(ERROR_DETAIL.into()),
            1 => ToolError::NotFound(ERROR_DETAIL.into()),
            2 => ToolError::LimitExceeded(ERROR_DETAIL.into()),
            3 => ToolError::BackendUnavailable(ERROR_DETAIL.into()),
            4 => ToolError::BackendFailure(ERROR_DETAIL.into()),
            5 => ToolError::internal(std::io::Error::other(ERROR_DETAIL)),
            _ => unreachable!("test variant"),
        })
    }
}

async fn assert_tool_error_egress(reject_failure: bool) {
    let mut registry = ToolRegistry::new();
    for (variant, class) in ERROR_CLASSES.iter().enumerate() {
        registry
            .register(ErrorTool {
                descriptor: ToolDescriptor::agent(*class, json!({ "type": "object" })),
                variant,
            })
            .expect("register error tool");
    }
    let host = Arc::new(EnvelopeHost {
        registry,
        auth: AllowAllAuthHook,
        manifest: FailingFinishManifest {
            begins: AtomicUsize::new(0),
            finishes: AtomicUsize::new(0),
            fails: AtomicUsize::new(0),
            reject_failure,
        },
        pipeline: gaze_assembly::CorePipelineConfig::new()
            .build()
            .expect("pipeline")
            .into_pipeline(),
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
    for (index, class) in ERROR_CLASSES.iter().enumerate() {
        let result = client
            .call_tool(CallToolRequestParams::new(*class))
            .await
            .expect("tool error is an MCP result, not a protocol error");
        let wire = serde_json::to_value(&result).expect("result serializes");
        assert!(
            !wire.to_string().contains(ERROR_DETAIL),
            "leaked {class} detail"
        );
        let expected = if reject_failure {
            "manifest-persistence-failed"
        } else {
            class
        };
        assert_eq!(
            wire,
            json!({
                "content": [{ "type": "text", "text": expected }],
                "isError": true
            })
        );
        // The failure must be persisted (or rejected) before the client sees any result.
        assert_eq!(host.manifest.begins.load(Ordering::SeqCst), index + 1);
        assert_eq!(host.manifest.fails.load(Ordering::SeqCst), index + 1);
        assert_eq!(host.manifest.finishes.load(Ordering::SeqCst), 0);
    }
    client.cancel().await.expect("client cancels");
    server_task.await.expect("server finishes");
}

#[tokio::test]
async fn tool_error_egress_is_class_only_after_manifest_failure_recorded() {
    assert_tool_error_egress(false).await;
}

#[tokio::test]
async fn fail_call_failure_takes_precedence_over_tool_error() {
    assert_tool_error_egress(true).await;
}
