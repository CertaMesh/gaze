use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use gaze::PiiClass;
use gaze_mcp_bridge::approval::ApprovalRequest;
use gaze_mcp_bridge::audit::{ArgPath, BridgeAuditEvent, BridgeAuditSink};
use gaze_mcp_bridge::client::{DownstreamClient, DownstreamError, DownstreamTool, RmcpChildClient};
use gaze_mcp_bridge::config::{BridgeConfig, LimitCfg, ServerSpec};
use gaze_mcp_bridge::policy::{
    DecisionOutcome, FieldPolicy, PolicyConfig, ResultMode, ResultPolicy, ToolPolicy,
};
use gaze_mcp_bridge::{BridgeHostBuilder, BridgeRegistry, BridgeSessionStore, NamespacedTool};
use gaze_mcp_core::{
    AuthError, AuthHook, DispatchError, DispatchHost, Principal, ToolDescriptor, ToolError,
};
use proptest::prop_assert;
use rmcp::model::{CallToolResult, Content, RawResource};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

const SID_A: &str = "01HRT7K6P6X5Q9M0V8YQ4N7TBC";
const SID_B: &str = "01HRT7K6P6X5Q9M0V8YQ4N7TBD";
const RAW_EMAIL: &str = "alice@example.invalid";

#[derive(Default)]
struct AllowAuth;

#[async_trait]
impl AuthHook for AllowAuth {
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
        Err(AuthError::Denied("operator not exposed".to_string()))
    }
}

#[derive(Default)]
struct MemoryAudit {
    events: Mutex<Vec<BridgeAuditEvent>>,
}

#[async_trait]
impl BridgeAuditSink for MemoryAudit {
    async fn record(&self, event: &BridgeAuditEvent) -> gaze_mcp_bridge::BridgeResult<()> {
        self.events.lock().await.push(event.clone());
        Ok(())
    }
}

struct FailingAudit;

#[async_trait]
impl BridgeAuditSink for FailingAudit {
    async fn record(&self, _event: &BridgeAuditEvent) -> gaze_mcp_bridge::BridgeResult<()> {
        Err(gaze_mcp_bridge::BridgeError::Audit(
            "planned failure".to_string(),
        ))
    }
}

#[derive(Clone)]
enum FakeResponse {
    Result(CallToolResult),
    McpError {
        message: String,
        data: Option<Value>,
    },
    ServiceError(String),
}

struct FakeClient {
    calls: Mutex<Vec<Value>>,
    response: Mutex<FakeResponse>,
}

impl FakeClient {
    fn new(response: FakeResponse) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            response: Mutex::new(response),
        })
    }

    async fn calls(&self) -> Vec<Value> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl DownstreamClient for FakeClient {
    async fn list_tools(
        &self,
        _timeout: Duration,
    ) -> gaze_mcp_bridge::BridgeResult<Vec<DownstreamTool>> {
        Ok(vec![DownstreamTool {
            raw_name: "send".to_string(),
            description: Some("send".to_string()),
            input_schema: json!({"type": "object"}),
            output_schema: None,
        }])
    }

    async fn list_resources(&self, _timeout: Duration) -> gaze_mcp_bridge::BridgeResult<usize> {
        Ok(1)
    }

    async fn list_prompts(&self, _timeout: Duration) -> gaze_mcp_bridge::BridgeResult<usize> {
        Ok(1)
    }

    async fn call_tool(
        &self,
        _tool_name: &str,
        args: Value,
        _timeout: Duration,
    ) -> Result<CallToolResult, DownstreamError> {
        self.calls.lock().await.push(args);
        match self.response.lock().await.clone() {
            FakeResponse::Result(result) => Ok(result),
            FakeResponse::McpError { message, data } => Err(DownstreamError::Mcp { message, data }),
            FakeResponse::ServiceError(message) => Err(DownstreamError::Service(message)),
        }
    }
}

fn result_text(text: &str) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text.to_string())])
}

async fn host_with_fake(
    client: Arc<FakeClient>,
    policy: PolicyConfig,
    audit: Arc<dyn BridgeAuditSink>,
) -> (gaze_mcp_bridge::BridgeHost, Arc<BridgeSessionStore>) {
    host_with_fake_and_limits(client, policy, audit, LimitCfg::default()).await
}

async fn host_with_fake_and_limits(
    client: Arc<FakeClient>,
    policy: PolicyConfig,
    audit: Arc<dyn BridgeAuditSink>,
    limits: LimitCfg,
) -> (gaze_mcp_bridge::BridgeHost, Arc<BridgeSessionStore>) {
    let tool = NamespacedTool {
        namespaced_name: "mail.send".to_string(),
        server: "mail".to_string(),
        raw_server: "mail".to_string(),
        raw_tool: "send".to_string(),
        sanitized_tool: "send".to_string(),
        descriptor: ToolDescriptor::agent("mail.send", json!({"type": "object"})),
        discovery_hash: "discovery-hash".to_string(),
        client,
    };
    let registry = Arc::new(BridgeRegistry::from_tools(vec![tool]).expect("registry"));
    let store = Arc::new(BridgeSessionStore::ephemeral());
    let host = BridgeHostBuilder::new(registry, Arc::clone(&store), audit)
        .auth(Arc::new(AllowAuth))
        .policy(policy)
        .limits(limits)
        .build();
    (host, store)
}

async fn seed_email_token(store: &BridgeSessionStore, sid: &str) -> String {
    let session = store.get(sid).await.expect("session");
    let guard = session.lock().await;
    guard.tokenize(&PiiClass::Email, RAW_EMAIL).expect("token")
}

fn allow_to_policy() -> PolicyConfig {
    let mut policy = PolicyConfig::default();
    let mut tool = ToolPolicy::default();
    let field = FieldPolicy {
        allow_sensitive_fields: true,
        ..FieldPolicy::default()
    };
    tool.arguments.insert("to".to_string(), field);
    policy.tools.insert("mail.send".to_string(), tool);
    policy
}

fn approval_to_policy() -> PolicyConfig {
    let mut policy = allow_to_policy();
    let tool = policy.tools.get_mut("mail.send").expect("tool");
    let field = tool.arguments.get_mut("to").expect("to field");
    field.requires_approval = true;
    policy
}

fn deny_result_policy() -> PolicyConfig {
    let mut policy = allow_to_policy();
    let tool = policy.tools.get_mut("mail.send").expect("tool");
    tool.result = Some(ResultPolicy {
        mode: ResultMode::Deny,
    });
    policy
}

#[tokio::test]
async fn config_rejects_file_mode_without_key_and_log_raw() {
    let no_key = r#"
        [session]
        mode = "file"
        dir = ".gaze/sessions"
        key_env = "GAZE_MISSING_TEST_KEY"

        [servers.mail]
        command = "mcp-mail"
    "#;
    assert!(BridgeConfig::from_toml_str(no_key).is_err());

    let log_raw = r#"
        [session]
        mode = "ephemeral"

        [servers.mail]
        command = "mcp-mail"

        [policy.default]
        log_raw = true
    "#;
    assert!(BridgeConfig::from_toml_str(log_raw).is_err());
}

#[tokio::test]
async fn discovery_namespaces_tools_and_reports_denied_resources_prompts() {
    let client = FakeClient::new(FakeResponse::Result(result_text("ok")));
    let mut clients: BTreeMap<String, Arc<dyn DownstreamClient>> = BTreeMap::new();
    clients.insert("mail server".to_string(), client);
    let registry = BridgeRegistry::discover(clients, Duration::from_secs(1))
        .await
        .expect("discover");
    let rows = registry.print_surface();
    assert!(rows.iter().any(|row| row == "tool allow mail_server.send"));
    assert!(rows
        .iter()
        .any(|row| row.starts_with("resources deny mail_server")));
    assert!(rows
        .iter()
        .any(|row| row.starts_with("prompts deny mail_server")));
}

#[tokio::test]
async fn sanitize_collision_is_registration_error() {
    let left = FakeClient::new(FakeResponse::Result(result_text("ok")));
    let right = FakeClient::new(FakeResponse::Result(result_text("ok")));
    let mut clients: BTreeMap<String, Arc<dyn DownstreamClient>> = BTreeMap::new();
    clients.insert("mail server".to_string(), left);
    clients.insert("mail/server".to_string(), right);
    let err = match BridgeRegistry::discover(clients, Duration::from_secs(1)).await {
        Ok(_) => panic!("sanitized collision should fail"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("collision"));
}

#[tokio::test]
async fn oversize_tool_name_is_registration_error() {
    let client = FakeClient::new(FakeResponse::Result(result_text("ok")));
    let mut clients: BTreeMap<String, Arc<dyn DownstreamClient>> = BTreeMap::new();
    clients.insert("s".repeat(129), client);
    let err = match BridgeRegistry::discover(clients, Duration::from_secs(1)).await {
        Ok(_) => panic!("oversize name should fail"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("1..=128"));
}

#[tokio::test]
async fn default_auth_denies_before_forward() {
    let client = FakeClient::new(FakeResponse::Result(result_text("ok")));
    let tool = NamespacedTool {
        namespaced_name: "mail.send".to_string(),
        server: "mail".to_string(),
        raw_server: "mail".to_string(),
        raw_tool: "send".to_string(),
        sanitized_tool: "send".to_string(),
        descriptor: ToolDescriptor::agent("mail.send", json!({"type": "object"})),
        discovery_hash: "hash".to_string(),
        client: client.clone(),
    };
    let registry = Arc::new(BridgeRegistry::from_tools(vec![tool]).expect("registry"));
    let host = BridgeHostBuilder::new(
        registry,
        Arc::new(BridgeSessionStore::ephemeral()),
        Arc::new(MemoryAudit::default()),
    )
    .build();
    let err = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({}),
            Some(SID_A),
        )
        .await
        .expect_err("auth denies");
    assert!(matches!(err, DispatchError::Auth(_)));
    assert!(client.calls().await.is_empty());
}

#[tokio::test]
async fn per_field_allow_restores_token_and_routes_downstream() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let (host, store) = host_with_fake(
        client.clone(),
        allow_to_policy(),
        Arc::new(MemoryAudit::default()),
    )
    .await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token, "subject": "hello"}),
            Some(SID_A),
        )
        .await
        .expect("dispatch");
    assert_eq!(response.payload["content"][0]["text"], "sent");
    assert_eq!(
        client.calls().await,
        vec![json!({"to": RAW_EMAIL, "subject": "hello"})]
    );
}

#[tokio::test]
async fn token_on_default_deny_field_is_blocked() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let (host, store) = host_with_fake(
        client.clone(),
        PolicyConfig::default(),
        Arc::new(MemoryAudit::default()),
    )
    .await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("blocked response");
    assert_eq!(
        response.payload["gaze_bridge"]["reason"],
        "sensitive_field_not_allowed"
    );
    assert!(client.calls().await.is_empty());
}

#[tokio::test]
async fn cross_session_token_is_not_restorable() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let (host, store) = host_with_fake(
        client.clone(),
        allow_to_policy(),
        Arc::new(MemoryAudit::default()),
    )
    .await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_B),
        )
        .await
        .expect("blocked response");
    assert_eq!(response.payload["gaze_bridge"]["reason"], "unknown_token");
    assert!(client.calls().await.is_empty());
}

#[tokio::test]
async fn trap_token_on_sensitive_field_is_blocked() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let (host, _) = host_with_fake(
        client.clone(),
        allow_to_policy(),
        Arc::new(MemoryAudit::default()),
    )
    .await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": "<Email_1>"}),
            Some(SID_A),
        )
        .await
        .expect("blocked response");
    assert_eq!(response.payload["gaze_bridge"]["reason"], "unknown_token");
    assert!(client.calls().await.is_empty());
}

#[tokio::test]
async fn forged_session_id_is_rejected() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let (host, _) = host_with_fake(
        client.clone(),
        allow_to_policy(),
        Arc::new(MemoryAudit::default()),
    )
    .await;
    let err = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": "hello"}),
            Some("session-1"),
        )
        .await
        .expect_err("bad session id");
    assert!(matches!(err, DispatchError::SessionId(_)));
    assert!(client.calls().await.is_empty());
}

#[tokio::test]
async fn raw_pii_in_args_is_blocked_even_without_token() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let (host, _) = host_with_fake(
        client.clone(),
        allow_to_policy(),
        Arc::new(MemoryAudit::default()),
    )
    .await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": RAW_EMAIL}),
            Some(SID_A),
        )
        .await
        .expect("blocked response");
    assert_eq!(response.payload["gaze_bridge"]["reason"], "raw_pii_in_args");
    assert!(client.calls().await.is_empty());
}

#[tokio::test]
async fn pii_object_key_in_args_fails_closed_without_raw_path() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let audit = Arc::new(MemoryAudit::default());
    let (host, _) = host_with_fake(client.clone(), allow_to_policy(), audit.clone()).await;
    let err = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({RAW_EMAIL: "hello"}),
            Some(SID_A),
        )
        .await
        .expect_err("PII-bearing object key fails closed");
    assert!(!format!("{err:?}").contains(RAW_EMAIL));
    assert!(client.calls().await.is_empty());
    let audit = serde_json::to_string(&*audit.events.lock().await).expect("audit json");
    assert!(!audit.contains(RAW_EMAIL));
}

#[tokio::test]
async fn approval_required_is_structured_and_not_forwarded() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let (host, store) = host_with_fake(
        client.clone(),
        approval_to_policy(),
        Arc::new(MemoryAudit::default()),
    )
    .await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("approval response");
    assert_eq!(
        response.payload["gaze_bridge"]["outcome"],
        "approval_required"
    );
    assert!(client.calls().await.is_empty());
}

#[test]
fn approval_request_hash_binds_changed_args() {
    let left = ApprovalRequest::new(
        "call",
        SID_A,
        &json!({"to": "<deadbeef:Email_1>"}),
        ["Email".to_string()].into_iter().collect(),
        &["<deadbeef:Email_1>".to_string()],
        "mail.send",
        "discovery",
        "reason",
        vec![ArgPath::root().child("to").expect("path")],
    );
    let right = ApprovalRequest::new(
        "call",
        SID_A,
        &json!({"to": "<deadbeef:Email_2>"}),
        ["Email".to_string()].into_iter().collect(),
        &["<deadbeef:Email_2>".to_string()],
        "mail.send",
        "discovery",
        "reason",
        vec![ArgPath::root().child("to").expect("path")],
    );
    assert_ne!(left.redacted_arg_sha256, right.redacted_arg_sha256);
    assert_ne!(left.token_list_sha256, right.token_list_sha256);
}

#[tokio::test]
async fn audit_must_succeed_before_forward() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let (host, store) =
        host_with_fake(client.clone(), allow_to_policy(), Arc::new(FailingAudit)).await;
    let token = seed_email_token(&store, SID_A).await;
    let err = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect_err("audit failure");
    assert!(matches!(err, DispatchError::Manifest(_)));
    assert!(client.calls().await.is_empty());
}

#[tokio::test]
async fn is_error_text_structured_and_meta_are_redacted() {
    let mut result = CallToolResult::error(vec![Content::text(format!("missing {RAW_EMAIL}"))]);
    result.structured_content = Some(json!({"owner": RAW_EMAIL}));
    result.meta = Some(rmcp::model::Meta({
        let mut map = rmcp::model::JsonObject::new();
        map.insert("trace".to_string(), json!(RAW_EMAIL));
        map
    }));
    let client = FakeClient::new(FakeResponse::Result(result));
    let (host, store) =
        host_with_fake(client, allow_to_policy(), Arc::new(MemoryAudit::default())).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("dispatch");
    let rendered = serde_json::to_string(&response.payload).expect("json");
    assert!(!rendered.contains(RAW_EMAIL));
    assert!(rendered.contains("isError"));
}

#[tokio::test]
async fn embedded_resource_resource_link_and_image_are_denied() {
    for content in [
        Content::embedded_text("file:///secret", RAW_EMAIL),
        Content::resource_link(RawResource {
            uri: "file:///secret".to_string(),
            name: "secret".to_string(),
            title: Some(RAW_EMAIL.to_string()),
            description: None,
            mime_type: None,
            size: None,
            icons: None,
            meta: None,
        }),
        Content::image("AAAA", "image/png"),
    ] {
        let client = FakeClient::new(FakeResponse::Result(CallToolResult::success(vec![content])));
        let (host, store) = host_with_fake(
            client.clone(),
            allow_to_policy(),
            Arc::new(MemoryAudit::default()),
        )
        .await;
        let token = seed_email_token(&store, SID_A).await;
        let response = host
            .dispatch(
                &Principal::new("agent"),
                "mail.send",
                json!({"to": token}),
                Some(SID_A),
            )
            .await
            .expect("blocked");
        assert_eq!(
            response.payload["gaze_bridge"]["reason"],
            "unsupported_content_block"
        );
    }
}

#[tokio::test]
async fn result_deny_policy_blocks_successful_result() {
    let client = FakeClient::new(FakeResponse::Result(result_text("screen bytes")));
    let (host, store) = host_with_fake(
        client,
        deny_result_policy(),
        Arc::new(MemoryAudit::default()),
    )
    .await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("blocked");
    assert_eq!(response.payload["gaze_bridge"]["reason"], "result_denied");
}

#[tokio::test]
async fn response_byte_and_block_limits_block_before_return() {
    let client = FakeClient::new(FakeResponse::Result(result_text(
        "this response is too long",
    )));
    let limits = LimitCfg {
        call_timeout_ms: 1_000,
        response_bytes: 16,
        content_blocks: 64,
    };
    let (host, store) = host_with_fake_and_limits(
        client,
        allow_to_policy(),
        Arc::new(MemoryAudit::default()),
        limits,
    )
    .await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("blocked");
    assert_eq!(
        response.payload["gaze_bridge"]["reason"],
        "response_too_large"
    );

    let many = CallToolResult::success(
        (0..65)
            .map(|idx| Content::text(format!("block {idx}")))
            .collect::<Vec<_>>(),
    );
    let client = FakeClient::new(FakeResponse::Result(many));
    let (host, store) =
        host_with_fake(client, allow_to_policy(), Arc::new(MemoryAudit::default())).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("blocked");
    assert_eq!(
        response.payload["gaze_bridge"]["reason"],
        "too_many_content_blocks"
    );
}

async fn last_audit_event(audit: &MemoryAudit) -> BridgeAuditEvent {
    audit
        .events
        .lock()
        .await
        .last()
        .expect("at least one audit event")
        .clone()
}

fn assert_blocked_audit(event: &BridgeAuditEvent, expected_rule: &str, server: &str, tool: &str) {
    assert_eq!(
        event.outcome,
        DecisionOutcome::Blocked,
        "post-ingress audit for a block ({expected_rule}) should be Blocked, but was {:?}",
        event.outcome,
    );
    assert_eq!(event.deciding_rule, expected_rule);
    assert_eq!(event.decision, "Blocked");
    assert_eq!(event.server, server);
    assert_eq!(event.tool, tool);
    assert!(
        !event.result_paths_affected.is_empty(),
        "a blocked ingress result should record at least one result_path_affected"
    );
}

#[tokio::test]
async fn ingress_result_deny_block_is_audited_as_blocked() {
    let client = FakeClient::new(FakeResponse::Result(result_text("screen bytes")));
    let audit = Arc::new(MemoryAudit::default());
    let (host, store) = host_with_fake(client, deny_result_policy(), audit.clone()).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("dispatch");
    assert_eq!(response.payload["gaze_bridge"]["reason"], "result_denied");
    let post_ingress = last_audit_event(&audit).await;
    assert_blocked_audit(&post_ingress, "ingress.result.deny", "mail", "send");
}

#[tokio::test]
async fn ingress_byte_limit_block_is_audited_as_blocked() {
    let client = FakeClient::new(FakeResponse::Result(result_text(
        "this response is too long",
    )));
    let limits = LimitCfg {
        call_timeout_ms: 1_000,
        response_bytes: 16,
        content_blocks: 64,
    };
    let audit = Arc::new(MemoryAudit::default());
    let (host, store) =
        host_with_fake_and_limits(client, allow_to_policy(), audit.clone(), limits).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("blocked");
    assert_eq!(
        response.payload["gaze_bridge"]["reason"],
        "response_too_large"
    );
    let post_ingress = last_audit_event(&audit).await;
    assert_blocked_audit(&post_ingress, "ingress.limit.bytes", "mail", "send");
}

#[tokio::test]
async fn ingress_block_count_limit_is_audited_as_blocked() {
    let many = CallToolResult::success(
        (0..65)
            .map(|idx| Content::text(format!("block {idx}")))
            .collect::<Vec<_>>(),
    );
    let client = FakeClient::new(FakeResponse::Result(many));
    let audit = Arc::new(MemoryAudit::default());
    let (host, store) = host_with_fake(client, allow_to_policy(), audit.clone()).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("blocked");
    assert_eq!(
        response.payload["gaze_bridge"]["reason"],
        "too_many_content_blocks"
    );
    let post_ingress = last_audit_event(&audit).await;
    assert_blocked_audit(&post_ingress, "ingress.limit.blocks", "mail", "send");
}

#[tokio::test]
async fn ingress_kind_deny_block_is_audited_as_blocked() {
    let content = Content::image("AAAA", "image/png");
    let client = FakeClient::new(FakeResponse::Result(CallToolResult::success(vec![content])));
    let audit = Arc::new(MemoryAudit::default());
    let (host, store) = host_with_fake(client, allow_to_policy(), audit.clone()).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("blocked");
    assert_eq!(
        response.payload["gaze_bridge"]["reason"],
        "unsupported_content_block"
    );
    let post_ingress = last_audit_event(&audit).await;
    assert_blocked_audit(&post_ingress, "ingress.kind.deny", "mail", "send");
}

#[tokio::test]
async fn ingress_allowed_text_result_is_audited_as_allowed_with_processed_rule() {
    let client = FakeClient::new(FakeResponse::Result(result_text("sent")));
    let audit = Arc::new(MemoryAudit::default());
    let (host, store) = host_with_fake(client, allow_to_policy(), audit.clone()).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token, "subject": "hello"}),
            Some(SID_A),
        )
        .await
        .expect("dispatch");
    assert_eq!(response.payload["content"][0]["text"], "sent");

    let events = audit.events.lock().await.clone();
    // A successful forward emits two events: egress-allowed then ingress-allowed.
    assert_eq!(events.len(), 2);
    let egress = &events[0];
    assert_eq!(egress.outcome, DecisionOutcome::Allowed);
    assert!(
        egress.deciding_rule.starts_with("egress."),
        "egress event should carry an egress.* rule, got {}",
        egress.deciding_rule
    );
    let ingress = &events[1];
    assert_eq!(ingress.outcome, DecisionOutcome::Allowed);
    assert_eq!(ingress.deciding_rule, "ingress.processed");
    assert_eq!(ingress.decision, "Allowed");
    // Both events for a single forwarded call share the same call id.
    assert_eq!(egress.upstream_request_id, ingress.upstream_request_id);
    assert_eq!(ingress.server, "mail");
    assert_eq!(ingress.tool, "send");
    // An allowed ResultMode::Process result carries no result_paths_affected by
    // default (no PII touched this short opaque text).
    assert!(ingress.result_paths_affected.is_empty());
}

#[tokio::test]
async fn downstream_json_rpc_error_data_is_redacted() {
    let client = FakeClient::new(FakeResponse::McpError {
        message: format!("not found {RAW_EMAIL}"),
        data: Some(json!({"email": RAW_EMAIL})),
    });
    let (host, store) =
        host_with_fake(client, allow_to_policy(), Arc::new(MemoryAudit::default())).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("error response");
    let rendered = serde_json::to_string(&response.payload).expect("json");
    assert!(!rendered.contains(RAW_EMAIL));
    assert_eq!(response.payload["isError"], true);
}

#[tokio::test]
async fn downstream_service_error_detail_is_generic_and_audit_is_clean() {
    let client = FakeClient::new(FakeResponse::ServiceError(format!(
        "downstream transport echoed {RAW_EMAIL}"
    )));
    let audit = Arc::new(MemoryAudit::default());
    let (host, store) = host_with_fake(client, allow_to_policy(), audit.clone()).await;
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("generic service error response");
    let rendered = serde_json::to_string(&response.payload).expect("json");
    assert!(!rendered.contains(RAW_EMAIL));
    assert_eq!(response.payload["isError"], true);
    assert_eq!(
        response.payload["error"]["message"],
        "downstream service error"
    );

    let audit = serde_json::to_string(&*audit.events.lock().await).expect("audit json");
    assert!(!audit.contains(RAW_EMAIL));
}

#[test]
fn backend_failure_agent_messages_are_generic() {
    for err in [
        gaze_mcp_bridge::BridgeError::SessionStore(format!("session load saw {RAW_EMAIL}")),
        gaze_mcp_bridge::BridgeError::Downstream(format!("downstream saw {RAW_EMAIL}")),
        gaze_mcp_bridge::BridgeError::Redaction(format!("redaction saw {RAW_EMAIL}")),
    ] {
        let dispatch = err.into_dispatch();
        let DispatchError::ToolError(ToolError::BackendFailure(message)) = dispatch else {
            panic!("expected generic backend failure");
        };
        assert!(!message.contains(RAW_EMAIL));
    }
}

#[tokio::test]
async fn encrypted_file_store_does_not_write_raw_pii_and_reloads() {
    let dir = TempDir::new().expect("tempdir");
    std::env::set_var("GAZE_BRIDGE_TEST_KEY", "22".repeat(32));
    let config = format!(
        r#"
        [session]
        mode = "file"
        dir = "{}"
        key_env = "GAZE_BRIDGE_TEST_KEY"

        [servers.mail]
        command = "mail"
        "#,
        dir.path().display()
    );
    let bridge_config = BridgeConfig::from_toml_str(&config).expect("config");
    let store = BridgeSessionStore::from_config(&bridge_config.session).expect("store");
    let session = store.get(SID_A).await.expect("session");
    {
        let guard = session.lock().await;
        guard
            .tokenize(&PiiClass::Email, RAW_EMAIL)
            .expect("tokenize");
        store.persist(SID_A, &guard).await.expect("persist");
    }
    let bytes = read_all_files(dir.path());
    assert!(!String::from_utf8_lossy(&bytes).contains(RAW_EMAIL));
    let reloaded = BridgeSessionStore::from_config(&bridge_config.session).expect("reload store");
    let loaded = reloaded.get(SID_A).await.expect("load session");
    assert_eq!(loaded.lock().await.snapshot_entries().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_mode_parallel_sessions_do_not_corrupt_each_other() {
    let dir = TempDir::new().expect("tempdir");
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_CONC", "33".repeat(32));
    let config = format!(
        r#"
        [session]
        mode = "file"
        dir = "{}"
        key_env = "GAZE_BRIDGE_TEST_KEY_CONC"

        [servers.mail]
        command = "mail"
        "#,
        dir.path().display()
    );
    let bridge_config = BridgeConfig::from_toml_str(&config).expect("config");
    let store = Arc::new(BridgeSessionStore::from_config(&bridge_config.session).expect("store"));
    let mut handles = Vec::new();
    for idx in 0..8 {
        let store = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let sid = format!("01HRT7K6P6X5Q9M0V8YQ4N7T{idx:02}");
            let session = store.get(&sid).await.expect("session");
            let guard = session.lock().await;
            guard
                .tokenize(&PiiClass::Email, &format!("user{idx}@example.invalid"))
                .expect("token");
            store.persist(&sid, &guard).await.expect("persist");
        }));
    }
    for handle in handles {
        handle.await.expect("join");
    }
    assert_eq!(std::fs::read_dir(dir.path()).expect("read dir").count(), 8);
}

fn session_cap_config(dir: Option<&Path>, max_sessions: usize) -> String {
    let session_block = match dir {
        Some(d) => format!(
            r#"
        [session]
        mode = "file"
        dir = "{dir}"
        key_env = "GAZE_BRIDGE_TEST_KEY_EVICT"
        max_sessions = {max_sessions}
        "#,
            dir = d.display(),
        ),
        None => format!(
            r#"
        [session]
        mode = "ephemeral"
        max_sessions = {max_sessions}
        "#,
        ),
    };
    format!(
        "{session_block}
        [servers.mail]
        command = \"mail\"
        ",
    )
}

#[tokio::test]
async fn session_cap_defaults_to_one_thousand_when_omitted() {
    let raw = r#"
        [session]
        mode = "ephemeral"

        [servers.mail]
        command = "mcp-mail"
    "#;
    let config = BridgeConfig::from_toml_str(raw).expect("config");
    assert_eq!(config.session.max_sessions, 1_000);
}

#[tokio::test]
async fn session_cap_rejects_zero() {
    let raw = r#"
        [session]
        mode = "ephemeral"
        max_sessions = 0

        [servers.mail]
        command = "mcp-mail"
    "#;
    assert!(BridgeConfig::from_toml_str(raw).is_err());
}

#[tokio::test]
async fn ephemeral_mode_rejects_new_sessions_when_cap_reached() {
    let config = BridgeConfig::from_toml_str(&session_cap_config(None, 2)).expect("config");
    let store = BridgeSessionStore::from_config(&config.session).expect("store");

    let sids = [
        "01HRT7K6P6X5Q9M0V8YQ4N7T01",
        "01HRT7K6P6X5Q9M0V8YQ4N7T02",
        "01HRT7K6P6X5Q9M0V8YQ4N7T03",
    ];
    store.get(sids[0]).await.expect("first session");
    store.get(sids[1]).await.expect("second session");
    assert_eq!(store.len().await, 2, "ephemeral store fills to the cap");

    match store.get(sids[2]).await {
        Err(err) => assert!(
            matches!(err, gaze_mcp_bridge::BridgeError::LimitExceeded(_)),
            "expected LimitExceeded, got {err:?}"
        ),
        Ok(_) => panic!("third session should exceed ephemeral cap"),
    }
    assert_eq!(
        store.len().await,
        2,
        "a rejected new session must not enlarge the cache"
    );

    // Cached sessions remain usable after the cap is reached.
    let still_cached = store.get(sids[0]).await.expect("cached session retained");
    assert_eq!(still_cached.lock().await.snapshot_entries().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_mode_evicts_lru_and_reloads_evicted_session_from_disk() {
    let dir = TempDir::new().expect("tempdir");
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_EVICT", "44".repeat(32));
    let config =
        BridgeConfig::from_toml_str(&session_cap_config(Some(dir.path()), 2)).expect("config");
    let store = Arc::new(BridgeSessionStore::from_config(&config.session).expect("store"));

    let sids = [
        "01HRT7K6P6X5Q9M0V8YQ4N7T01",
        "01HRT7K6P6X5Q9M0V8YQ4N7T02",
        "01HRT7K6P6X5Q9M0V8YQ4N7T03",
    ];

    // Tokenize + persist sid_a so its token can be restored from disk after eviction.
    let token_a = {
        let session = store.get(sids[0]).await.expect("a");
        let guard = session.lock().await;
        let token = guard.tokenize(&PiiClass::Email, RAW_EMAIL).expect("token");
        store.persist(sids[0], &guard).await.expect("persist a");
        token
    };
    let _ = store.get(sids[1]).await.expect("b");
    assert_eq!(store.len().await, 2);

    // Inserting a third session exceeds the cap and evicts the LRU (sid_a).
    let _ = store.get(sids[2]).await.expect("c");
    assert_eq!(store.len().await, 2, "file store stays bounded at the cap");

    // sid_a was evicted; reloading it must re-read the encrypted file and restore
    // the previously issued token (round-trip works after eviction).
    let reloaded = store.get(sids[0]).await.expect("reload a");
    let guard = reloaded.lock().await;
    assert_eq!(guard.restore(&token_a), Some(RAW_EMAIL.to_string()));
    assert_eq!(guard.snapshot_entries().len(), 1);
    drop(guard);
    assert_eq!(
        store.len().await,
        2,
        "reload evicts again rather than growing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_mode_lru_keeps_recently_used_session_cached() {
    let dir = TempDir::new().expect("tempdir");
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_LRU", "55".repeat(32));
    let config = BridgeConfig::from_toml_str(
        &session_cap_config(Some(dir.path()), 2)
            .replace("GAZE_BRIDGE_TEST_KEY_EVICT", "GAZE_BRIDGE_TEST_KEY_LRU"),
    )
    .expect("config");
    let store = BridgeSessionStore::from_config(&config.session).expect("store");

    let sids = [
        "01HRT7K6P6X5Q9M0V8YQ4N7T01",
        "01HRT7K6P6X5Q9M0V8YQ4N7T02",
        "01HRT7K6P6X5Q9M0V8YQ4N7T03",
    ];

    // Tokenize sid_a without persisting: its token lives only in the in-memory
    // cache. Weak references distinguish cached identity from a disk reload.
    let token_a = {
        let session = store.get(sids[0]).await.expect("a");
        let guard = session.lock().await;
        guard.tokenize(&PiiClass::Email, RAW_EMAIL).expect("token")
    };
    let a_identity = Arc::downgrade(&store.get(sids[0]).await.unwrap());
    let b_identity = Arc::downgrade(&store.get(sids[1]).await.expect("b"));
    // Touch sid_a → it becomes most-recently-used; sid_b is now the LRU.
    let _ = store.get(sids[0]).await;
    // Insert sid_c → cap exceeded → evict the LRU (sid_b), not sid_a.
    let _ = store.get(sids[2]).await.expect("c");
    assert_eq!(store.len().await, 2);

    assert!(
        a_identity.upgrade().is_some(),
        "recent session stays cached"
    );
    assert!(
        b_identity.upgrade().is_none(),
        "least recent session was evicted"
    );

    // sid_a survived eviction: its cached token is still valid (no disk reload).
    let a_cached = store.get(sids[0]).await.expect("a still cached");
    assert_eq!(
        a_cached.lock().await.restore(&token_a),
        Some(RAW_EMAIL.to_string()),
        "LRU must keep the most-recently-used session cached"
    );

    // sid_b was evicted after its empty state was persisted; reload is empty.
    let b_reloaded = store.get(sids[1]).await.expect("b reloaded");
    assert_eq!(
        b_reloaded.lock().await.snapshot_entries().len(),
        0,
        "LRU must evict the least-recently-used session"
    );
}

/// Regression test for the active-session eviction race.
///
/// Scenario: cap=1, call A holds its SharedSession Arc while call B for a
/// different session id is attempted. The LRU eviction loop must not remove
/// session A from the cache while A's Arc is still live, because doing so
/// would allow a subsequent get(A) to load the older on-disk snapshot into a
/// second independent Session object — the two live sessions could then
/// overwrite each other's persisted token mappings on persist().
///
/// Expected behaviour: get(B) is rejected with LimitExceeded (all cached
/// sessions are active), A remains canonical in the cache, and after A's Arc
/// is dropped a fresh get(A) returns the same in-memory session (no stale
/// reload).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w1_file_cap_retains_active_session_identity() {
    let dir = TempDir::new().expect("tempdir");
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_ACTIVE", "66".repeat(32));
    let config = BridgeConfig::from_toml_str(&format!(
        r#"
        [session]
        mode = "file"
        dir = "{dir}"
        key_env = "GAZE_BRIDGE_TEST_KEY_ACTIVE"
        max_sessions = 1

        [servers.mail]
        command = "mail"
        "#,
        dir = dir.path().display(),
    ))
    .expect("config");
    let store = Arc::new(BridgeSessionStore::from_config(&config.session).expect("store"));

    let sid_a = "01HRT7K6P6X5Q9M0V8YQ4N7T01";
    let sid_b = "01HRT7K6P6X5Q9M0V8YQ4N7T02";

    // Acquire session A and tokenize something — do NOT release the Arc yet.
    let session_a = store.get(sid_a).await.expect("session A");
    let token_a = {
        let guard = session_a.lock().await;
        let token = guard
            .tokenize(&PiiClass::Email, RAW_EMAIL)
            .expect("tokenize");
        store.persist(sid_a, &guard).await.expect("persist A");
        token
    };
    // session_a Arc is still live: strong_count >= 2 (cache + this variable).
    assert_eq!(store.len().await, 1);

    // Attempting to insert a second session while A is active must be rejected.
    // The eviction loop should detect that session A is in use and refuse to
    // evict it, returning LimitExceeded instead of inserting B over a live A.
    match store.get(sid_b).await {
        Err(err) => assert!(
            matches!(err, gaze_mcp_bridge::BridgeError::LimitExceeded(_)),
            "expected LimitExceeded, got {err:?}"
        ),
        Ok(_) => panic!("B must be rejected while A is active"),
    }
    // The cache must not have grown: B was rejected, A is still the sole entry.
    assert_eq!(store.len().await, 1, "cache must not grow past the cap");

    // A must still be canonical: the Arc we hold must still resolve the correct
    // token — there must be no second independent Session for A in existence.
    assert_eq!(
        session_a.lock().await.restore(&token_a),
        Some(RAW_EMAIL.to_string()),
        "held session A must still contain the token after failed B insertion"
    );

    // Drop A's Arc — now only the cache holds the reference (strong_count == 1).
    drop(session_a);

    // Now B's insertion should succeed: A is inactive and can be evicted.
    {
        let session_b = store
            .get(sid_b)
            .await
            .expect("B must succeed after A is released");
        assert_eq!(store.len().await, 1, "B evicted A, cache stays at cap");
        drop(session_b);
    }

    // Reloading A from disk must restore the persisted token (proving the
    // eviction/reload path preserves reversibility and does not corrupt the
    // on-disk snapshot).
    let reloaded_a = store.get(sid_a).await.expect("reload A");
    assert_eq!(
        reloaded_a.lock().await.restore(&token_a),
        Some(RAW_EMAIL.to_string()),
        "A must be restorable from disk after eviction"
    );
}

proptest::proptest! {
    #[test]
    fn audit_event_serializes_paths_only(local in "[a-z]{1,12}") {
        let raw = format!("{local}@example.invalid");
        prop_assert!(ArgPath::root().child("x@y.z").is_err());
        prop_assert!(ArgPath::root().child(&raw).is_err());
        let mut event = BridgeAuditEvent::new(
            "01HRT7K6P6X5Q9M0V8YQ4N7TBC".to_string(),
            SID_A,
            "mail".to_string(),
            "send".to_string(),
            DecisionOutcome::Allowed,
            "test",
        );
        event.arg_paths_affected.push(ArgPath::root().child("to").expect("path"));
        let serialized = serde_json::to_string(&event).expect("json");
        prop_assert!(!serialized.contains(&raw));
        prop_assert!(!serialized.contains("@example.invalid"));
    }
}

#[tokio::test]
async fn rmcp_child_process_fixture_spawns_lists_calls_and_bridge_redacts() {
    let temp = TempDir::new().expect("tempdir");
    let script = temp.path().join("fixture_mcp.py");
    write_python_fixture(&script);

    let spec = ServerSpec {
        command: "python3".to_string(),
        args: vec![script.display().to_string()],
        env: BTreeMap::new(),
        cwd: None,
    };
    let client = RmcpChildClient::spawn(&spec).await.expect("client");
    let tools = client
        .list_tools(Duration::from_secs(5))
        .await
        .expect("tools");
    assert_eq!(tools[0].raw_name, "send");

    let mut clients: BTreeMap<String, Arc<dyn DownstreamClient>> = BTreeMap::new();
    clients.insert("mail".to_string(), client);
    let registry = Arc::new(
        BridgeRegistry::discover(clients, Duration::from_secs(5))
            .await
            .expect("registry"),
    );
    let audit_path = temp.path().join("audit.jsonl");
    let store = Arc::new(BridgeSessionStore::ephemeral());
    let host = BridgeHostBuilder::new(
        registry,
        Arc::clone(&store),
        Arc::new(gaze_mcp_bridge::FileBridgeAuditSink::new(&audit_path)),
    )
    .auth(Arc::new(AllowAuth))
    .policy(allow_to_policy())
    .build();
    let token = seed_email_token(&store, SID_A).await;
    let response = host
        .dispatch(
            &Principal::new("agent"),
            "mail.send",
            json!({"to": token}),
            Some(SID_A),
        )
        .await
        .expect("dispatch");
    let rendered = serde_json::to_string(&response.payload).expect("json");
    assert!(!rendered.contains(RAW_EMAIL));
    let audit = std::fs::read_to_string(audit_path).expect("audit");
    assert!(!audit.contains(RAW_EMAIL));
}

#[tokio::test]
async fn child_stderr_is_not_inherited_by_bridge_process() {
    let temp = TempDir::new().expect("tempdir");
    let script = temp.path().join("fixture_mcp.py");
    write_python_fixture(&script);

    let output = Command::new(std::env::current_exe().expect("current test binary"))
        .arg("--exact")
        .arg("rmcp_child_stderr_capture_helper")
        .arg("--ignored")
        .arg("--nocapture")
        .env("GAZE_BRIDGE_STDERR_HELPER", "1")
        .env("GAZE_BRIDGE_STDERR_FIXTURE", &script)
        .output()
        .expect("run stderr helper");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "stderr helper failed: {combined}");
    assert!(
        !combined.contains(RAW_EMAIL),
        "bridge process leaked raw stderr: {combined}"
    );
}

#[tokio::test]
#[ignore = "invoked by child_stderr_is_not_inherited_by_bridge_process"]
async fn rmcp_child_stderr_capture_helper() {
    if std::env::var_os("GAZE_BRIDGE_STDERR_HELPER").is_none() {
        return;
    }
    let script = std::env::var("GAZE_BRIDGE_STDERR_FIXTURE").expect("fixture path");
    let spec = ServerSpec {
        command: "python3".to_string(),
        args: vec![script],
        env: BTreeMap::new(),
        cwd: None,
    };
    let client = RmcpChildClient::spawn(&spec).await.expect("client");
    client
        .list_tools(Duration::from_secs(5))
        .await
        .expect("tools");
    client
        .call_tool("send", json!({"to": RAW_EMAIL}), Duration::from_secs(5))
        .await
        .expect("call");
}

fn read_all_files(dir: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        if entry.file_type().expect("file type").is_file() {
            out.extend(std::fs::read(entry.path()).expect("read file"));
        }
    }
    out
}

fn write_python_fixture(path: &Path) {
    let script = r#"
import json
import sys

def send(obj):
    sys.stdout.write(json.dumps(obj, separators=(",", ":")) + "\n")
    sys.stdout.flush()

for line in sys.stdin:
    if not line.strip():
        continue
    msg = json.loads(line)
    mid = msg.get("id")
    method = msg.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.0.0"}}})
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        send({"jsonrpc":"2.0","id":mid,"result":{"tools":[{"name":"send","description":"send","inputSchema":{"type":"object","properties":{"to":{"type":"string"}}}}]}})
    elif method == "tools/call":
        args = msg.get("params", {}).get("arguments", {})
        print("stderr planted alice@example.invalid", file=sys.stderr, flush=True)
        send({"jsonrpc":"2.0","id":mid,"result":{"content":[{"type":"text","text":"sent to " + args.get("to", "")}],"isError":False}})
    else:
        send({"jsonrpc":"2.0","id":mid,"error":{"code":-32601,"message":"unknown"}})
"#;
    std::fs::write(path, script).expect("write fixture");
}

#[tokio::test]
async fn r2_original_w1_file_cap_retains_active_session_identity() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_EVICT", "44".repeat(32));
    let config = BridgeConfig::from_toml_str(&session_cap_config(Some(dir.path()), 1)).unwrap();
    let store = BridgeSessionStore::from_config(&config.session).unwrap();
    let a = "01HRT7K6P6X5Q9M0V8YQ4N7T01";
    let first = store.get(a).await.unwrap();
    first
        .lock()
        .await
        .tokenize(&PiiClass::Email, RAW_EMAIL)
        .unwrap();
    let _ = store.get("01HRT7K6P6X5Q9M0V8YQ4N7T02").await;
    let reacquired = store.get(a).await.unwrap();
    assert!(
        Arc::ptr_eq(&first, &reacquired),
        "active session was replaced by a stale independent session"
    );
}

#[tokio::test]
async fn r2_failed_persist_must_not_allow_dirty_session_eviction() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_EVICT", "44".repeat(32));
    let config = BridgeConfig::from_toml_str(&session_cap_config(Some(dir.path()), 1)).unwrap();
    let store = BridgeSessionStore::from_config(&config.session).unwrap();
    let sid_a = "01HRT7K6P6X5Q9M0V8YQ4N7T01";
    let sid_b = "01HRT7K6P6X5Q9M0V8YQ4N7T02";
    let a = store.get(sid_a).await.unwrap();
    let token = {
        let guard = a.lock().await;
        let token = guard
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .unwrap();
        // Simulate a persistence failure without permissions or timing dependencies.
        std::fs::remove_dir(dir.path()).unwrap();
        std::fs::write(dir.path(), "unavailable session directory").unwrap();
        assert!(store.persist(sid_a, &guard).await.is_err());
        std::fs::remove_file(dir.path()).unwrap();
        std::fs::create_dir(dir.path()).unwrap();
        token
    };
    drop(a);
    let _ = store.get(sid_b).await;
    let recovered = store.get(sid_a).await.unwrap();
    assert_eq!(
        recovered.lock().await.restore(&token),
        Some("alice@example.invalid".to_string()),
        "failed persistence must not make unpersisted token mappings eligible for eviction"
    );
}

#[tokio::test]
async fn eviction_persists_unpersisted_candidate_before_restore_round_trip() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_EVICT", "44".repeat(32));
    let config = BridgeConfig::from_toml_str(&session_cap_config(Some(dir.path()), 1)).unwrap();
    let store = BridgeSessionStore::from_config(&config.session).unwrap();
    let a = store.get(SID_A).await.unwrap();
    let token = a
        .lock()
        .await
        .tokenize(&PiiClass::Email, RAW_EMAIL)
        .unwrap();
    drop(a);
    let b = store.get(SID_B).await.unwrap();
    assert_eq!(store.len().await, 1);
    let reloaded = BridgeSessionStore::from_config(&config.session).unwrap();
    let restored = reloaded.get(SID_A).await.unwrap();
    assert_eq!(
        restored.lock().await.restore(&token),
        Some(RAW_EMAIL.to_string())
    );
    assert!(!String::from_utf8_lossy(&read_all_files(dir.path())).contains(RAW_EMAIL));
    drop(b);
}

#[tokio::test]
async fn eviction_rejects_admission_when_candidate_persistence_fails() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_EVICT", "44".repeat(32));
    let config = BridgeConfig::from_toml_str(&session_cap_config(Some(dir.path()), 1)).unwrap();
    let store = BridgeSessionStore::from_config(&config.session).unwrap();
    let a = store.get(SID_A).await.unwrap();
    let token = a
        .lock()
        .await
        .tokenize(&PiiClass::Email, RAW_EMAIL)
        .unwrap();
    drop(a);
    std::fs::remove_dir(dir.path()).unwrap();
    std::fs::write(dir.path(), "unavailable session directory").unwrap();
    let result = store.get(SID_B).await;
    assert!(
        matches!(result, Err(gaze_mcp_bridge::BridgeError::SessionStore(ref message))
        if message.starts_with("create session dir failed:")),
        "admission must reject a failed eviction-time persist"
    );
    assert_eq!(store.len().await, 1);
    let retained = store.get(SID_A).await.unwrap();
    assert_eq!(
        retained.lock().await.restore(&token),
        Some(RAW_EMAIL.to_string())
    );
    drop(retained);
    std::fs::remove_file(dir.path()).unwrap();
    std::fs::create_dir(dir.path()).unwrap();
    drop(
        store
            .get(SID_B)
            .await
            .expect("retry after filesystem recovery"),
    );
    assert_eq!(
        store.get(SID_A).await.unwrap().lock().await.restore(&token),
        Some(RAW_EMAIL.to_string())
    );
}

#[tokio::test]
async fn eviction_preserves_unpersisted_state_after_caller_cancellation() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_EVICT", "44".repeat(32));
    let config = BridgeConfig::from_toml_str(&session_cap_config(Some(dir.path()), 1)).unwrap();
    let store = Arc::new(BridgeSessionStore::from_config(&config.session).unwrap());
    let (ready, mutated) = tokio::sync::oneshot::channel();
    let caller_store = Arc::clone(&store);
    let caller = tokio::spawn(async move {
        let session = caller_store.get(SID_A).await.unwrap();
        let guard = session.lock().await;
        let token = guard.tokenize(&PiiClass::Email, RAW_EMAIL).unwrap();
        ready.send(token).unwrap();
        std::future::pending::<()>().await;
        drop(guard);
    });
    let token = mutated.await.unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    drop(store.get(SID_B).await.unwrap());
    let recovered = store.get(SID_A).await.unwrap();
    assert_eq!(
        recovered.lock().await.restore(&token),
        Some(RAW_EMAIL.to_string())
    );
}

#[tokio::test]
async fn r3_weak_upgrade_during_eviction_preserves_canonical_manifest() {
    use std::future::Future;
    use std::task::Poll;

    let dir = TempDir::new().unwrap();
    std::env::set_var("GAZE_BRIDGE_TEST_KEY_EVICT", "44".repeat(32));
    let config = BridgeConfig::from_toml_str(&session_cap_config(Some(dir.path()), 1)).unwrap();
    let store = BridgeSessionStore::from_config(&config.session).unwrap();
    let a = store.get(SID_A).await.unwrap();
    let weak = Arc::downgrade(&a);
    drop(a);

    // Poll admission into persistence I/O, after its inactivity check.
    let mut admission = Box::pin(store.get(SID_B));
    std::future::poll_fn(|cx| {
        assert!(admission.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let revived = weak
        .upgrade()
        .expect("candidate remains cached during persist");
    drop(admission.await);

    // Public Weak::upgrade bypasses the cache lock and creates a live caller.
    let token = revived
        .lock()
        .await
        .tokenize(&PiiClass::Email, RAW_EMAIL)
        .unwrap();
    let reacquired = store.get(SID_A).await.unwrap();
    assert_eq!(
        reacquired.lock().await.restore(&token),
        Some(RAW_EMAIL.to_string()),
        "eviction split the canonical manifest from a revived public session"
    );
    assert!(Arc::ptr_eq(&revived, &reacquired));
}
