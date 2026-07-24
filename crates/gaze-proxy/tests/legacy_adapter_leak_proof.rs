//! Executed leak proof for the legacy (non-codec) OpenAI and Gemini adapter path.
//!
//! These tests drive the real public proxy request path (`gaze_proxy::serve`) against a
//! capturing mock upstream and assert on the bytes that would reach the provider. They
//! document CURRENT behavior at `origin/main` d32ae07 for zero-leak scoping (Solo todo
//! #2400 comment 1435 item 2). Every `proof_` test asserts a leak that is present today
//! and MUST start failing once the corresponding fix lands.
//!
//! Two independent leak sub-classes are isolated:
//!
//! * NUMERIC BYPASS — `PiiSurface.text` is `&mut String` (`adapter.rs:432`) and
//!   `walk_all_strings` no-ops on `Value::Number` (`adapter.rs:488`), so a JSON number can
//!   never become a detection surface even inside a fully walked subtree.
//! * FIELD-ALLOWLIST BYPASS — `OpenAiAdapter::request_pii_surfaces` (`adapters/openai.rs:38`)
//!   and `collect_gemini_surfaces` (`adapters/gemini.rs:49`) match a hardcoded set of
//!   top-level keys with a `_ => {}` catch-all, and `server.rs:1812` forwards the whole
//!   original body (`.json(&json)` at `server.rs:1817`) after mutating only those surfaces.
//!
//! Fixtures are synthetic-only per AGENTS.md rule 2.

#![allow(clippy::too_many_lines)]

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use gaze::{Action, ClassRule, DefaultRule, PiiClass, Pipeline};
use gaze_proxy::adapters::{GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use gaze_recognizers::RegexDetector;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

/// Synthetic PII marker strings. Never real PII (AGENTS.md rule 2).
const EMAIL: &str = "alice@example.invalid";
/// Digit-only order id. Detected as `Custom("OrderId")` when it reaches the pipeline as text.
const ORDER_ID_DIGITS: &str = "7001234";
const ORDER_ID_NUMBER: u64 = 7_001_234;

/// Pipeline that CAN detect both markers, so a surviving marker proves the bytes never
/// reached detection rather than proving a detector miss.
fn leak_probe_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .detector(
            RegexDetector::new(r"\b7001234\b", PiiClass::Custom("OrderId".to_string())).unwrap(),
        )
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("OrderId".to_string()),
            Action::Tokenize,
        ))
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
    async fn first_forwarded(&self) -> Value {
        let forwarded = self.forwarded.lock().await.clone();
        assert_eq!(forwarded.len(), 1, "exactly one upstream request expected");
        forwarded[0].clone()
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

async fn spawn_proxy(adapter: Arc<dyn ProviderAdapter>) -> ProxyServer {
    let bind = unused_local_addr();
    let config = ProxyConfig::new(bind, vec![adapter]);
    let pipeline = Arc::new(leak_probe_pipeline());
    let handle = tokio::spawn(async move {
        gaze_proxy::serve(config, pipeline).await.unwrap();
    });
    wait_for_proxy(bind).await;
    ProxyServer {
        base_url: format!("http://{bind}"),
        handle,
    }
}

async fn spawn_openai() -> (MockUpstream, ProxyServer) {
    let upstream = spawn_upstream(|_| json!({"choices": [{"text": "ok"}]})).await;
    let proxy = spawn_proxy(Arc::new(OpenAiAdapter::new(upstream.base_url.clone()))).await;
    (upstream, proxy)
}

async fn spawn_gemini() -> (MockUpstream, ProxyServer) {
    let upstream = spawn_upstream(|_| {
        json!({"candidates": [{"content": {"parts": [{"text": "ok"}], "role": "model"}}]})
    })
    .await;
    let proxy = spawn_proxy(Arc::new(GeminiAdapter::new(upstream.base_url.clone()))).await;
    (upstream, proxy)
}

fn unused_local_addr() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
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
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn post_json(proxy: &ProxyServer, path: &str, body: Value) {
    let response = Client::new()
        .post(format!("{}{path}", proxy.base_url))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "proxy rejected the request: {}",
        response.status()
    );
}

/// Asserts a string field was tokenized: marker gone, a gaze token present.
fn assert_tokenized(value: &Value, marker: &str) {
    let text = value.as_str().expect("string field expected");
    assert!(
        !text.contains(marker),
        "expected {marker} to be tokenized, got {text}"
    );
    assert!(
        text.contains('<') && text.contains('>'),
        "expected a gaze token in {text}"
    );
}

// ---------------------------------------------------------------------------
// OpenAI — controls (covered surfaces behave correctly)
// ---------------------------------------------------------------------------

/// CONTROL: an allowlisted, string-valued surface IS redacted. Calibrates every proof below.
#[tokio::test]
async fn control_openai_allowlisted_string_surface_is_tokenized() {
    let (upstream, proxy) = spawn_openai().await;
    post_json(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": format!("contact {EMAIL} about order {ORDER_ID_DIGITS}")}]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    let content = &forwarded["messages"][0]["content"];
    assert_tokenized(content, EMAIL);
    assert_tokenized(content, ORDER_ID_DIGITS);
}

// ---------------------------------------------------------------------------
// OpenAI — sub-class (i): NUMERIC BYPASS
// ---------------------------------------------------------------------------

/// PROOF (numeric bypass, isolated): `prompt` IS an allowlisted surface walked by
/// `push_text_blocks` (`adapters/openai.rs:44`), yet within the SAME array the string element
/// is tokenized and the numeric element is forwarded raw. `push_text_blocks` falls through
/// `_ => {}` (`adapter.rs:513`) for any non-string, non-text-block item.
///
/// The OpenAI Completions API documents `prompt` as `string | array of strings | array of
/// tokens | array of token arrays`, so a numeric array element is a legal wire shape.
#[tokio::test]
async fn proof_openai_numeric_bypass_inside_allowlisted_prompt_array() {
    let (upstream, proxy) = spawn_openai().await;
    post_json(
        &proxy,
        "/v1/completions",
        json!({
            "model": "gpt-test",
            "prompt": [format!("order {ORDER_ID_DIGITS} for {EMAIL}"), ORDER_ID_NUMBER]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    // Same field, same walk: the string element was detected and tokenized ...
    assert_tokenized(&forwarded["prompt"][0], ORDER_ID_DIGITS);
    // ... and the numeric element reached the provider verbatim.
    assert_eq!(
        forwarded["prompt"][1],
        json!(ORDER_ID_NUMBER),
        "LEAK: numeric array element bypassed detection entirely"
    );
}

/// PROOF (numeric bypass, JSON-Schema literal shape): the OpenAI analogue of the Anthropic
/// `{"description": "order id of the customer", "const": 7001234}` finding. Here the bypass is
/// compound — `tools` is not allowlisted at all — so BOTH the schema description string and the
/// numeric literal egress raw.
#[tokio::test]
async fn proof_openai_tool_schema_numeric_and_string_literals_leak() {
    let (upstream, proxy) = spawn_openai().await;
    post_json(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": "look it up"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "lookup_order",
                    "description": format!("look up the order placed by {EMAIL}"),
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "order_id": {
                                "type": "integer",
                                "description": "order id of the customer",
                                "const": ORDER_ID_NUMBER,
                                "enum": [ORDER_ID_NUMBER]
                            }
                        }
                    }
                }
            }]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    let function = &forwarded["tools"][0]["function"];
    assert_eq!(
        function["description"].as_str().unwrap(),
        format!("look up the order placed by {EMAIL}"),
        "LEAK: tools[].function.description egressed raw (field-allowlist bypass)"
    );
    let order_id = &function["parameters"]["properties"]["order_id"];
    assert_eq!(
        order_id["const"],
        json!(ORDER_ID_NUMBER),
        "LEAK: numeric schema const egressed raw"
    );
    assert_eq!(
        order_id["enum"][0],
        json!(ORDER_ID_NUMBER),
        "LEAK: numeric schema enum member egressed raw"
    );
}

// ---------------------------------------------------------------------------
// OpenAI — sub-class (ii): FIELD-ALLOWLIST BYPASS (real PII strings)
// ---------------------------------------------------------------------------

/// PROOF (allowlist bypass, top level): `user` is OpenAI's documented end-user identifier
/// field. It is a first-class PII carrier in practice and is not in the allowlist
/// (`adapters/openai.rs:41-59`, `_ => {}`), so it egresses raw in the SAME request whose
/// `messages` content was correctly tokenized.
#[tokio::test]
async fn proof_openai_user_identifier_field_leaks_raw() {
    let (upstream, proxy) = spawn_openai().await;
    post_json(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": format!("my address is {EMAIL}")}],
            "user": EMAIL,
            "metadata": {"customer_email": EMAIL},
            "stop": [format!("{EMAIL}:")]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_tokenized(&forwarded["messages"][0]["content"], EMAIL);
    assert_eq!(
        forwarded["user"].as_str().unwrap(),
        EMAIL,
        "LEAK: `user` end-user identifier egressed raw"
    );
    assert_eq!(
        forwarded["metadata"]["customer_email"].as_str().unwrap(),
        EMAIL,
        "LEAK: `metadata` string values egressed raw"
    );
    assert_eq!(
        forwarded["stop"][0].as_str().unwrap(),
        format!("{EMAIL}:"),
        "LEAK: `stop` sequences egressed raw"
    );
}

/// PROOF (allowlist bypass, nested inside an allowlisted parent):
/// `collect_message_surfaces` (`adapters/openai.rs:111`) matches only `content`,
/// `tool_results`, and `tool_calls`. OpenAI's per-message `name` (participant name) and a
/// tool message's `tool_call_id` fall through `_ => {}` and egress raw even though their
/// parent `messages` IS allowlisted.
#[tokio::test]
async fn proof_openai_message_child_keys_leak_raw() {
    let (upstream, proxy) = spawn_openai().await;
    post_json(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [
                {"role": "user", "name": EMAIL, "content": format!("ping {EMAIL}")},
                {"role": "tool", "tool_call_id": format!("call_{ORDER_ID_DIGITS}"), "content": "ok"}
            ]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_tokenized(&forwarded["messages"][0]["content"], EMAIL);
    assert_eq!(
        forwarded["messages"][0]["name"].as_str().unwrap(),
        EMAIL,
        "LEAK: messages[].name egressed raw"
    );
    assert_eq!(
        forwarded["messages"][1]["tool_call_id"].as_str().unwrap(),
        format!("call_{ORDER_ID_DIGITS}"),
        "LEAK: messages[].tool_call_id egressed raw"
    );
}

/// MINOR-3 analogue on the legacy path: numeric sampling controls are forwarded verbatim and
/// are never marked, range-checked, or probed — the legacy path has no coverage machinery at
/// all, so this is the default for the whole body rather than a special case.
#[tokio::test]
async fn proof_openai_numeric_controls_are_forwarded_unprobed() {
    let (upstream, proxy) = spawn_openai().await;
    post_json(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": ORDER_ID_NUMBER,
            "temperature": 0.7,
            "top_p": 0.9
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_eq!(forwarded["max_tokens"], json!(ORDER_ID_NUMBER));
    assert_eq!(forwarded["temperature"], json!(0.7));
    assert_eq!(forwarded["top_p"], json!(0.9));
}

// ---------------------------------------------------------------------------
// Gemini — controls
// ---------------------------------------------------------------------------

const GEMINI_PATH: &str = "/v1beta/models/gemini-test:generateContent";

/// CONTROL: `contents[].parts[].text` and `functionCall.args` string values ARE redacted.
#[tokio::test]
async fn control_gemini_allowlisted_surfaces_are_tokenized() {
    let (upstream, proxy) = spawn_gemini().await;
    post_json(
        &proxy,
        GEMINI_PATH,
        json!({
            "contents": [{
                "role": "user",
                "parts": [
                    {"text": format!("contact {EMAIL}")},
                    {"functionCall": {"name": "lookup", "args": {"email": EMAIL}}}
                ]
            }]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_tokenized(&forwarded["contents"][0]["parts"][0]["text"], EMAIL);
    assert_tokenized(
        &forwarded["contents"][0]["parts"][1]["functionCall"]["args"]["email"],
        EMAIL,
    );
}

// ---------------------------------------------------------------------------
// Gemini — sub-class (i): NUMERIC BYPASS (fully isolated)
// ---------------------------------------------------------------------------

/// PROOF (numeric bypass, fully isolated): `functionCall.args` is walked generically by
/// `walk_all_strings` (`adapters/gemini.rs:108`). Two sibling keys in the SAME walked object:
/// the string is tokenized, the number is forwarded raw because `walk_all_strings` no-ops on
/// `Value::Number` (`adapter.rs:488`) and `PiiSurface.text` is `&mut String`
/// (`adapter.rs:432`). Nothing about the detector changes between the two — only the JSON type.
#[tokio::test]
async fn proof_gemini_numeric_bypass_inside_walked_function_call_args() {
    let (upstream, proxy) = spawn_gemini().await;
    post_json(
        &proxy,
        GEMINI_PATH,
        json!({
            "contents": [{
                "role": "user",
                "parts": [{"functionCall": {"name": "lookup", "args": {
                    "order_id_as_string": ORDER_ID_DIGITS,
                    "order_id_as_number": ORDER_ID_NUMBER
                }}}]
            }]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    let args = &forwarded["contents"][0]["parts"][0]["functionCall"]["args"];
    assert_tokenized(&args["order_id_as_string"], ORDER_ID_DIGITS);
    assert_eq!(
        args["order_id_as_number"],
        json!(ORDER_ID_NUMBER),
        "LEAK: sibling numeric value in the same walked object bypassed detection"
    );
}

// ---------------------------------------------------------------------------
// Gemini — sub-class (ii): FIELD-ALLOWLIST BYPASS
// ---------------------------------------------------------------------------

/// PROOF (allowlist bypass, protobuf-JSON snake_case alias): `collect_gemini_surfaces`
/// (`adapters/gemini.rs:53-70`) matches the literal key `"systemInstruction"` only. The
/// Google Generative Language API is a protobuf-JSON surface, which accepts BOTH the
/// lowerCamelCase name and the original snake_case field name. A client sending
/// `system_instruction` therefore gets zero detection while the camelCase spelling in the
/// SAME request is tokenized.
#[tokio::test]
async fn proof_gemini_snake_case_field_alias_bypasses_allowlist() {
    let (upstream, proxy) = spawn_gemini().await;
    post_json(
        &proxy,
        GEMINI_PATH,
        json!({
            "systemInstruction": {"parts": [{"text": format!("camel {EMAIL}")}]},
            "system_instruction": {"parts": [{"text": format!("snake {EMAIL}")}]},
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_tokenized(&forwarded["systemInstruction"]["parts"][0]["text"], EMAIL);
    assert_eq!(
        forwarded["system_instruction"]["parts"][0]["text"]
            .as_str()
            .unwrap(),
        format!("snake {EMAIL}"),
        "LEAK: snake_case protobuf-JSON alias bypassed the camelCase-only allowlist"
    );
}

/// PROOF (allowlist bypass, request-level fields): `tools[].functionDeclarations`,
/// `generationConfig.stopSequences`, `cachedContent`, and `toolConfig` are all absent from the
/// two-key request allowlist and egress raw.
#[tokio::test]
async fn proof_gemini_request_level_fields_leak_raw() {
    let (upstream, proxy) = spawn_gemini().await;
    post_json(
        &proxy,
        GEMINI_PATH,
        json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "tools": [{"functionDeclarations": [{
                "name": "lookup_order",
                "description": format!("look up the order placed by {EMAIL}"),
                "parameters": {"type": "object", "properties": {
                    "order_id": {"type": "integer", "const": ORDER_ID_NUMBER}
                }}
            }]}],
            "generationConfig": {"stopSequences": [EMAIL], "maxOutputTokens": ORDER_ID_NUMBER},
            "cachedContent": format!("cachedContents/{EMAIL}")
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_eq!(
        forwarded["tools"][0]["functionDeclarations"][0]["description"]
            .as_str()
            .unwrap(),
        format!("look up the order placed by {EMAIL}"),
        "LEAK: tools[].functionDeclarations[].description egressed raw"
    );
    assert_eq!(
        forwarded["tools"][0]["functionDeclarations"][0]["parameters"]["properties"]["order_id"]
            ["const"],
        json!(ORDER_ID_NUMBER),
        "LEAK: numeric schema const egressed raw"
    );
    assert_eq!(
        forwarded["generationConfig"]["stopSequences"][0]
            .as_str()
            .unwrap(),
        EMAIL,
        "LEAK: generationConfig.stopSequences egressed raw"
    );
    assert_eq!(
        forwarded["cachedContent"].as_str().unwrap(),
        format!("cachedContents/{EMAIL}"),
        "LEAK: cachedContent egressed raw"
    );
}

/// PROOF (allowlist bypass, inside an allowlisted parent): `collect_content`
/// (`adapters/gemini.rs:86`) matches only the `text`, `functionCall`, and `functionResponse`
/// part kinds. A `fileData.fileUri` part — a documented Gemini part kind — falls through
/// `_ => {}` and egresses raw even though its `contents[]` parent IS allowlisted.
#[tokio::test]
async fn proof_gemini_non_text_part_kinds_leak_raw() {
    let (upstream, proxy) = spawn_gemini().await;
    post_json(
        &proxy,
        GEMINI_PATH,
        json!({
            "contents": [{
                "role": "user",
                "parts": [
                    {"text": format!("see attachment for {EMAIL}")},
                    {"fileData": {"mimeType": "text/plain",
                                  "fileUri": format!("https://files.example.invalid/{EMAIL}/invoice.pdf")}}
                ]
            }]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_tokenized(&forwarded["contents"][0]["parts"][0]["text"], EMAIL);
    assert_eq!(
        forwarded["contents"][0]["parts"][1]["fileData"]["fileUri"]
            .as_str()
            .unwrap(),
        format!("https://files.example.invalid/{EMAIL}/invoice.pdf"),
        "LEAK: fileData.fileUri part egressed raw"
    );
}
