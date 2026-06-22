use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use gaze::{
    token_shape, Action, ClassRule, CleanDocument, DefaultRule, PiiClass, Pipeline, RawDocument,
    Scope, Session,
};
use gaze_proxy::adapters::OpenAiAdapter;
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use gaze_recognizers::RegexDetector;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

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
    forwarded: Arc<Mutex<Vec<Value>>>,
    response: Arc<dyn Fn(Value) -> Value + Send + Sync>,
}

struct MockUpstream {
    base_url: Url,
    forwarded: Arc<Mutex<Vec<Value>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl MockUpstream {
    async fn forwarded(&self) -> Vec<Value> {
        self.forwarded.lock().await.clone()
    }
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

async fn spawn_upstream(response: impl Fn(Value) -> Value + Send + Sync + 'static) -> MockUpstream {
    let forwarded = Arc::new(Mutex::new(Vec::new()));
    let state = UpstreamState {
        forwarded: forwarded.clone(),
        response: Arc::new(response),
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
        forwarded,
        handle,
    }
}

async fn capture_upstream(
    State(state): State<UpstreamState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    state.forwarded.lock().await.push(body.clone());
    Json((state.response)(body))
}

async fn spawn_proxy(upstream: Url) -> ProxyServer {
    let bind = unused_local_addr();
    let config = ProxyConfig::new(
        bind,
        vec![Arc::new(OpenAiAdapter::new(upstream)) as Arc<dyn ProviderAdapter>],
    );
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

#[test]
fn openai_request_surfaces_redact_content_and_tool_arguments() {
    let adapter = OpenAiAdapter::new(Url::parse("https://api.openai.com").unwrap());
    let pipeline = email_pipeline();
    let session = Session::new(Scope::Conversation("openai".to_string())).unwrap();
    let mut body = json!({
        "messages": [{
            "role": "user",
            "content": "email alice@example.invalid",
            "tool_calls": [{
                "function": {
                    "name": "lookup",
                    "arguments": "{\"email\":\"alice@example.invalid\"}"
                }
            }]
        }]
    });

    for surface in adapter.request_pii_surfaces(&mut body) {
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(surface.text.clone()))
            .unwrap()
        else {
            panic!("text clean document expected");
        };
        *surface.text = clean;
    }

    let serialized = body.to_string();
    assert!(!serialized.contains("alice@example.invalid"));
    assert!(serialized.contains("Email_1"));
    let token = token_shape::find_token(&serialized).expect("token emitted");
    assert_eq!(
        session.restore(token).as_deref(),
        Some("alice@example.invalid")
    );
}

#[tokio::test]
async fn openai_completions_prompt_string_redacts_before_forwarding() {
    let upstream = spawn_upstream(|_| json!({"choices": [{"text": "ok"}]})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/completions", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "prompt": "contact ada@example.invalid"
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    let forwarded = upstream.forwarded().await;
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string();
    assert!(!serialized.contains("ada@example.invalid"));
    assert!(serialized.contains("Email_1"));
}

#[tokio::test]
async fn openai_completions_prompt_array_redacts_before_forwarding() {
    let upstream = spawn_upstream(|_| json!({"choices": [{"text": "ok"}]})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/completions", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "prompt": ["contact ada@example.invalid", "no pii here"]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    let forwarded = upstream.forwarded().await;
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string();
    assert!(!serialized.contains("ada@example.invalid"));
    assert!(serialized.contains("Email_1"));
}

#[tokio::test]
async fn openai_completions_response_text_restores_tokens() {
    let upstream = spawn_upstream(|body| {
        json!({
            "choices": [{
                "text": body["system"].as_str().unwrap()
            }]
        })
    })
    .await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/completions", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "system": "reply to ada@example.invalid",
            "prompt": "safe prompt"
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    let forwarded = upstream.forwarded().await;
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string();
    assert!(!serialized.contains("ada@example.invalid"));
    assert!(serialized.contains("Email_1"));

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["choices"][0]["text"], "reply to ada@example.invalid");
}

#[tokio::test]
async fn openai_responses_instructions_redacts_before_forwarding() {
    let upstream = spawn_upstream(|_| json!({"output": [{"content": [{"text": "ok"}]}]})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/responses", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "instructions": "contact ada@example.invalid",
            "input": "safe input"
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    let forwarded = upstream.forwarded().await;
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string();
    assert!(!serialized.contains("ada@example.invalid"));
    assert!(serialized.contains("Email_1"));
}

#[tokio::test]
async fn unmatched_path_returns_404_without_forwarding_upstream() {
    let upstream = spawn_upstream(|_| json!({"unexpected": true})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/embeddings", proxy.base_url))
        .json(&json!({
            "input": "contact ada@example.invalid"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    assert!(upstream.forwarded().await.is_empty());
}
