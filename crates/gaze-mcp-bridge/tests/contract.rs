use std::collections::BTreeMap;
use std::path::Path;
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
use gaze_mcp_core::{AuthError, AuthHook, DispatchError, DispatchHost, Principal, ToolDescriptor};
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

proptest::proptest! {
    #[test]
    fn audit_event_serializes_paths_only(local in "[a-z]{1,12}") {
        let raw = format!("{local}@example.invalid");
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
