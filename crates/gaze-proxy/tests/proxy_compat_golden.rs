use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, Uri};
use axum::routing::post;
use axum::{Json, Router};
use gaze::{Action, ClassRule, DefaultRule, PiiClass, Pipeline};
use gaze_proxy::adapters::{GeminiAdapter, OpenAiAdapter};
use gaze_proxy::daemon::DaemonConfig;
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use gaze_recognizers::RegexDetector;
use reqwest::{Client, Method, Response, StatusCode};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

const BODY_LIMIT: u64 = 2 * 1024 * 1024;

fn email_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}

#[derive(Clone, Debug)]
struct CapturedRequest {
    path: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

#[derive(Clone)]
struct UpstreamState {
    captured: Arc<Mutex<Vec<CapturedRequest>>>,
}

struct MockUpstream {
    base_url: Url,
    captured: Arc<Mutex<Vec<CapturedRequest>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl MockUpstream {
    async fn captured(&self) -> Vec<CapturedRequest> {
        self.captured.lock().await.clone()
    }
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct ProxyServer {
    bind: SocketAddr,
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for ProxyServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn spawn_upstream() -> MockUpstream {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let state = UpstreamState {
        captured: captured.clone(),
    };
    let app = Router::new()
        .route("/{*path}", post(capture_upstream))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    MockUpstream {
        base_url: Url::parse(&format!("http://{addr}")).unwrap(),
        captured,
        handle,
    }
}

async fn capture_upstream(
    State(state): State<UpstreamState>,
    uri: Uri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let path = uri.path().to_string();
    let captured_headers = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    state.captured.lock().await.push(CapturedRequest {
        path: path.clone(),
        headers: captured_headers,
        body: body.clone(),
    });

    let response = if path == "/v1/completions" {
        json!({
            "choices": [{
                "text": body["prompt"]
            }]
        })
    } else if path.starts_with("/v1beta/models/") && path.ends_with(":generateContent") {
        json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "text": body["contents"][0]["parts"][0]["text"]
                    }]
                }
            }]
        })
    } else {
        json!({"unexpected": true})
    };
    Json(response)
}

async fn spawn_proxy(
    adapters: Vec<Arc<dyn ProviderAdapter>>,
    body_limit_bytes: u64,
) -> ProxyServer {
    let bind = unused_local_addr();
    let mut config = ProxyConfig::new(bind, adapters);
    config.body_limit_bytes = body_limit_bytes;
    let pipeline = Arc::new(email_pipeline());
    let handle = tokio::spawn(async move {
        gaze_proxy::serve(config, pipeline).await.unwrap();
    });
    wait_for_proxy(bind).await;
    ProxyServer {
        bind,
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
        if let Ok(response) = client.get(&health_url).send().await {
            if response.status().is_success() {
                return;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "proxy did not start at {bind}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn adapters_for(upstream: &Url) -> Vec<Arc<dyn ProviderAdapter>> {
    vec![
        Arc::new(OpenAiAdapter::new(upstream.clone())),
        Arc::new(GeminiAdapter::new(upstream.clone())),
    ]
}

fn add_legacy_forwarded_headers(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request
        .header("authorization", "Bearer synthetic-local")
        .header("x-api-key", "synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .header("openai-beta", "synthetic-beta")
        .header("content-type", "application/json")
        .header("x-synthetic-sdk-metadata", "must-be-dropped")
}

fn assert_legacy_headers(captured: &CapturedRequest) {
    assert_eq!(
        captured.headers.get("authorization").map(String::as_str),
        Some("Bearer synthetic-local")
    );
    assert_eq!(
        captured.headers.get("x-api-key").map(String::as_str),
        Some("synthetic-key")
    );
    assert_eq!(
        captured
            .headers
            .get("anthropic-version")
            .map(String::as_str),
        Some("2023-06-01")
    );
    assert_eq!(
        captured.headers.get("openai-beta").map(String::as_str),
        Some("synthetic-beta")
    );
    assert_eq!(
        captured.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert!(!captured.headers.contains_key("x-synthetic-sdk-metadata"));
}

async fn assert_error(response: Response, status: StatusCode, name: &str) {
    assert_eq!(response.status(), status);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"error": name})
    );
}

#[test]
fn openai_and_gemini_route_matching_is_legacy_exact() {
    let upstream = Url::parse("https://example.invalid").unwrap();
    let openai = OpenAiAdapter::new(upstream.clone());
    let gemini = GeminiAdapter::new(upstream);

    for path in ["/v1/chat/completions", "/v1/completions", "/v1/responses"] {
        assert!(openai.matches_path(&Method::POST, path), "{path}");
    }
    for (method, path) in [
        (Method::GET, "/v1/chat/completions"),
        (Method::POST, "/v1/chat/completions/"),
        (Method::POST, "/v1/embeddings"),
    ] {
        assert!(!openai.matches_path(&method, path), "{method} {path}");
    }

    for path in [
        "/v1beta/models/gemini-test:generateContent",
        "/v1beta/models/gemini-test:streamGenerateContent",
        "/v1beta/models/gemini-test:countTokens",
    ] {
        assert!(gemini.matches_path(&Method::POST, path), "{path}");
    }
    for (method, path) in [
        (Method::GET, "/v1beta/models/gemini-test:generateContent"),
        (Method::POST, "/v1beta/models/gemini-test:generateContent/"),
        (
            Method::POST,
            "/v1beta/models/gemini-test:batchGenerateContent",
        ),
        (Method::POST, "/v1/models/gemini-test:generateContent"),
    ] {
        assert!(!gemini.matches_path(&method, path), "{method} {path}");
    }
}

#[tokio::test]
async fn legacy_non_stream_round_trip_auto_creates_sessions_and_forwards_headers() {
    let upstream = spawn_upstream().await;
    let proxy = spawn_proxy(adapters_for(&upstream.base_url), BODY_LIMIT).await;
    let client = Client::new();

    let openai_response =
        add_legacy_forwarded_headers(client.post(format!("{}/v1/completions", proxy.base_url)))
            .json(&json!({
                "model": "gpt-test",
                "prompt": "contact alice@example.invalid"
            }))
            .send()
            .await
            .unwrap();
    assert_eq!(openai_response.status(), StatusCode::OK);
    let openai_body: Value = openai_response.json().await.unwrap();
    assert_eq!(
        openai_body["choices"][0]["text"],
        "contact alice@example.invalid"
    );

    let gemini_response = add_legacy_forwarded_headers(client.post(format!(
        "{}/v1beta/models/gemini-test:generateContent",
        proxy.base_url
    )))
    .json(&json!({
        "contents": [{
            "role": "user",
            "parts": [{"text": "contact alice@example.invalid"}]
        }]
    }))
    .send()
    .await
    .unwrap();
    assert_eq!(gemini_response.status(), StatusCode::OK);
    let gemini_body: Value = gemini_response.json().await.unwrap();
    assert_eq!(
        gemini_body["candidates"][0]["content"]["parts"][0]["text"],
        "contact alice@example.invalid"
    );

    let captured = upstream.captured().await;
    assert_eq!(captured.len(), 2);
    for path in [
        "/v1/completions",
        "/v1beta/models/gemini-test:generateContent",
    ] {
        let request = captured
            .iter()
            .find(|request| request.path == path)
            .unwrap();
        assert_legacy_headers(request);
        let serialized = request.body.to_string();
        assert!(!serialized.contains("alice@example.invalid"));
        assert!(serialized.contains("Email_1"));
    }
}

#[tokio::test]
async fn legacy_error_names_statuses_and_no_forwarding_are_stable() {
    let upstream = spawn_upstream().await;
    let proxy = spawn_proxy(adapters_for(&upstream.base_url), 32).await;
    let client = Client::new();

    let adapter_not_found = client
        .post(format!("{}/v1/embeddings", proxy.base_url))
        .json(&json!({"input": "safe"}))
        .send()
        .await
        .unwrap();
    assert_error(adapter_not_found, StatusCode::NOT_FOUND, "AdapterNotFound").await;

    let body_too_large = client
        .post(format!("{}/v1/completions", proxy.base_url))
        .body(json!({"prompt": "x".repeat(64)}).to_string())
        .send()
        .await
        .unwrap();
    assert_error(
        body_too_large,
        StatusCode::PAYLOAD_TOO_LARGE,
        "BodyTooLarge",
    )
    .await;

    let invalid_json = client
        .post(format!("{}/v1/completions", proxy.base_url))
        .header("content-type", "application/json")
        .body("not-json")
        .send()
        .await
        .unwrap();
    assert_error(invalid_json, StatusCode::BAD_REQUEST, "InvalidJson").await;
    assert!(upstream.captured().await.is_empty());

    let unreachable_addr = unused_local_addr();
    let unreachable = Url::parse(&format!("http://{unreachable_addr}")).unwrap();
    let unreachable_proxy =
        spawn_proxy(vec![Arc::new(OpenAiAdapter::new(unreachable))], BODY_LIMIT).await;
    let upstream_unreachable = client
        .post(format!("{}/v1/completions", unreachable_proxy.base_url))
        .json(&json!({"prompt": "safe"}))
        .send()
        .await
        .unwrap();
    assert_error(
        upstream_unreachable,
        StatusCode::BAD_GATEWAY,
        "UpstreamUnreachable",
    )
    .await;
}

#[tokio::test]
async fn health_endpoint_shape_is_stable() {
    let upstream = spawn_upstream().await;
    let proxy = spawn_proxy(adapters_for(&upstream.base_url), BODY_LIMIT).await;
    let health: Value = Client::new()
        .get(format!("{}/_gaze_proxy/healthz", proxy.base_url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let keys = health
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(keys, BTreeSet::from(["adapters", "bind", "uptime_secs"]));
    assert!(health["uptime_secs"].as_u64().is_some());
    assert_eq!(health["bind"], proxy.bind.to_string());
    assert_eq!(
        health["adapters"],
        json!([
            {"name": "openai", "upstream": upstream.base_url.as_str()},
            {"name": "gemini", "upstream": upstream.base_url.as_str()}
        ])
    );
}

#[test]
fn daemon_defaults_keep_public_loopback_and_anthropic_origin() {
    let config = DaemonConfig::default();
    assert_eq!(config.bind, "127.0.0.1:8787".parse().unwrap());
    assert_eq!(
        config.adapters.anthropic.upstream.as_str(),
        "https://api.anthropic.com/"
    );
}
