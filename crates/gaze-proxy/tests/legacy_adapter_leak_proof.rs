//! Leak contract for the legacy (non-codec) OpenAI and Gemini adapter path.
//!
//! These tests drive the real public proxy request path (`gaze_proxy::serve`) against a
//! capturing mock upstream and assert on the bytes that would reach the provider.
//!
//! They began as an executed leak PROOF against `origin/main` d32ae07 (Solo todo #2400
//! comment 1435 item 2), where every `proof_` test asserted raw PII surviving to the wire.
//! Each one now asserts the fixed behavior — fail closed, or tokenized — so the file doubles
//! as the regression contract for the fix.
//!
//! Two independent leak sub-classes were confirmed and are covered here:
//!
//! * NUMERIC BYPASS — `PiiSurface.text` is `&mut String` (`adapter.rs:432`) and
//!   `walk_all_strings` no-ops on `Value::Number` (`adapter.rs:488`), so a JSON number can
//!   never become a detection surface even inside a fully walked subtree.
//! * FIELD-ALLOWLIST BYPASS — `OpenAiAdapter::request_pii_surfaces` and
//!   `collect_gemini_surfaces` match hardcoded key sets with a `_ => {}` catch-all, and
//!   `server.rs` forwards the whole original body after mutating only those surfaces.
//!
//! Both are closed by `residual_scan_request` (F1), which fails closed on any PII outside an
//! adapter surface.
//!
//! ## Locale note
//!
//! `Pipeline::redact` pins `[LocaleTag::Global]` (`gaze/src/pipeline.rs:350`), and the residual
//! re-scan uses the same chain deliberately, so the residual can be neither weaker nor stronger
//! than the primary pass. Consequence: locale-gated recognizers (`postal.us`, `postal.de`,
//! national phone) never activate on proxied traffic at all. The en-US / de-DE corpora below
//! therefore use locale-agnostic five-digit stand-in recognizers, which is what makes the
//! carrier / non-carrier boundary observable on this path.
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
use gaze_proxy::{ProviderAdapter, ProxyConfig, ProxyError};
use gaze_recognizers::RegexDetector;
use reqwest::{Client, Response, StatusCode};
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

/// Locale-agnostic five-digit stand-in for a national postal recognizer.
///
/// The real `postal.us` / `postal.de` recognizers are locale-gated and therefore never active
/// on the proxy path (see the module-level locale note). This stand-in reproduces their
/// detection shape under `LocaleTag::Global` so the numeric carrier / non-carrier boundary is
/// observable. `class_label` distinguishes the en-US and de-DE corpus runs.
fn postal_probe_pipeline(class_label: &str) -> Pipeline {
    Pipeline::builder()
        .detector(
            RegexDetector::new(r"\b\d{5}\b", PiiClass::Custom(class_label.to_string())).unwrap(),
        )
        .rule(ClassRule::new(
            PiiClass::Custom(class_label.to_string()),
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

    /// Asserts the proxy failed closed: nothing at all reached the provider.
    async fn assert_nothing_forwarded(&self) {
        let forwarded = self.forwarded.lock().await.clone();
        assert!(
            forwarded.is_empty(),
            "fail-closed expected, but {} request(s) reached the provider",
            forwarded.len()
        );
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

async fn spawn_proxy(adapter: Arc<dyn ProviderAdapter>, pipeline: Pipeline) -> ProxyServer {
    let bind = unused_local_addr();
    let config = ProxyConfig::new(bind, vec![adapter]);
    let pipeline = Arc::new(pipeline);
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
    spawn_openai_with(leak_probe_pipeline()).await
}

async fn spawn_openai_with(pipeline: Pipeline) -> (MockUpstream, ProxyServer) {
    let upstream = spawn_upstream(|_| json!({"choices": [{"text": "ok"}]})).await;
    let proxy = spawn_proxy(
        Arc::new(OpenAiAdapter::new(upstream.base_url.clone())),
        pipeline,
    )
    .await;
    (upstream, proxy)
}

async fn spawn_gemini() -> (MockUpstream, ProxyServer) {
    spawn_gemini_with(leak_probe_pipeline()).await
}

async fn spawn_gemini_with(pipeline: Pipeline) -> (MockUpstream, ProxyServer) {
    let upstream = spawn_upstream(
        |_| json!({"candidates": [{"content": {"parts": [{"text": "ok"}], "role": "model"}}]}),
    )
    .await;
    let proxy = spawn_proxy(
        Arc::new(GeminiAdapter::new(upstream.base_url.clone())),
        pipeline,
    )
    .await;
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

async fn post_json(proxy: &ProxyServer, path: &str, body: Value) -> Response {
    Client::new()
        .post(format!("{}{path}", proxy.base_url))
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// Asserts the proxy accepted and forwarded the request.
async fn post_accepted(proxy: &ProxyServer, path: &str, body: Value) {
    let response = post_json(proxy, path, body).await;
    assert!(
        response.status().is_success(),
        "expected the request to be forwarded, got {}",
        response.status()
    );
}

/// Asserts the proxy failed closed with `UnsurfacedPii`, and that the client-facing body
/// discloses only the error name — never the field path and never the detected value.
async fn post_rejected(proxy: &ProxyServer, upstream: &MockUpstream, path: &str, body: Value) {
    let response = post_json(proxy, path, body).await;
    assert_eq!(
        response.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "expected a fail-closed rejection"
    );
    let rendered = response.text().await.unwrap();
    assert_eq!(rendered, json!({"error": "UnsurfacedPii"}).to_string());
    assert!(!rendered.contains(EMAIL), "client body leaked the fixture");
    assert!(
        !rendered.contains(ORDER_ID_DIGITS),
        "client body leaked the fixture"
    );
    upstream.assert_nothing_forwarded().await;
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
// Controls — covered surfaces still behave correctly (must stay green, unchanged)
// ---------------------------------------------------------------------------

/// CONTROL: an allowlisted, string-valued surface IS redacted. Calibrates every proof below.
#[tokio::test]
async fn control_openai_allowlisted_string_surface_is_tokenized() {
    let (upstream, proxy) = spawn_openai().await;
    post_accepted(
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

/// CONTROL: `contents[].parts[].text` and `functionCall.args` string values ARE redacted.
#[tokio::test]
async fn control_gemini_allowlisted_surfaces_are_tokenized() {
    let (upstream, proxy) = spawn_gemini().await;
    post_accepted(
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
// Sub-class (i): NUMERIC BYPASS — now fails closed
// ---------------------------------------------------------------------------

/// Was: the numeric element of an allowlisted `prompt` array egressed raw while the string
/// element beside it was tokenized. `push_text_blocks` falls through `_ => {}` for any
/// non-string item, and a number can never become a `PiiSurface`.
#[tokio::test]
async fn proof_openai_numeric_bypass_inside_allowlisted_prompt_array() {
    let (upstream, proxy) = spawn_openai().await;
    post_rejected(
        &proxy,
        &upstream,
        "/v1/completions",
        json!({
            "model": "gpt-test",
            "prompt": [format!("order {ORDER_ID_DIGITS} for {EMAIL}"), ORDER_ID_NUMBER]
        }),
    )
    .await;
}

/// Was: the OpenAI analogue of the Anthropic `{"description": ..., "const": 7001234}` finding —
/// both the schema description string and the numeric literal egressed raw.
#[tokio::test]
async fn proof_openai_tool_schema_numeric_and_string_literals_leak() {
    let (upstream, proxy) = spawn_openai().await;
    post_rejected(
        &proxy,
        &upstream,
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
}

/// Was: a sibling numeric value inside a fully walked `functionCall.args` object egressed raw
/// while the string beside it was tokenized. The cleanest isolation of the numeric sub-class:
/// nothing differs between the two keys but the JSON type.
#[tokio::test]
async fn proof_gemini_numeric_bypass_inside_walked_function_call_args() {
    let (upstream, proxy) = spawn_gemini().await;
    post_rejected(
        &proxy,
        &upstream,
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
}

// ---------------------------------------------------------------------------
// Sub-class (ii): FIELD-ALLOWLIST BYPASS — now fails closed
// ---------------------------------------------------------------------------

/// Was: `user` (OpenAI's documented end-user identifier), `metadata`, and `stop` all egressed
/// raw in the same request whose `messages` content was correctly tokenized.
///
/// All three are free-text carriers the model or the provider reads back verbatim, so they are
/// now surfaced and pseudonymized rather than merely rejected.
#[tokio::test]
async fn proof_openai_user_identifier_field_leaks_raw() {
    let (upstream, proxy) = spawn_openai().await;
    post_accepted(
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
    assert_tokenized(&forwarded["user"], EMAIL);
    assert_tokenized(&forwarded["metadata"]["customer_email"], EMAIL);
    assert_tokenized(&forwarded["stop"][0], EMAIL);
}

/// Was: `messages[].name` and `messages[].tool_call_id` egressed raw despite `messages` being
/// allowlisted — the nested-inside-a-covered-parent shape, which is the easiest to miss.
///
/// `name` is a participant name read by the model, so it is now surfaced and pseudonymized.
#[tokio::test]
async fn proof_openai_message_child_keys_leak_raw() {
    let (upstream, proxy) = spawn_openai().await;
    post_accepted(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "name": EMAIL, "content": format!("ping {EMAIL}")}]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_tokenized(&forwarded["messages"][0]["name"], EMAIL);
    assert_tokenized(&forwarded["messages"][0]["content"], EMAIL);
}

/// A resource identifier stays UNSURFACED on purpose: `tool_call_id` is correlated provider-side,
/// so substituting a token would silently break tool calling. PII there fails closed instead,
/// which tells the adopter their identifier carries PII rather than handing them a broken
/// request that looks fine.
#[tokio::test]
async fn openai_resource_identifiers_fail_closed_rather_than_being_rewritten() {
    let (upstream, proxy) = spawn_openai().await;
    post_rejected(
        &proxy,
        &upstream,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [
                {"role": "tool", "tool_call_id": format!("call-{ORDER_ID_DIGITS}"), "content": "ok"}
            ]
        }),
    )
    .await;
}

const GEMINI_PATH: &str = "/v1beta/models/gemini-test:generateContent";

/// Was: the Generative Language API is a protobuf-JSON surface accepting both
/// `systemInstruction` and `system_instruction`, but the adapter matched the camelCase literal
/// only — so the snake_case spelling of a COVERED field got zero detection.
///
/// Now both spellings are recognized as the same field and BOTH are tokenized, rather than the
/// request merely failing closed. `snake_case_parts_are_also_recognized` covers the nested
/// spelling; this test pins the top-level one.
#[tokio::test]
async fn proof_gemini_snake_case_field_alias_bypasses_allowlist() {
    let (upstream, proxy) = spawn_gemini().await;
    post_accepted(
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
    assert_tokenized(&forwarded["system_instruction"]["parts"][0]["text"], EMAIL);
}

/// The snake_case aliases nested below the top level are recognized too: `function_call` and
/// `function_response` are the protobuf spellings of part kinds the adapter walks, and
/// `system_instruction.parts` may itself be spelled either way.
#[tokio::test]
async fn snake_case_parts_are_also_recognized() {
    let (upstream, proxy) = spawn_gemini().await;
    post_accepted(
        &proxy,
        GEMINI_PATH,
        json!({
            "contents": [{
                "role": "user",
                "parts": [
                    {"function_call": {"name": "lookup", "args": {"email": EMAIL}}},
                    {"function_response": {"name": "lookup", "response": {"email": EMAIL}}}
                ]
            }]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    let parts = &forwarded["contents"][0]["parts"];
    assert_tokenized(&parts[0]["function_call"]["args"]["email"], EMAIL);
    assert_tokenized(&parts[1]["function_response"]["response"]["email"], EMAIL);
}

/// Was: `tools[].functionDeclarations[].description`, its numeric schema `const`,
/// `generationConfig.stopSequences`, and `cachedContent` all egressed raw.
#[tokio::test]
async fn proof_gemini_request_level_fields_leak_raw() {
    let (upstream, proxy) = spawn_gemini().await;
    post_rejected(
        &proxy,
        &upstream,
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
}

/// Was: a `fileData.fileUri` part egressed raw despite its `contents[]` parent being
/// allowlisted — `collect_content` matches only three part kinds.
#[tokio::test]
async fn proof_gemini_non_text_part_kinds_leak_raw() {
    let (upstream, proxy) = spawn_gemini().await;
    post_rejected(
        &proxy,
        &upstream,
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
}

// ---------------------------------------------------------------------------
// Numeric carrier scope — the D2 boundary
// ---------------------------------------------------------------------------

/// Sampling and generation controls are declared non-carriers and are forwarded unprobed.
///
/// `max_tokens` here carries the same digits as the `OrderId` fixture: if the non-carrier
/// declaration were removed, this request would be rejected. That makes this test part of the
/// mutation probe for `NON_CARRIER_NUMERIC_KEYS`.
#[tokio::test]
async fn proof_openai_numeric_controls_are_forwarded_unprobed() {
    let (upstream, proxy) = spawn_openai().await;
    post_accepted(
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

/// FALSE-REJECT CORPUS (en-US): ordinary sampling-control values in the 4- and 5-digit range
/// must NOT be rejected, even with a five-digit postal-shaped recognizer active.
///
/// This is the precision half of the D2 boundary. Delete `NON_CARRIER_NUMERIC_KEYS` and this
/// goes red.
#[tokio::test]
async fn ordinary_sampling_controls_are_not_rejected_en_us() {
    let (upstream, proxy) = spawn_openai_with(postal_probe_pipeline("PostalUs")).await;
    post_accepted(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 20000,
            "max_completion_tokens": 16384,
            "seed": 12345,
            "n": 1,
            "top_p": 0.9,
            "temperature": 0.7,
            "presence_penalty": 0,
            "frequency_penalty": 0
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_eq!(forwarded["max_tokens"], json!(20000));
    assert_eq!(forwarded["seed"], json!(12345));
}

/// FALSE-REJECT CORPUS (de-DE): the Gemini spellings of the same controls, under a de-DE
/// five-digit postal-shaped recognizer.
#[tokio::test]
async fn ordinary_sampling_controls_are_not_rejected_de_de() {
    let (upstream, proxy) = spawn_gemini_with(postal_probe_pipeline("PostalDe")).await;
    post_accepted(
        &proxy,
        GEMINI_PATH,
        json!({
            "contents": [{"role": "user", "parts": [{"text": "hallo"}]}],
            "generationConfig": {
                "maxOutputTokens": 99999,
                "candidateCount": 1,
                "topP": 0.9,
                "topK": 40,
                "seed": 54321
            }
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    assert_eq!(
        forwarded["generationConfig"]["maxOutputTokens"],
        json!(99999)
    );
    assert_eq!(forwarded["generationConfig"]["seed"], json!(54321));
}

/// BOUNDS ARE CARRIERS (en-US): a five-digit schema `maximum` DOES fail closed.
///
/// This pins an intentional decision so a later reader cannot mistake it for an oversight.
/// Numeric schema bounds stay carriers to match the Anthropic codec's ruling on numeric schema
/// literals; a proxy that answered differently for the same input shape would be an axis-4
/// determinism regression. The values-vs-bounds question is recorded as an open user decision
/// to be resolved once, across both paths.
#[tokio::test]
async fn schema_numeric_bounds_fail_closed_en_us() {
    let (upstream, proxy) = spawn_openai_with(postal_probe_pipeline("PostalUs")).await;
    post_rejected(
        &proxy,
        &upstream,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {
                "name": "lookup", "description": "lookup",
                "parameters": {"type": "object", "properties": {
                    "code": {"type": "integer", "minimum": 10000, "maximum": 99999}
                }}
            }}]
        }),
    )
    .await;
}

/// BOUNDS ARE CARRIERS (de-DE): the same decision on the Gemini declaration shape.
#[tokio::test]
async fn schema_numeric_bounds_fail_closed_de_de() {
    let (upstream, proxy) = spawn_gemini_with(postal_probe_pipeline("PostalDe")).await;
    post_rejected(
        &proxy,
        &upstream,
        GEMINI_PATH,
        json!({
            "contents": [{"role": "user", "parts": [{"text": "hallo"}]}],
            "tools": [{"functionDeclarations": [{
                "name": "lookup", "description": "lookup",
                "parameters": {"type": "object", "properties": {
                    "code": {"type": "integer", "maximum": 99999}
                }}
            }]}]
        }),
    )
    .await;
}

/// A tool schema may legitimately declare a property NAMED like a sampling control. A numeric
/// literal there is tool-declared data, not a decoding knob, so `CARRIER_SUBTREE_KEYS` keeps it
/// probed. Without that guard the non-carrier list would punch holes in exactly the schema
/// positions the Anthropic `const` finding proved exploitable.
#[tokio::test]
async fn control_named_property_inside_a_schema_stays_a_carrier() {
    let (upstream, proxy) = spawn_openai_with(postal_probe_pipeline("PostalUs")).await;
    post_rejected(
        &proxy,
        &upstream,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {
                "name": "weather", "description": "weather",
                "parameters": {"type": "object", "properties": {
                    "seed": {"type": "integer", "const": 12345}
                }}
            }}]
        }),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Error disclosure contract
// ---------------------------------------------------------------------------

/// The typed error carries the field path ONLY — never the detected value.
///
/// `Display` and `Debug` both feed logs, so both are checked. The client-facing body is
/// covered separately by every `post_rejected` call, which asserts it discloses only the
/// error name.
#[test]
fn unsurfaced_pii_error_discloses_the_field_path_but_never_the_value() {
    let error = ProxyError::UnsurfacedPii {
        field_path: "tools[0].function.description".to_string(),
    };

    let rendered = error.to_string();
    assert!(
        rendered.contains("tools[0].function.description"),
        "the field path is the actionable part of the error: {rendered}"
    );
    assert!(!rendered.contains(EMAIL), "Display leaked the value");
    assert!(!rendered.contains(ORDER_ID_DIGITS), "Display leaked digits");

    let debugged = format!("{error:?}");
    assert!(!debugged.contains(EMAIL), "Debug leaked the value");
    assert!(!debugged.contains(ORDER_ID_DIGITS), "Debug leaked digits");
}
