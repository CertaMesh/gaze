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
async fn openai_responses_structured_input_message_input_text_is_tokenized_and_forwarded() {
    let upstream = spawn_upstream(|_| json!({"output": [{"content": [{"text": "ok"}]}]})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/responses", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "my email is alice@example.invalid"
                }]
            }]
        }))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "structured input should be forwarded, got {}",
        response.status()
    );

    let forwarded = upstream.forwarded().await;
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string();
    assert!(
        !serialized.contains("alice@example.invalid"),
        "PII egressed raw: {serialized}"
    );
    assert!(
        serialized.contains("Email_1"),
        "expected a token: {serialized}"
    );
    assert_eq!(
        forwarded[0]["input"][0]["type"], "message",
        "message item shape preserved"
    );
    assert_eq!(
        forwarded[0]["input"][0]["content"][0]["type"], "input_text",
        "content part shape preserved"
    );
}

#[tokio::test]
async fn openai_responses_structured_input_string_content_is_tokenized_and_forwarded() {
    let upstream = spawn_upstream(|_| json!({"output": [{"content": [{"text": "ok"}]}]})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/responses", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "input": [{
                "type": "message",
                "role": "user",
                "content": "my email is alice@example.invalid"
            }]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    let forwarded = upstream.forwarded().await;
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string();
    assert!(!serialized.contains("alice@example.invalid"));
    assert!(serialized.contains("Email_1"));
    assert_eq!(forwarded[0]["input"][0]["type"], "message");
    assert!(forwarded[0]["input"][0]["content"].is_string());
}

#[tokio::test]
async fn openai_responses_structured_input_multiple_messages_and_parts_all_tokenized() {
    let upstream = spawn_upstream(|_| json!({"output": [{"content": [{"text": "ok"}]}]})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/responses", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "input": [
                {"type": "message", "role": "system", "content": [{"type": "input_text", "text": "reach me at alice@example.invalid"}]},
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "and cc bob@example.invalid too"},
                    {"type": "input_text", "text": "no pii here"}
                ]}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    let forwarded = upstream.forwarded().await;
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string();
    assert!(!serialized.contains("alice@example.invalid"));
    assert!(!serialized.contains("bob@example.invalid"));
    assert!(serialized.contains("Email_1"));
    assert!(serialized.contains("Email_2"));
    assert_eq!(forwarded[0]["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(forwarded[0]["input"][1]["content"][0]["type"], "input_text");
    assert_eq!(
        forwarded[0]["input"][1]["content"][1]["text"],
        "no pii here"
    );
}

#[tokio::test]
async fn openai_chat_completions_text_parts_are_unaffected_by_input_text_broadening() {
    let upstream = spawn_upstream(|_| json!({"choices": [{"message": {"content": "ok"}}]})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/chat/completions", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "messages": [{
                "role": "user",
                "content": [{"type": "text", "text": "my email is alice@example.invalid"}]
            }]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    let forwarded = upstream.forwarded().await;
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string();
    assert!(!serialized.contains("alice@example.invalid"));
    assert!(serialized.contains("Email_1"));
    assert_eq!(forwarded[0]["messages"][0]["content"][0]["type"], "text");
}

/// Resource-identifiers in Responses-API structured `input` stay UNSURFACED by
/// design: a `function_call` item is not a `message`, so its fields are not enumerated
/// by the adapter, and any PII there fails closed rather than being silently rewritten.
/// This is the blast-radius guard — the `push_text_blocks` broadening must only add
/// surfacing for `message` items' `content` text parts, not rewrite non-message kinds.
#[tokio::test]
async fn openai_responses_structured_input_function_call_arguments_still_fail_closed() {
    let upstream = spawn_upstream(|_| json!({"output": [{"content": [{"text": "ok"}]}]})).await;
    let proxy = spawn_proxy(upstream.base_url.clone()).await;

    let response = Client::new()
        .post(format!("{}/v1/responses", proxy.base_url))
        .json(&json!({
            "model": "gpt-test",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "function_call", "call_id": "call_7001234", "arguments": "{\"email\":\"alice@example.invalid\"}"}
            ]
        }))
        .send()
        .await
        .unwrap();

    // PII inside an unsurfaced non-message item must fail closed, never forwarded.
    assert_eq!(
        response.status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        "function_call.arguments carrying PII must fail closed, got {}",
        response.status()
    );
    assert_eq!(
        response.text().await.unwrap(),
        r#"{"error":"UnsurfacedPii"}"#
    );
    assert!(upstream.forwarded().await.is_empty());
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
