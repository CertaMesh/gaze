use async_trait::async_trait;
use gaze_mcp_core::*;
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Store(Mutex<Vec<&'static str>>);
#[async_trait]
impl ManifestStore for Store {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.0.lock().unwrap().push("begin");
        Ok(CallHandle::new(ctx.call_id))
    }
    async fn finish_call(&self, _: CallHandle, _: SnapshotRef) -> Result<(), ManifestError> {
        self.0.lock().unwrap().push("finish");
        Ok(())
    }
    async fn fail_call(&self, _: CallHandle, _: FailureReason) -> Result<(), ManifestError> {
        self.0.lock().unwrap().push("fail");
        Ok(())
    }
}
struct Auth;
#[async_trait]
impl AuthHook for Auth {
    async fn authorize_agent(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Err(AuthError::Denied("synthetic".into()))
    }
}
struct Producer(ToolDescriptor);
#[async_trait]
impl Tool for Producer {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.0
    }
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        assert!(
            !ctx.redacted_args()["text"]
                .as_str()
                .unwrap()
                .contains("alice@example.invalid")
        );
        if ctx.redacted_args()["text"] == "fail" {
            return Ok(ToolResponse::json(json!({"alice@example.invalid":42})));
        }
        Ok(ToolResponse::json(json!({"result":"bob@example.invalid"})))
    }
}
struct Host {
    registry: ToolRegistry,
    core: gaze_assembly::CorePipeline,
    session: gaze::Session,
    store: Store,
}
#[async_trait]
impl DispatchHost for Host {
    async fn dispatch(
        &self,
        principal: &Principal,
        name: &str,
        args: Value,
        sid: Option<&str>,
    ) -> Result<ToolResponse, DispatchError> {
        PiiEnvelope::new(
            &self.registry,
            &Auth,
            &self.store,
            self.core.pipeline(),
            &self.session,
            self.core.locale_chain().as_slice(),
            &SessionIdPolicy::default_strict(),
        )
        .dispatch(principal, name, args, sid)
        .await
    }
    fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.registry.list().into_iter().cloned().collect()
    }
}
#[tokio::test]
async fn actual_transport_protects_fresh_response_and_hides_carrier_errors() {
    let mut registry = ToolRegistry::new();
    registry
        .register(Producer(
            ToolDescriptor::agent("test", json!({"type":"object"})).with_carriers(
                CarrierDeclaration::text_fields(&["text"]),
                CarrierDeclaration::text_fields(&["result"]),
            ),
        ))
        .unwrap();
    let host = Arc::new(Host {
        registry,
        core: gaze_assembly::CorePipelineConfig::new().build().unwrap(),
        session: gaze::Session::new(gaze::Scope::Ephemeral).unwrap(),
        store: Store::default(),
    });
    let handler = RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::agent("synthetic")))
        .into_server_handler(host.clone());
    let (client_stream, server_stream) = tokio::io::duplex(16384);
    let server = tokio::spawn(async move {
        rmcp::serve_server(handler, server_stream)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let client = ().serve(client_stream).await.unwrap();
    let success = client
        .call_tool(
            CallToolRequestParams::new("test").with_arguments(
                json!({"text":"alice@example.invalid"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(success.is_error, Some(true));
    let wire = serde_json::to_string(&success).unwrap();
    assert!(!wire.contains("alice@example.invalid"));
    assert!(!wire.contains("bob@example.invalid"));
    assert!(
        host.session
            .snapshot_entries()
            .iter()
            .any(|entry| entry.raw == "bob@example.invalid")
    );
    let rejected = client
        .call_tool(
            CallToolRequestParams::new("test")
                .with_arguments(json!({"text":"fail"}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(rejected.is_error, Some(true));
    assert_eq!(
        rejected.content[0].raw.as_text().unwrap().text,
        "redaction-failed"
    );
    assert_eq!(
        *host.store.0.lock().unwrap(),
        ["begin", "finish", "begin", "fail"]
    );
    client.cancel().await.unwrap();
    server.await.unwrap();
}
