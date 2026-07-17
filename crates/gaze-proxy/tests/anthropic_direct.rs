use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use gaze::{Action, ClassRule, DefaultRule, PiiClass, Pipeline};
use gaze_proxy::adapters::anthropic::DEFAULT_ANTHROPIC_UPSTREAM;
use gaze_proxy::adapters::AnthropicAdapter;
use gaze_proxy::{
    AuthenticatedPrincipal, PrincipalContext, PrincipalResolveError, PrincipalResolver,
    ProtocolContract, ProviderAdapter, ProxyConfig, ProxyErrorCode, SessionPolicy,
    SessionRegistryConfig, ANTHROPIC_PROXY_ERROR_FRAME, ANTHROPIC_PROXY_PING_FRAME,
};
use gaze_recognizers::RegexDetector;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

const EMAIL: &str = "alice@example.invalid";

struct RecordingResolver {
    called: Arc<AtomicBool>,
}

impl PrincipalResolver for RecordingResolver {
    fn resolve(
        &self,
        _context: PrincipalContext<'_>,
    ) -> Result<AuthenticatedPrincipal, PrincipalResolveError> {
        self.called.store(true, Ordering::SeqCst);
        AuthenticatedPrincipal::try_from_canonical_bytes(b"synthetic-principal".to_vec())
    }
}

#[derive(Clone)]
struct UpstreamState {
    captures: Arc<Mutex<Vec<CapturedRequest>>>,
    rate_limited: bool,
    invalid_stream: bool,
    delayed_stream: bool,
}

struct CapturedRequest {
    path_and_query: String,
    headers: HeaderMap,
    body: Vec<u8>,
}

struct RunningServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct MockUpstream {
    origin: Url,
    captures: Arc<Mutex<Vec<CapturedRequest>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn email_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}

async fn capture_anthropic_request(
    State(state): State<UpstreamState>,
    request: Request<Body>,
) -> axum::response::Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 2 * 1024 * 1024).await.unwrap().to_vec();
    let request_json: Value = serde_json::from_slice(&body).unwrap();
    state.captures.lock().await.push(CapturedRequest {
        path_and_query: parts
            .uri
            .path_and_query()
            .map_or_else(|| parts.uri.path().to_string(), ToString::to_string),
        headers: parts.headers,
        body,
    });

    if state.rate_limited {
        return axum::response::Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("content-type", "application/json")
            .header("retry-after", "7")
            .body(Body::from(
                r#"{"type":"error","error":{"type":"rate_limit_error","message":"synthetic"}}"#,
            ))
            .unwrap();
    }
    if state.invalid_stream {
        return axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from(
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"must-not-replay\"}}\n\n",
            ))
            .unwrap();
    }

    let protected = request_json["messages"][0]["content"]
        .as_str()
        .unwrap_or("synthetic");
    if request_json["stream"].as_bool().unwrap_or(false) {
        let escaped = serde_json::to_string(protected).unwrap();
        let frames = format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-test\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{{\"input_tokens\":1,\"output_tokens\":0}}}}}}\n\nevent: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\nevent: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":{escaped}}}}}\n\nevent: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\nevent: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":1}}}}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
        );
        if state.delayed_stream {
            let chunks = futures_util::stream::once(async move {
                tokio::time::sleep(Duration::from_millis(1_100)).await;
                Ok::<_, std::io::Error>(Bytes::from(frames))
            });
            return axum::response::Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(chunks))
                .unwrap();
        }
        axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from(frames))
            .unwrap()
    } else {
        Json(json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude-test",
            "content": [{"type": "text", "text": protected}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }))
        .into_response()
    }
}

async fn spawn_upstream() -> MockUpstream {
    spawn_upstream_with_mode(false, false, false).await
}

async fn spawn_upstream_with_rate_limit(rate_limited: bool) -> MockUpstream {
    spawn_upstream_with_mode(rate_limited, false, false).await
}

async fn spawn_upstream_with_mode(
    rate_limited: bool,
    invalid_stream: bool,
    delayed_stream: bool,
) -> MockUpstream {
    let captures = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/messages", post(capture_anthropic_request))
        .with_state(UpstreamState {
            captures: Arc::clone(&captures),
            rate_limited,
            invalid_stream,
            delayed_stream,
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    MockUpstream {
        origin: Url::parse(&format!("http://{addr}")).unwrap(),
        captures,
        handle,
    }
}

async fn spawn_proxy(adapter: AnthropicAdapter) -> RunningServer {
    let bind = unused_local_addr();
    let config = ProxyConfig::anthropic_direct(bind, adapter);
    let handle = tokio::spawn(async move {
        gaze_proxy::serve(config, Arc::new(email_pipeline()))
            .await
            .unwrap();
    });
    wait_for_proxy(bind).await;
    RunningServer {
        base_url: format!("http://{bind}"),
        handle,
    }
}

fn unused_local_addr() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

async fn wait_for_proxy(bind: SocketAddr) {
    let client = Client::new();
    let health_url = format!("http://{bind}/_gaze_proxy/healthz");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if client
            .get(&health_url)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn sdk_request(stream: bool) -> Value {
    json!({
        "model": "claude-test",
        "max_tokens": 32,
        "messages": [{"role": "user", "content": EMAIL}],
        "stream": stream
    })
}

fn sdk_client_request(
    client: &Client,
    proxy: &RunningServer,
    stream: bool,
) -> reqwest::RequestBuilder {
    client
        .post(format!("{}/v1/messages", proxy.base_url))
        .header("x-api-key", "sdk-synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .header("user-agent", "Anthropic/Python synthetic")
        .header("x-stainless-lang", "python")
        .header("x-stainless-package-version", "0.0.0-test")
        .header("x-stainless-os", "MacOS")
        .header("x-stainless-arch", "arm64")
        .header("x-stainless-runtime", "CPython")
        .header("x-stainless-runtime-version", "3.13.0")
        .header("authorization", "Bearer synthetic-local-only")
        .header("cookie", "synthetic=discard")
        .json(&sdk_request(stream))
}

#[test]
fn direct_constructor_and_continuity_builder_defaults_are_pinned() {
    assert_eq!(DEFAULT_ANTHROPIC_UPSTREAM, "https://api.anthropic.com");
    let upstream = Url::parse("https://api.anthropic.com").unwrap();
    let ephemeral = AnthropicAdapter::new(upstream.clone());
    let ephemeral_contract = ephemeral.contract();
    assert!(matches!(
        ephemeral_contract.protocol(),
        ProtocolContract::Codec(_)
    ));
    assert!(matches!(
        ephemeral_contract.session_policy(),
        Some(SessionPolicy::EphemeralSingleRequest)
    ));
    assert_eq!(ephemeral.allowed_versions(), &["2023-06-01"]);
    assert!(ephemeral.allowed_betas().is_empty());

    let registry = SessionRegistryConfig::default()
        .try_with_max_active_sessions(8)
        .unwrap();
    let continuity = AnthropicAdapter::builder(upstream)
        .session_registry_config(registry)
        .connect_timeout(Duration::from_secs(2))
        .unwrap()
        .request_timeout(Duration::from_secs(30))
        .unwrap()
        .total_timeout(Duration::from_secs(60))
        .unwrap()
        .build()
        .unwrap();
    assert!(matches!(
        continuity.contract().session_policy(),
        Some(SessionPolicy::OpaqueHeaderContinuity(config)) if config == registry
    ));
    assert_eq!(continuity.allowed_versions(), &["2023-06-01"]);
    assert!(continuity.allowed_betas().is_empty());
}

#[test]
fn builder_rejects_an_untrusted_upstream_origin() {
    for invalid in [
        "http://example.invalid",
        "http://localhost",
        "https://synthetic:credential@127.0.0.1",
        "https://example.invalid/v1",
        "https://example.invalid?synthetic=1",
        "https://example.invalid#synthetic",
    ] {
        let error = AnthropicAdapter::builder(Url::parse(invalid).unwrap())
            .build()
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::InvalidUpstreamUrl);
    }
    AnthropicAdapter::builder(Url::parse("http://127.0.0.1:8787").unwrap())
        .build()
        .unwrap();

    for invalid in [Duration::ZERO, Duration::from_secs(31)] {
        assert!(
            AnthropicAdapter::builder(Url::parse("https://api.anthropic.com").unwrap())
                .ping_interval(invalid)
                .is_err()
        );
    }
    AnthropicAdapter::builder(Url::parse("https://api.anthropic.com").unwrap())
        .ping_interval(Duration::from_secs(1))
        .unwrap()
        .max_headers(32)
        .unwrap()
        .max_header_name_bytes(64)
        .unwrap()
        .max_header_value_bytes(4 * 1024)
        .unwrap()
        .max_header_bytes(32 * 1024)
        .unwrap()
        .build()
        .unwrap();
}

#[tokio::test]
async fn official_sdk_shaped_non_stream_flow_uses_exact_route_headers_and_proved_body() {
    let upstream = spawn_upstream().await;
    let proxy = spawn_proxy(AnthropicAdapter::new(upstream.origin.clone())).await;
    let response = sdk_client_request(&Client::new(), &proxy, false)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_json: Value = response.json().await.unwrap();
    assert_eq!(response_json["content"][0]["text"], EMAIL);

    let captures = upstream.captures.lock().await;
    assert_eq!(captures.len(), 1);
    let captured = &captures[0];
    assert_eq!(captured.path_and_query, "/v1/messages");
    assert_eq!(
        captured.headers.get("x-api-key").unwrap(),
        "sdk-synthetic-key"
    );
    assert_eq!(
        captured.headers.get("anthropic-version").unwrap(),
        "2023-06-01"
    );
    assert_eq!(
        captured.headers.get("content-type").unwrap(),
        "application/json"
    );
    for dropped in [
        "authorization",
        "cookie",
        "x-gaze-session-id",
        "x-stainless-lang",
        "x-stainless-package-version",
        "x-stainless-os",
        "x-stainless-arch",
        "x-stainless-runtime",
        "x-stainless-runtime-version",
    ] {
        assert!(captured.headers.get(dropped).is_none());
    }
    assert!(!captured
        .body
        .windows(EMAIL.len())
        .any(|window| window == EMAIL.as_bytes()));
    assert!(std::str::from_utf8(&captured.body)
        .unwrap()
        .contains("Email_1"));
}

#[tokio::test]
async fn official_sdk_shaped_stream_flow_restores_only_after_complete_sse_proof() {
    let upstream = spawn_upstream().await;
    let proxy = spawn_proxy(AnthropicAdapter::new(upstream.origin.clone())).await;
    let response = sdk_client_request(&Client::new(), &proxy, true)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    let body = response.text().await.unwrap();
    assert!(body.contains(EMAIL));
    assert!(body.contains("event: message_stop"));
}

#[tokio::test]
async fn ephemeral_rejects_session_header_and_direct_route_is_exact() {
    let upstream = spawn_upstream().await;
    let proxy = spawn_proxy(AnthropicAdapter::new(upstream.origin.clone())).await;
    let client = Client::new();

    let unexpected_session = sdk_client_request(&client, &proxy, false)
        .header("x-gaze-session-id", "123e4567-e89b-42d3-a456-426614174000")
        .send()
        .await
        .unwrap();
    assert_eq!(unexpected_session.status(), StatusCode::BAD_REQUEST);

    for path in ["/v1/messages/", "/v1/messages?synthetic=1"] {
        let response = client
            .post(format!("{}{}", proxy.base_url, path))
            .header("x-api-key", "sdk-synthetic-key")
            .header("anthropic-version", "2023-06-01")
            .json(&sdk_request(false))
            .send()
            .await
            .unwrap();
        assert!(!response.status().is_success());
    }
    let wrong_method = client
        .get(format!("{}/v1/messages", proxy.base_url))
        .header("x-api-key", "sdk-synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .unwrap();
    assert!(!wrong_method.status().is_success());
    assert!(upstream.captures.lock().await.is_empty());
}

#[tokio::test]
async fn continuity_requires_canonical_uuid_v4_and_reuses_the_committed_mapping() {
    const SESSION_ID: &str = "123e4567-e89b-42d3-a456-426614174000";

    let upstream = spawn_upstream().await;
    let adapter = AnthropicAdapter::builder(upstream.origin.clone())
        .session_registry_config(SessionRegistryConfig::default())
        .build()
        .unwrap();
    let proxy = spawn_proxy(adapter).await;
    let client = Client::new();

    let missing = sdk_client_request(&client, &proxy, false)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    let missing_error: Value = missing.json().await.unwrap();
    assert_eq!(
        missing_error["error"]["code"],
        Value::String("SessionIdentityRequired".to_string())
    );

    for invalid in [
        "synthetic-session",
        "123e4567-e89b-12d3-a456-426614174000",
        "123e4567-e89b-42d3-7456-426614174000",
        "123E4567-E89B-42D3-A456-426614174000",
    ] {
        let response = sdk_client_request(&client, &proxy, false)
            .header("x-gaze-session-id", invalid)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(upstream.captures.lock().await.is_empty());

    for _ in 0..2 {
        let response = sdk_client_request(&client, &proxy, false)
            .header("x-gaze-session-id", SESSION_ID)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response_json: Value = response.json().await.unwrap();
        assert_eq!(response_json["content"][0]["text"], EMAIL);
    }

    let captures = upstream.captures.lock().await;
    assert_eq!(captures.len(), 2);
    let first: Value = serde_json::from_slice(&captures[0].body).unwrap();
    let second: Value = serde_json::from_slice(&captures[1].body).unwrap();
    assert_eq!(
        first["messages"][0]["content"],
        second["messages"][0]["content"]
    );
    assert_ne!(first["messages"][0]["content"], EMAIL);
    assert!(captures
        .iter()
        .all(|capture| capture.headers.get("x-gaze-session-id").is_none()));
}

#[tokio::test]
async fn trusted_principal_resolution_precedes_request_body_materialization() {
    let upstream = spawn_upstream().await;
    let called = Arc::new(AtomicBool::new(false));
    let adapter = AnthropicAdapter::builder(upstream.origin.clone())
        .principal_resolver(Arc::new(RecordingResolver {
            called: Arc::clone(&called),
        }))
        .build()
        .unwrap();
    let proxy = spawn_proxy(adapter).await;
    let (body_sender, body_receiver) = tokio::sync::mpsc::channel(1);
    let request = Client::new()
        .post(format!("{}/v1/messages", proxy.base_url))
        .header("content-type", "application/json")
        .header("x-api-key", "sdk-synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .header("x-gaze-session-id", "123e4567-e89b-42d3-a456-426614174000")
        .body(reqwest::Body::wrap_stream(
            tokio_stream::wrappers::ReceiverStream::new(body_receiver),
        ));
    let request_task = tokio::spawn(async move { request.send().await.unwrap() });

    tokio::time::timeout(Duration::from_secs(2), async {
        while !called.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("resolver must run while the body stream is still withheld");
    body_sender
        .send(Ok::<_, std::io::Error>(Bytes::from(
            serde_json::to_vec(&sdk_request(false)).unwrap(),
        )))
        .await
        .unwrap();
    drop(body_sender);

    assert_eq!(request_task.await.unwrap().status(), StatusCode::OK);
    assert_eq!(upstream.captures.lock().await.len(), 1);
}

#[tokio::test]
async fn beta_header_is_denied_by_default_and_forwarded_only_when_allowlisted() {
    const BETA: &str = "tools-2025-01-01";

    let upstream = spawn_upstream().await;
    let default_proxy = spawn_proxy(AnthropicAdapter::new(upstream.origin.clone())).await;
    let denied = sdk_client_request(&Client::new(), &default_proxy, false)
        .header("anthropic-beta", BETA)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::BAD_REQUEST);
    assert!(upstream.captures.lock().await.is_empty());
    drop(default_proxy);

    let adapter = AnthropicAdapter::builder(upstream.origin.clone())
        .allow_beta(BETA)
        .unwrap()
        .build()
        .unwrap();
    let allowlisted_proxy = spawn_proxy(adapter).await;
    let allowed = sdk_client_request(&Client::new(), &allowlisted_proxy, false)
        .header("anthropic-beta", BETA)
        .header("x-gaze-session-id", "123e4567-e89b-42d3-a456-426614174000")
        .send()
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    let captures = upstream.captures.lock().await;
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].headers.get("anthropic-beta").unwrap(), BETA);
}

#[tokio::test]
async fn stream_rate_limit_is_an_ordinary_429_with_validated_retry_after() {
    let upstream = spawn_upstream_with_rate_limit(true).await;
    let proxy = spawn_proxy(AnthropicAdapter::new(upstream.origin.clone())).await;
    let response = sdk_client_request(&Client::new(), &proxy, true)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers().get("retry-after").unwrap(), "7");
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );
    let body = response.bytes().await.unwrap();
    assert!(!body
        .windows(b"event:".len())
        .any(|window| window == b"event:"));
    assert_eq!(upstream.captures.lock().await.len(), 1);
}

#[tokio::test]
async fn duplicate_and_folded_singleton_headers_fail_closed_before_upstream_io() {
    let upstream = spawn_upstream().await;
    let proxy = spawn_proxy(AnthropicAdapter::new(upstream.origin.clone())).await;
    let client = Client::new();

    let duplicate_key = sdk_client_request(&client, &proxy, false)
        .header("x-api-key", "second-synthetic-key")
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate_key.status(), StatusCode::BAD_REQUEST);

    let duplicate_version = sdk_client_request(&client, &proxy, false)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate_version.status(), StatusCode::BAD_REQUEST);

    let folded_key = client
        .post(format!("{}/v1/messages", proxy.base_url))
        .header("x-api-key", "first-synthetic-key, second-synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .json(&sdk_request(false))
        .send()
        .await
        .unwrap();
    assert_eq!(folded_key.status(), StatusCode::BAD_REQUEST);

    let oversized_key = client
        .post(format!("{}/v1/messages", proxy.base_url))
        .header("x-api-key", "k".repeat(8 * 1024 + 1))
        .header("anthropic-version", "2023-06-01")
        .json(&sdk_request(false))
        .send()
        .await
        .unwrap();
    assert!(!oversized_key.status().is_success());

    let mut excessive_headers = client
        .post(format!("{}/v1/messages", proxy.base_url))
        .header("x-api-key", "sdk-synthetic-key")
        .header("anthropic-version", "2023-06-01");
    for index in 0..65 {
        excessive_headers = excessive_headers.header(format!("x-synthetic-{index}"), "bounded");
    }
    let excessive_headers = excessive_headers
        .json(&sdk_request(false))
        .send()
        .await
        .unwrap();
    assert!(!excessive_headers.status().is_success());
    assert!(upstream.captures.lock().await.is_empty());
    drop(proxy);

    let continuity = spawn_proxy(
        AnthropicAdapter::builder(upstream.origin.clone())
            .build()
            .unwrap(),
    )
    .await;
    let duplicate_session = sdk_client_request(&client, &continuity, false)
        .header("x-gaze-session-id", "123e4567-e89b-42d3-a456-426614174000")
        .header("x-gaze-session-id", "123e4567-e89b-42d3-a456-426614174000")
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate_session.status(), StatusCode::BAD_REQUEST);
    assert!(upstream.captures.lock().await.is_empty());
}

#[tokio::test]
async fn closed_errors_and_debug_omit_credentials_pii_and_upstream_text() {
    let raw_origin = "https://synthetic:credential@127.0.0.1?private=term";
    let error = AnthropicAdapter::builder(Url::parse(raw_origin).unwrap())
        .build()
        .unwrap_err();
    for rendered in [format!("{error}"), format!("{error:?}")] {
        assert!(!rendered.contains("credential"));
        assert!(!rendered.contains("example.invalid"));
        assert!(!rendered.contains("private"));
    }

    let upstream = spawn_upstream().await;
    let adapter = AnthropicAdapter::new(upstream.origin.clone());
    let rendered_adapter = format!("{adapter:?}");
    assert!(!rendered_adapter.contains(upstream.origin.as_str()));
    let proxy = spawn_proxy(adapter).await;
    let response = Client::new()
        .post(format!("{}/v1/messages", proxy.base_url))
        .header("x-api-key", "sdk-synthetic-key")
        .json(&sdk_request(false))
        .send()
        .await
        .unwrap();
    let body = response.text().await.unwrap();
    for forbidden in ["sdk-synthetic-key", EMAIL, upstream.origin.as_str()] {
        assert!(!body.contains(forbidden));
    }
    assert!(upstream.captures.lock().await.is_empty());
}

#[tokio::test]
async fn late_stream_proof_failure_emits_only_the_constant_safe_error_frame() {
    let upstream = spawn_upstream_with_mode(false, true, false).await;
    let proxy = spawn_proxy(AnthropicAdapter::new(upstream.origin.clone())).await;
    let response = sdk_client_request(&Client::new(), &proxy, true)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap(), ANTHROPIC_PROXY_ERROR_FRAME);
    assert_eq!(upstream.captures.lock().await.len(), 1);
}

#[tokio::test]
async fn delayed_stream_emits_only_the_compiled_ping_before_proved_replay() {
    let upstream = spawn_upstream_with_mode(false, false, true).await;
    let adapter = AnthropicAdapter::builder(upstream.origin.clone())
        .ping_interval(Duration::from_secs(1))
        .unwrap()
        .build()
        .unwrap();
    let proxy = spawn_proxy(adapter).await;
    let response = sdk_client_request(&Client::new(), &proxy, true)
        .header("x-gaze-session-id", "123e4567-e89b-42d3-a456-426614174000")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.bytes().await.unwrap();
    assert!(body.starts_with(ANTHROPIC_PROXY_PING_FRAME));
    assert!(body
        .windows(EMAIL.len())
        .any(|window| window == EMAIL.as_bytes()));
    assert_eq!(upstream.captures.lock().await.len(), 1);
}

#[tokio::test]
#[ignore = "requires GAZE_OFFICIAL_SDK_PYTHON pointing to Python with the official anthropic package"]
async fn unmodified_official_python_sdk_runs_non_stream_and_stream_against_loopback() {
    let python = std::env::var_os("GAZE_OFFICIAL_SDK_PYTHON")
        .expect("GAZE_OFFICIAL_SDK_PYTHON must identify the prepared SDK interpreter");
    let upstream = spawn_upstream_with_mode(false, false, true).await;
    let proxy = spawn_proxy(
        AnthropicAdapter::builder(upstream.origin.clone())
            .ping_interval(Duration::from_secs(1))
            .unwrap()
            .build()
            .unwrap(),
    )
    .await;
    let script_path =
        std::env::temp_dir().join(format!("gaze-anthropic-sdk-{}.py", std::process::id()));
    std::fs::write(
        &script_path,
        r#"import os
import anthropic

client = anthropic.Anthropic(
    api_key="sdk-synthetic-key",
    base_url=os.environ["ANTHROPIC_BASE_URL"],
    default_headers={"x-gaze-session-id": "123e4567-e89b-42d3-a456-426614174000"},
)
request = {
    "model": "claude-test",
    "max_tokens": 32,
    "messages": [{"role": "user", "content": "alice@example.invalid"}],
}
message = client.messages.create(**request)
print(message.content[0].text)
with client.messages.stream(**request) as stream:
    print("".join(stream.text_stream))
"#,
    )
    .unwrap();
    let base_url = proxy.base_url.clone();
    let script_for_process = script_path.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(python)
            .arg(script_for_process)
            .env("ANTHROPIC_BASE_URL", base_url)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    std::fs::remove_file(script_path).unwrap();

    assert!(
        output.status.success(),
        "official SDK failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .matches(EMAIL)
            .count(),
        2
    );
    let captures = upstream.captures.lock().await;
    assert_eq!(captures.len(), 2);
    for captured in captures.iter() {
        assert_eq!(captured.path_and_query, "/v1/messages");
        assert_eq!(
            captured.headers.get("x-api-key").unwrap(),
            "sdk-synthetic-key"
        );
        assert_eq!(
            captured.headers.get("anthropic-version").unwrap(),
            "2023-06-01"
        );
        for dropped in [
            "authorization",
            "cookie",
            "user-agent",
            "x-gaze-session-id",
            "x-stainless-lang",
            "x-stainless-package-version",
            "x-stainless-os",
            "x-stainless-arch",
            "x-stainless-runtime",
            "x-stainless-runtime-version",
        ] {
            assert!(captured.headers.get(dropped).is_none());
        }
        assert!(!captured
            .body
            .windows(EMAIL.len())
            .any(|window| window == EMAIL.as_bytes()));
    }
    let first: Value = serde_json::from_slice(&captures[0].body).unwrap();
    let second: Value = serde_json::from_slice(&captures[1].body).unwrap();
    assert_eq!(
        first["messages"][0]["content"],
        second["messages"][0]["content"]
    );
}
