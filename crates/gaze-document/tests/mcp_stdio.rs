#![cfg(all(feature = "mcp", feature = "ocr-tesseract", feature = "pdf-input"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use gaze_mcp_core::manifest::{
    BeginCallContext, CallHandle, FailureReason, ManifestError, ManifestStore, SnapshotRef,
};
use gaze_mcp_core::session_id::SessionIdPolicy;
use gaze_mcp_core::{
    AuthError, AuthHook, DispatchError, DispatchHost, PiiEnvelope, Principal, ToolDescriptor,
    ToolResponse,
};
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use serde_json::{json, Value};
use tokio::io::duplex;

struct AllowAllAuth;

#[async_trait]
impl AuthHook for AllowAllAuth {
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

struct RecordingManifest {
    begins: AtomicUsize,
    finishes: AtomicUsize,
    fails: AtomicUsize,
}

impl RecordingManifest {
    fn new() -> Self {
        Self {
            begins: AtomicUsize::new(0),
            finishes: AtomicUsize::new(0),
            fails: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ManifestStore for RecordingManifest {
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
        Ok(())
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

struct EnvelopeHost {
    registry: gaze_mcp_core::ToolRegistry,
    auth: AllowAllAuth,
    manifest: Arc<RecordingManifest>,
    pipeline: gaze::Pipeline,
    session: gaze::Session,
    session_id_policy: SessionIdPolicy,
}

impl EnvelopeHost {
    fn new() -> Self {
        let mut registry = gaze_mcp_core::ToolRegistry::new();
        gaze_document::mcp::register_tools(
            &mut registry,
            gaze_document::mcp::GazeReadOpts::default(),
        )
        .expect("register document tools");
        Self {
            registry,
            auth: AllowAllAuth,
            manifest: Arc::new(RecordingManifest::new()),
            pipeline: gaze::Pipeline::builder().build().expect("pipeline"),
            session: gaze::Session::new(gaze::Scope::Ephemeral).expect("session"),
            session_id_policy: SessionIdPolicy::default_strict(),
        }
    }
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
            self.manifest.as_ref(),
            &self.pipeline,
            &self.session,
            &[gaze::LocaleTag::Global],
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

fn synthetic_image_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("synthetic_image.png")
}

fn parse_success_payload(result: rmcp::model::CallToolResult) -> Value {
    assert_eq!(result.is_error, Some(false));
    let text = &result.content[0].raw.as_text().unwrap().text;
    serde_json::from_str(text).expect("tool response json")
}

fn assert_clean_response(payload: &Value, expect_source_kind: &str) {
    let clean = payload["clean_markdown"]
        .as_str()
        .expect("clean_markdown string");
    assert!(clean.contains(":Email_"), "{clean}");
    assert!(clean.contains(":Name_"), "{clean}");
    assert!(clean.contains(":Custom:phone_"), "{clean}");
    assert!(!clean.contains("Jane Doe"), "{clean}");
    assert!(!clean.contains("@example.invalid"), "{clean}");
    assert!(!clean.contains("555-0142"), "{clean}");
    assert!(!payload["manifest_id"].as_str().unwrap().is_empty());
    assert_eq!(payload["file_metadata"]["source_kind"], expect_source_kind);
    assert_eq!(payload["file_metadata"]["bundle_version"], json!(2));
}

#[tokio::test]
async fn stdio_document_tools_return_clean_payloads() {
    let host = Arc::new(EnvelopeHost::new());
    let frontend = RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::agent("stdio-test")));
    let handler = frontend.into_server_handler(host.clone());
    let (client_stream, server_stream) = duplex(64 * 1024);

    let server_task = tokio::spawn(async move {
        let running = rmcp::serve_server(handler, server_stream)
            .await
            .expect("server initializes");
        running.waiting().await.expect("server task joins")
    });

    let client = ().serve(client_stream).await.expect("client initializes");
    let tools = client
        .list_tools(Default::default())
        .await
        .expect("list tools");
    let tool_names: Vec<_> = tools.tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert!(tool_names.contains(&"gaze_read_text"));
    assert!(tool_names.contains(&"gaze_read_file"));

    let text_args = json!({
        "text": "Bill to: Jane Doe\nEmail: jane.doe@example.invalid\nPhone: +1-555-0142"
    })
    .as_object()
    .cloned()
    .expect("arguments are object");
    let text_result = client
        .call_tool(CallToolRequestParams::new("gaze_read_text").with_arguments(text_args))
        .await
        .expect("text tool call succeeds");
    let text_payload = parse_success_payload(text_result);
    assert_clean_response(&text_payload, "text");

    let file_args = json!({ "path": synthetic_image_path() })
        .as_object()
        .cloned()
        .expect("arguments are object");
    let file_result = client
        .call_tool(CallToolRequestParams::new("gaze_read_file").with_arguments(file_args))
        .await
        .expect("file tool call returns");
    if file_result.is_error == Some(true) {
        let text = &file_result.content[0].raw.as_text().unwrap().text;
        if text.starts_with("backend-unavailable:") {
            eprintln!("SKIP: {text}");
            client.cancel().await.expect("client cancels");
            server_task.await.expect("server finishes");
            return;
        }
        panic!("unexpected file tool error: {text}");
    }
    let file_payload = parse_success_payload(file_result);
    assert_clean_response(&file_payload, "image");

    assert_eq!(host.manifest.begins.load(Ordering::SeqCst), 2);
    assert_eq!(host.manifest.finishes.load(Ordering::SeqCst), 2);
    assert_eq!(host.manifest.fails.load(Ordering::SeqCst), 0);

    client.cancel().await.expect("client cancels");
    server_task.await.expect("server finishes");
}
