//! Regression contract for the legacy (non-codec) adapter response header egress.
//!
//! The legacy response branch in `proxy_inner` (`server.rs`) forwards an upstream response
//! back to the downstream client. Before the fix it copied every upstream response header to
//! the downstream client, removing only `Content-Length`. Hop-by-hop headers (`Connection`,
//! `Keep-Alive`, `Transfer-Encoding`, ...), `Set-Cookie`, and arbitrary upstream infrastructure
//! metadata (`Server`, `Via`, `X-Request-Id`, ...) were forwarded verbatim — a violation of
//! RFC 7230 §6.1 (a proxy MUST NOT forward hop-by-hop headers) and an information /
//! operational separation leak.
//!
//! These tests assert the fixed behavior: the legacy response branch rebuilds a minimal
//! header set containing only the upstream `content-type`, mirroring the strict header
//! hygiene the direct/codec path applies (`validate_direct_response_head_with_config`).
//!
//! Fixtures are synthetic-only per AGENTS.md rule 2.

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use gaze::{Action, ClassRule, DefaultRule, PiiClass, Pipeline};
use gaze_proxy::adapters::{GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use gaze_recognizers::RegexDetector;
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use url::Url;

/// Synthetic PII marker. Never real PII (AGENTS.md rule 2).
const EMAIL: &str = "alice@example.invalid";

fn email_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}

#[derive(Clone)]
struct UpstreamState {
    responded: Arc<Mutex<Vec<HeaderMap>>>,
}

struct MockUpstream {
    base_url: Url,
    _responded: Arc<Mutex<Vec<HeaderMap>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct ProxyServer {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for ProxyServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Returns a response shape matching the request so PII restoration round-trips for both
/// adapters.
fn upstream_response_body(body: &Value) -> Value {
    if body.get("contents").is_some() {
        json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": body["contents"][0]["parts"][0]["text"]}],
                    "role": "model"
                }
            }]
        })
    } else {
        json!({ "choices": [{ "text": body["prompt"] }] })
    }
}

/// Upstream handler that emits hop-by-hop, `Set-Cookie`, and infrastructure metadata
/// headers on its JSON response. The proxy must strip all of these before the downstream
/// client sees them.
///
/// `Transfer-Encoding` is listed in `FORBIDDEN_DOWNSTREAM` but not injected here: injecting it
/// would corrupt the HTTP framing (axum sends a known `Content-Length` body while the header
/// advertises chunked), making the upstream response unreadable by the proxy's client. Its
/// absence downstream is still guaranteed by the `assert_only_allowed_headers_emitted` check,
/// which rejects any header other than `content-type`.
async fn leaky_upstream(
    State(state): State<UpstreamState>,
    _uri: Uri,
    _headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut leaky = HeaderMap::new();
    leaky.insert(
        axum::http::header::SET_COOKIE,
        HeaderValue::from_static("upstream-session=leaked; Path=/"),
    );
    leaky.insert(
        axum::http::header::CONNECTION,
        HeaderValue::from_static("keep-alive"),
    );
    leaky.insert("keep-alive", HeaderValue::from_static("timeout=5"));
    leaky.insert(
        axum::http::header::SERVER,
        HeaderValue::from_static("internal-infra-leak"),
    );
    leaky.insert("via", HeaderValue::from_static("1.1 egress-proxy"));
    leaky.insert(
        "x-upstream-server",
        HeaderValue::from_static("internal-infra-leak"),
    );
    leaky.insert(
        "x-synthetic-trace",
        HeaderValue::from_static("leaked-to-downstream"),
    );
    leaky.insert(
        "x-request-id",
        HeaderValue::from_static("upstream-req-12345"),
    );
    leaky.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    state.responded.lock().await.push(leaky.clone());
    (StatusCode::OK, leaky, Json(upstream_response_body(&body)))
}

/// Upstream that returns `text/event-stream` with the same leaky header set, to exercise
/// the legacy SSE transform path.
async fn sse_upstream(
    State(state): State<UpstreamState>,
    _uri: Uri,
    _headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        axum::http::header::CONNECTION,
        HeaderValue::from_static("keep-alive"),
    );
    headers.insert(
        axum::http::header::SERVER,
        HeaderValue::from_static("internal-infra-leak"),
    );
    state.responded.lock().await.push(headers.clone());
    let text = body["prompt"].as_str().unwrap_or("");
    let sse = format!("data: {{\"choices\":[{{\"text\":\"{text}\"}}]}}\n\n");
    (
        StatusCode::OK,
        headers,
        axum::body::Body::from(sse.into_bytes()),
    )
}

/// Upstream that omits `content-type` to exercise the default fallback.
async fn bare_upstream(
    State(state): State<UpstreamState>,
    _uri: Uri,
    _headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.responded.lock().await.push(HeaderMap::new());
    (
        StatusCode::OK,
        HeaderMap::new(),
        Json(upstream_response_body(&body)),
    )
}

async fn spawn_upstream_with(handler: axum::routing::MethodRouter<UpstreamState>) -> MockUpstream {
    let responded = Arc::new(Mutex::new(Vec::new()));
    let state = UpstreamState {
        responded: responded.clone(),
    };
    let app = Router::new().route("/{*path}", handler).with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    MockUpstream {
        base_url: Url::parse(&format!("http://{addr}")).unwrap(),
        _responded: responded,
        handle,
    }
}

async fn spawn_leaky_upstream() -> MockUpstream {
    spawn_upstream_with(post(leaky_upstream)).await
}

async fn spawn_sse_upstream() -> MockUpstream {
    spawn_upstream_with(post(sse_upstream)).await
}

async fn spawn_bare_upstream() -> MockUpstream {
    spawn_upstream_with(post(bare_upstream)).await
}

fn unused_local_addr() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

async fn spawn_proxy(adapter: Arc<dyn ProviderAdapter>) -> ProxyServer {
    let bind = unused_local_addr();
    let config = ProxyConfig::new(bind, vec![adapter]);
    let pipeline = Arc::new(email_pipeline());
    let handle = tokio::spawn(async move {
        gaze_proxy::serve(config, pipeline).await.unwrap();
    });
    wait_for_proxy(bind).await;
    ProxyServer {
        base_url: format!("http://{bind}"),
        handle,
    }
}

async fn wait_for_proxy(bind: SocketAddr) {
    let client = Client::new();
    let health_url = format!("http://{bind}/_gaze_proxy/healthz");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(response) = client.get(&health_url).send().await {
            if response.status().is_success() {
                return;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "proxy did not start at {bind}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The set of downstream response headers we expect the legacy path to permit — only
/// `content-type`. Everything else the upstream emits must be dropped.
const ALLOWED_DOWNSTREAM: &[&str] = &["content-type"];

/// Headers that the upstream emits but the downstream client must never receive.
const FORBIDDEN_DOWNSTREAM: &[&str] = &[
    "set-cookie",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "server",
    "via",
    "x-upstream-server",
    "x-synthetic-trace",
    "x-request-id",
];

/// Returns the names of all headers (lowercased) present in the downstream response,
/// excluding transport-injected names added by the client/server stack outside the
/// proxy's control (`content-length`, `date`, `host`).
fn proxy_emitted_header_names(headers: &HeaderMap) -> Vec<String> {
    headers
        .iter()
        .map(|(name, _)| name.as_str().to_string())
        .filter(|name| !matches!(name.as_str(), "content-length" | "date" | "host"))
        .collect::<Vec<_>>()
}

fn assert_forbidden_headers_stripped(headers: &HeaderMap) {
    for name in FORBIDDEN_DOWNSTREAM {
        assert!(
            !headers.contains_key(*name),
            "downstream received forbidden header `{name}` (value: {:?}) — \
             the legacy response path must strip hop-by-hop, Set-Cookie, and metadata headers",
            headers.get(*name).and_then(|v| v.to_str().ok()),
        );
    }
}

fn assert_only_allowed_headers_emitted(headers: &HeaderMap) {
    let emitted = proxy_emitted_header_names(headers);
    for name in &emitted {
        assert!(
            ALLOWED_DOWNSTREAM.contains(&name.as_str()),
            "unexpected downstream header `{name}` — \
             legacy path must emit only content-type among upstream-provided headers",
        );
    }
}

fn assert_content_type_preserved(headers: &HeaderMap, expected: &str) {
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert_eq!(
        ct, expected,
        "content-type must be preserved from the upstream response",
    );
}

fn assert_openai_body_round_trips(body: &Value) {
    assert_eq!(
        body["choices"][0]["text"], EMAIL,
        "OpenAI response body must round-trip PII restoration",
    );
}

fn assert_gemini_body_round_trips(body: &Value) {
    assert_eq!(
        body["candidates"][0]["content"]["parts"][0]["text"], EMAIL,
        "Gemini response body must round-trip PII restoration",
    )
}

fn openai_request() -> Value {
    json!({ "prompt": EMAIL })
}

fn gemini_request() -> Value {
    json!({
        "contents": [{
            "role": "user",
            "parts": [{"text": EMAIL}]
        }]
    })
}

async fn post_downstream(proxy: &ProxyServer, path: &str, body: &Value) -> reqwest::Response {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    client
        .post(format!("{}{path}", proxy.base_url))
        .header("authorization", "Bearer test-key")
        .header("content-type", "application/json")
        .json(body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn legacy_openai_response_strips_hop_by_hop_set_cookie_and_metadata_headers() {
    let upstream = spawn_leaky_upstream().await;
    let proxy = spawn_proxy(Arc::new(OpenAiAdapter::new(upstream.base_url.clone()))).await;
    let response = post_downstream(&proxy, "/v1/completions", &openai_request()).await;

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let body: Value = response.json().await.unwrap();
    assert_openai_body_round_trips(&body);

    assert_forbidden_headers_stripped(&headers);
    assert_content_type_preserved(&headers, "application/json");
    assert_only_allowed_headers_emitted(&headers);
}

#[tokio::test]
async fn legacy_gemini_response_strips_hop_by_hop_set_cookie_and_metadata_headers() {
    let upstream = spawn_leaky_upstream().await;
    let proxy = spawn_proxy(Arc::new(GeminiAdapter::new(upstream.base_url.clone()))).await;
    let response = post_downstream(
        &proxy,
        "/v1beta/models/gemini-test:generateContent",
        &gemini_request(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let body: Value = response.json().await.unwrap();
    assert_gemini_body_round_trips(&body);

    assert_forbidden_headers_stripped(&headers);
    assert_content_type_preserved(&headers, "application/json");
    assert_only_allowed_headers_emitted(&headers);
}

#[tokio::test]
async fn legacy_openai_response_preserves_sse_content_type_and_strips_metadata() {
    let upstream = spawn_sse_upstream().await;
    let proxy = spawn_proxy(Arc::new(OpenAiAdapter::new(upstream.base_url.clone()))).await;
    let response = post_downstream(&proxy, "/v1/completions", &openai_request()).await;

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    // The SSE content-type must reach the downstream so the legacy transform path stays correct.
    assert_content_type_preserved(&headers, "text/event-stream");
    // Even on the SSE path, hop-by-hop / metadata headers must be stripped.
    assert!(!headers.contains_key("connection"));
    assert!(!headers.contains_key("set-cookie"));
    assert!(!headers.contains_key("server"));
}

#[tokio::test]
async fn legacy_openai_response_defaults_to_application_json_when_upstream_omits_content_type() {
    let upstream = spawn_bare_upstream().await;
    let proxy = spawn_proxy(Arc::new(OpenAiAdapter::new(upstream.base_url.clone()))).await;
    let response = post_downstream(&proxy, "/v1/completions", &openai_request()).await;

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let body: Value = response.json().await.unwrap();
    assert_openai_body_round_trips(&body);

    assert_content_type_preserved(&headers, "application/json");
}

#[tokio::test]
async fn legacy_openai_downstream_content_length_matches_actual_body_not_upstream() {
    // A stale Content-Length from the upstream (after body rewriting on the legacy path) must
    // not reach the downstream client. The proxy emits a fresh body whose framing is
    // authoritative — any downstream content-length matches the actual bytes received.
    let upstream = spawn_leaky_upstream().await;
    let proxy = spawn_proxy(Arc::new(OpenAiAdapter::new(upstream.base_url.clone()))).await;
    let response = post_downstream(&proxy, "/v1/completions", &openai_request()).await;

    let headers = response.headers().clone();
    let bytes = response.bytes().await.unwrap();
    if let Some(cl) = headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
    {
        assert_eq!(
            cl,
            bytes.len(),
            "downstream content-length must match the actual proxy-emitted body, \
             not a stale upstream value",
        );
    }
}
