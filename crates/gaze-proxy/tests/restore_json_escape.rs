//! Restored values must land in the syntax of the field that carries them.
//!
//! A token the model echoes into tool-call `arguments` or into JSON-mode content sits inside a
//! JSON string literal of a serialized JSON document. Restoring the raw value there verbatim
//! breaks that document the moment the value holds a `"`, a `\`, or a control character: the
//! agent either fails to parse its own tool call or, worse, parses a silently different value.
//! Plain-text content has no such syntax and must keep restoring byte for byte.
//!
//! Every test drives the real proxy end to end. The request carries the raw values in prose, the
//! shipped `password.field` recognizer (the opt-in `secrets` rulepack) and an adopter rulepack
//! recognizer capture them, and the mocked provider echoes the tokens it received back into one
//! response destination per test.

use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use gaze::{token_shape, LocaleChain, Pipeline};
use gaze_assembly::CorePipelineConfig;
use gaze_proxy::adapters::{AnthropicAdapter, GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use reqwest::Client;
use serde_json::{json, Value};
use url::Url;

/// A double quote, a backslash, a TAB, and a non-ASCII letter. `password.field` captures the
/// single-quoted spelling verbatim, so this is exactly the manifest value.
const RAW_PASSWORD: &str = "Pa\"ss\\\\w\u{f6}rd\t1";
/// A double quote and a newline, captured by an adopter recognizer spanning two lines.
const RAW_DELIVERY: &str = "Hinterhaus \"Nord\"\nAufgang C";
/// A UNC path. Pasted verbatim into a JSON string it still PARSES, but `\\` decodes to one
/// backslash and `\n` to a newline, so the agent would silently act on a different path.
const RAW_SHARE: &str = r"\\fileserver\new_hires";

const ADOPTER_RULEPACK: &str = r#"
schema_version = "0.1.0"
rulepack_id = "adopter-delivery"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "adopter.delivery_block"
safety_tier = "safe_default"
class = "custom:delivery_block"
enabled = true
locales = ["global"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''(?m)^Deliver to: (.+\n.+)$'''
capture_groups = [1]

[[recognizers]]
id = "adopter.share_path"
safety_tier = "safe_default"
class = "custom:share_path"
enabled = true
locales = ["global"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''(?m)^Share: (\S+)$'''
capture_groups = [1]

[[recognizers]]
id = "adopter.unc_secret"
safety_tier = "safe_default"
class = "custom:unc_secret"
enabled = true
locales = ["global"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''Secret: (\\\\[a-z]+\\[a-z]+)'''
capture_groups = [1]

[[recognizers]]
id = "adopter.customer_name"
safety_tier = "safe_default"
class = "custom:customer_name"
enabled = true
locales = ["global"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''\bRobert \S+ Smith\b'''
"#;

fn user_text() -> String {
    format!("password: '{RAW_PASSWORD}'\nDeliver to: {RAW_DELIVERY}\nShare: {RAW_SHARE}\n")
}

fn pipeline() -> (Pipeline, LocaleChain) {
    let mut rulepack = tempfile::Builder::new()
        .suffix(".toml")
        .tempfile()
        .expect("adopter rulepack file");
    rulepack
        .write_all(ADOPTER_RULEPACK.as_bytes())
        .expect("write adopter rulepack");
    let core = CorePipelineConfig::new()
        .with_bundled_rulepack("secrets")
        .with_rulepack_path(rulepack.path().to_path_buf())
        .build()
        .expect("core + secrets + adopter rulepack assemble");
    let chain = core.locale_chain().clone();
    (core.into_pipeline(), chain)
}

/// The tokens the proxy substituted into the forwarded request.
struct Tokens {
    password: String,
    delivery: String,
    share: String,
}

impl Tokens {
    fn from_forwarded(request: &Value) -> Self {
        let serialized = request.to_string();
        let found: Vec<&str> = token_shape::find_tokens(&serialized).collect();
        let pick = |class: &str| {
            found
                .iter()
                .find(|token| token.contains(class))
                .unwrap_or_else(|| panic!("no {class} token forwarded in {serialized}"))
                .to_string()
        };
        let tokens = Self {
            password: pick("password"),
            delivery: pick("delivery_block"),
            share: pick("share_path"),
        };
        assert!(
            !serialized.contains("Hinterhaus")
                && !serialized.contains("w\u{f6}rd")
                && !serialized.contains("fileserver"),
            "raw values reached the provider: {serialized}"
        );
        tokens
    }

    /// A JSON document echoing both tokens as string values.
    fn json_document(&self) -> String {
        json!({"password": self.password, "delivery": self.delivery}).to_string()
    }

    /// A JSON document whose only value is the UNC path.
    fn share_document(&self) -> String {
        json!({"share": self.share}).to_string()
    }

    fn plain_text(&self) -> String {
        format!(
            "Saved {} for {} on {}.",
            self.password, self.delivery, self.share
        )
    }
}

fn expected_plain_text() -> String {
    format!("Saved {RAW_PASSWORD} for {RAW_DELIVERY} on {RAW_SHARE}.")
}

/// The restored JSON document must parse, and its values must be the raw values byte for byte.
fn assert_exact_json(destination: &str, restored: &str) {
    let parsed: Value = serde_json::from_str(restored).unwrap_or_else(|error| {
        panic!("{destination}: restored JSON does not parse ({error}): {restored:?}")
    });
    assert_eq!(
        parsed["password"].as_str(),
        Some(RAW_PASSWORD),
        "{destination}: password restored to a different value: {restored:?}"
    );
    assert_eq!(
        parsed["delivery"].as_str(),
        Some(RAW_DELIVERY),
        "{destination}: delivery block restored to a different value: {restored:?}"
    );
}

fn assert_exact_text(destination: &str, restored: &str) {
    assert_eq!(
        restored,
        expected_plain_text(),
        "{destination}: plain text did not restore byte-exact"
    );
}

type Responder = Arc<dyn Fn(&Value, &Tokens) -> (&'static str, String) + Send + Sync>;

struct Upstream {
    base_url: Url,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn provider(
    State(responder): State<Responder>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    let tokens = Tokens::from_forwarded(&request);
    let (content_type, body) = responder(&request, &tokens);
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .body(Body::from(body))
        .unwrap()
}

async fn spawn_upstream(
    responder: impl Fn(&Value, &Tokens) -> (&'static str, String) + Send + Sync + 'static,
) -> Upstream {
    let app = Router::new()
        .route("/{*path}", post(provider))
        .with_state(Arc::new(responder) as Responder);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Upstream {
        base_url: Url::parse(&format!("http://{addr}")).unwrap(),
        handle,
    }
}

struct Proxy {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn spawn_proxy(config: impl FnOnce(SocketAddr) -> ProxyConfig) -> Proxy {
    let (pipeline, chain) = pipeline();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = listener.local_addr().unwrap();
    let config = config(bind).with_locale_chain(chain);
    let handle = tokio::spawn(async move {
        gaze_proxy::serve_with_listener(config, Arc::new(pipeline), listener)
            .await
            .unwrap();
    });
    let client = Client::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !client
        .get(format!("http://{bind}/_gaze_proxy/healthz"))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "proxy did not start at {bind}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Proxy {
        base_url: format!("http://{bind}"),
        handle,
    }
}

async fn spawn_legacy(upstream: &Upstream) -> Proxy {
    let base = upstream.base_url.clone();
    spawn_proxy(move |bind| {
        ProxyConfig::new(
            bind,
            vec![
                Arc::new(OpenAiAdapter::new(base.clone())) as Arc<dyn ProviderAdapter>,
                Arc::new(GeminiAdapter::new(base)) as Arc<dyn ProviderAdapter>,
            ],
        )
    })
    .await
}

async fn post_legacy(proxy: &Proxy, path: &str, body: Value) -> String {
    let response = Client::new()
        .post(format!("{}{path}", proxy.base_url))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(status.is_success(), "{path} returned {status}: {text}");
    text
}

/// The JSON payloads of an SSE body, in order, without the `[DONE]` sentinel.
fn sse_payloads(body: &str) -> Vec<Value> {
    body.split("\n\n")
        .filter_map(|frame| {
            let data = frame
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            (!data.is_empty() && data != "[DONE]")
                .then(|| serde_json::from_str(&data).expect("SSE payload is JSON"))
        })
        .collect()
}

fn sse_body(payloads: &[Value]) -> String {
    let mut body = String::new();
    for payload in payloads {
        body.push_str("data: ");
        body.push_str(&payload.to_string());
        body.push_str("\n\n");
    }
    body
}

// ---------------------------------------------------------------------------------------------
// OpenAI Chat Completions (legacy surface adapter)
// ---------------------------------------------------------------------------------------------

fn chat_request(json_mode: bool, stream: bool) -> Value {
    let mut request = json!({
        "model": "gpt-test",
        "messages": [{"role": "user", "content": user_text()}],
        "stream": stream,
    });
    if json_mode {
        request["response_format"] = json!({"type": "json_object"});
    }
    request
}

fn chat_completion(message: Value) -> String {
    json!({
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
    })
    .to_string()
}

fn chat_chunk(delta: Value) -> Value {
    json!({
        "id": "chatcmpl-1",
        "object": "chat.completion.chunk",
        "choices": [{"index": 0, "delta": delta, "finish_reason": null}],
    })
}

/// Splits `document` so every token sits whole in its own fragment while the JSON string
/// syntax around it (the opening and closing quotes) arrives in neighbouring fragments.
fn fragments_around_tokens(document: &str, tokens: &Tokens) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut rest = document;
    while !rest.is_empty() {
        let next = [&tokens.password, &tokens.delivery, &tokens.share]
            .into_iter()
            .filter_map(|token| rest.find(token.as_str()).map(|at| (at, token.len())))
            .min();
        match next {
            Some((at, len)) => {
                if at > 0 {
                    fragments.push(rest[..at].to_string());
                }
                fragments.push(rest[at..at + len].to_string());
                rest = &rest[at + len..];
            }
            None => {
                fragments.push(rest.to_string());
                rest = "";
            }
        }
    }
    fragments
}

#[tokio::test]
async fn openai_chat_tool_call_arguments_restore_to_the_exact_values() {
    let upstream = spawn_upstream(|_, tokens| {
        let body = chat_completion(json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "save_contact", "arguments": tokens.json_document()}
            }]
        }));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/chat/completions", chat_request(false, false)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let arguments = response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .expect("arguments stay a string");
    assert_exact_json("chat tool_calls[].function.arguments", arguments);
}

#[tokio::test]
async fn openai_chat_tool_call_arguments_do_not_silently_rewrite_a_backslash_path() {
    let upstream = spawn_upstream(|_, tokens| {
        let body = chat_completion(json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "open_share", "arguments": tokens.share_document()}
            }]
        }));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/chat/completions", chat_request(false, false)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let arguments = response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .expect("arguments stay a string");
    let parsed: Value = serde_json::from_str(arguments)
        .unwrap_or_else(|error| panic!("restored arguments do not parse ({error}): {arguments:?}"));
    assert_eq!(
        parsed["share"].as_str(),
        Some(RAW_SHARE),
        "the agent would act on a different path: {arguments:?}"
    );
}

/// A tool result is usually JSON text, so a value captured there is stored in its JSON-escaped
/// spelling (`\"`, and `\u00fc` from a Python `json.dumps` with the default `ensure_ascii`).
/// Echoed into tool-call arguments it is already valid string content; escaping it again
/// would hand the agent a backslash that was never in the customer record.
#[tokio::test]
async fn openai_chat_value_from_a_json_tool_result_restores_into_arguments_unchanged() {
    const TOOL_RESULT: &str =
        r#"{"customer": "Robert \"Bob\" Smith", "contact": "Robert M\u00fcller Smith"}"#;
    let upstream = spawn_upstream(|request, _| {
        let serialized = request.to_string();
        let names: Vec<&str> = token_shape::find_tokens(&serialized)
            .filter(|token| token.contains("customer_name"))
            .collect();
        assert_eq!(
            names.len(),
            2,
            "two customer names tokenized in {serialized}"
        );
        let arguments = json!({"customer": names[0], "contact": names[1]}).to_string();
        let body = chat_completion(json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_2",
                "type": "function",
                "function": {"name": "send_letter", "arguments": arguments}
            }]
        }));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let request = json!({
        "model": "gpt-test",
        "messages": [
            {"role": "user", "content": user_text()},
            {"role": "assistant", "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "lookup_customer", "arguments": "{\"id\":7}"}
            }]},
            {"role": "tool", "tool_call_id": "call_1", "content": TOOL_RESULT}
        ]
    });
    let body = post_legacy(&proxy, "/v1/chat/completions", request).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let arguments = response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .expect("arguments stay a string");
    let parsed: Value = serde_json::from_str(arguments)
        .unwrap_or_else(|error| panic!("restored arguments do not parse ({error}): {arguments:?}"));
    assert_eq!(parsed["customer"], "Robert \"Bob\" Smith", "{arguments:?}");
    assert_eq!(
        parsed["contact"], "Robert M\u{fc}ller Smith",
        "{arguments:?}"
    );
}

#[tokio::test]
async fn openai_chat_json_mode_content_restores_to_the_exact_values() {
    let upstream = spawn_upstream(|request, tokens| {
        assert_eq!(request["response_format"]["type"], "json_object");
        let body = chat_completion(json!({"role": "assistant", "content": tokens.json_document()}));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/chat/completions", chat_request(true, false)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let content = response["choices"][0]["message"]["content"]
        .as_str()
        .expect("content is a string");
    assert_exact_json("chat JSON-mode message.content", content);
}

#[tokio::test]
async fn openai_chat_plain_text_content_restores_byte_exact() {
    let upstream = spawn_upstream(|_, tokens| {
        let body = chat_completion(json!({"role": "assistant", "content": tokens.plain_text()}));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/chat/completions", chat_request(false, false)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let content = response["choices"][0]["message"]["content"]
        .as_str()
        .expect("content is a string");
    assert_exact_text("chat plain message.content", content);
}

#[tokio::test]
async fn openai_chat_stream_tool_call_arguments_restore_to_the_exact_values() {
    let upstream = spawn_upstream(|_, tokens| {
        let mut payloads = vec![chat_chunk(json!({
            "role": "assistant",
            "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": {"name": "save_contact", "arguments": ""}
            }]
        }))];
        for fragment in fragments_around_tokens(&tokens.json_document(), tokens) {
            payloads.push(chat_chunk(json!({
                "tool_calls": [{"index": 0, "function": {"arguments": fragment}}]
            })));
        }
        (
            "text/event-stream",
            sse_body(&payloads) + "data: [DONE]\n\n",
        )
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/chat/completions", chat_request(false, true)).await;
    let arguments: String = sse_payloads(&body)
        .iter()
        .filter_map(|payload| {
            payload["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str()
        })
        .collect();
    assert_exact_json(
        "chat stream delta.tool_calls[].function.arguments",
        &arguments,
    );
}

#[tokio::test]
async fn openai_chat_stream_json_mode_content_restores_to_the_exact_values() {
    let upstream = spawn_upstream(|request, tokens| {
        assert_eq!(request["response_format"]["type"], "json_object");
        let payloads: Vec<Value> = fragments_around_tokens(&tokens.json_document(), tokens)
            .into_iter()
            .map(|fragment| chat_chunk(json!({"content": fragment})))
            .collect();
        (
            "text/event-stream",
            sse_body(&payloads) + "data: [DONE]\n\n",
        )
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/chat/completions", chat_request(true, true)).await;
    let content: String = sse_payloads(&body)
        .iter()
        .filter_map(|payload| payload["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_exact_json("chat stream JSON-mode delta.content", &content);
}

#[tokio::test]
async fn openai_chat_stream_plain_text_content_restores_byte_exact() {
    let upstream = spawn_upstream(|_, tokens| {
        let payloads: Vec<Value> = fragments_around_tokens(&tokens.plain_text(), tokens)
            .into_iter()
            .map(|fragment| chat_chunk(json!({"content": fragment})))
            .collect();
        (
            "text/event-stream",
            sse_body(&payloads) + "data: [DONE]\n\n",
        )
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/chat/completions", chat_request(false, true)).await;
    let content: String = sse_payloads(&body)
        .iter()
        .filter_map(|payload| payload["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_exact_text("chat stream plain delta.content", &content);
}

// ---------------------------------------------------------------------------------------------
// OpenAI Responses API (legacy surface adapter)
// ---------------------------------------------------------------------------------------------

fn responses_request(json_mode: bool) -> Value {
    let mut request = json!({"model": "gpt-test", "input": user_text()});
    if json_mode {
        request["text"] = json!({"format": {"type": "json_object"}});
    }
    request
}

fn responses_output(output: Value) -> String {
    json!({"id": "resp_1", "object": "response", "status": "completed", "output": output})
        .to_string()
}

fn output_message(text: String) -> Value {
    json!({
        "type": "message",
        "id": "msg_1",
        "role": "assistant",
        "status": "completed",
        "content": [{"type": "output_text", "text": text, "annotations": []}]
    })
}

#[tokio::test]
async fn openai_responses_function_and_mcp_call_arguments_restore_to_the_exact_values() {
    let upstream = spawn_upstream(|_, tokens| {
        let body = responses_output(json!([
            {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "save_contact",
                "arguments": tokens.json_document(),
                "status": "completed"
            },
            {
                "type": "mcp_call",
                "id": "mcp_1",
                "server_label": "contacts",
                "name": "save_contact",
                "arguments": tokens.json_document()
            }
        ]));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/responses", responses_request(false)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    for (index, destination) in [
        "responses function_call.arguments",
        "responses mcp_call.arguments",
    ]
    .into_iter()
    .enumerate()
    {
        let arguments = response["output"][index]["arguments"]
            .as_str()
            .expect("arguments stay a string");
        assert_exact_json(destination, arguments);
    }
}

#[tokio::test]
async fn openai_responses_json_mode_output_text_restores_to_the_exact_values() {
    let upstream = spawn_upstream(|request, tokens| {
        assert_eq!(request["text"]["format"]["type"], "json_object");
        let body = responses_output(json!([output_message(tokens.json_document())]));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/responses", responses_request(true)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let text = response["output"][0]["content"][0]["text"]
        .as_str()
        .expect("output_text is a string");
    assert_exact_json("responses JSON-mode output_text", text);
}

#[tokio::test]
async fn openai_responses_plain_output_text_restores_byte_exact() {
    let upstream = spawn_upstream(|_, tokens| {
        let body = responses_output(json!([output_message(tokens.plain_text())]));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, "/v1/responses", responses_request(false)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let text = response["output"][0]["content"][0]["text"]
        .as_str()
        .expect("output_text is a string");
    assert_exact_text("responses plain output_text", text);
}

// ---------------------------------------------------------------------------------------------
// Gemini generateContent (legacy surface adapter)
// ---------------------------------------------------------------------------------------------

const GEMINI_GENERATE: &str = "/v1beta/models/gemini-test:generateContent";
const GEMINI_STREAM: &str = "/v1beta/models/gemini-test:streamGenerateContent?alt=sse";

fn gemini_request(json_mode: bool) -> Value {
    let mut request = json!({
        "contents": [{"role": "user", "parts": [{"text": user_text()}]}]
    });
    if json_mode {
        request["generationConfig"] = json!({"responseMimeType": "application/json"});
    }
    request
}

fn gemini_candidate(parts: Value) -> Value {
    json!({
        "candidates": [{
            "index": 0,
            "content": {"role": "model", "parts": parts}
        }]
    })
}

#[tokio::test]
async fn gemini_function_call_args_restore_to_the_exact_values() {
    let upstream = spawn_upstream(|_, tokens| {
        let body = gemini_candidate(json!([{
            "functionCall": {
                "name": "save_contact",
                "args": {"password": tokens.password, "delivery": tokens.delivery}
            }
        }]));
        ("application/json", body.to_string())
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, GEMINI_GENERATE, gemini_request(false)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let args = &response["candidates"][0]["content"]["parts"][0]["functionCall"]["args"];
    assert_exact_json("gemini functionCall.args", &args.to_string());
}

#[tokio::test]
async fn gemini_json_mode_text_restores_to_the_exact_values() {
    let upstream = spawn_upstream(|request, tokens| {
        assert_eq!(
            request["generationConfig"]["responseMimeType"],
            "application/json"
        );
        let body = gemini_candidate(json!([
            {"thought": true, "text": tokens.plain_text()},
            {"text": tokens.json_document()}
        ]));
        ("application/json", body.to_string())
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, GEMINI_GENERATE, gemini_request(true)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let parts = &response["candidates"][0]["content"]["parts"];
    // A thought summary is prose even when the answer is JSON.
    assert_exact_text(
        "gemini JSON-mode thought summary",
        parts[0]["text"].as_str().expect("thought part is a string"),
    );
    assert_exact_json(
        "gemini JSON-mode parts[].text",
        parts[1]["text"].as_str().expect("text part is a string"),
    );
}

#[tokio::test]
async fn gemini_stream_json_mode_text_restores_to_the_exact_values() {
    let upstream = spawn_upstream(|_, tokens| {
        let payloads: Vec<Value> = fragments_around_tokens(&tokens.json_document(), tokens)
            .into_iter()
            .map(|fragment| gemini_candidate(json!([{"text": fragment}])))
            .collect();
        ("text/event-stream", sse_body(&payloads))
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    // The snake_case spelling of the same protobuf fields selects JSON output too.
    let request = json!({
        "contents": [{"role": "user", "parts": [{"text": user_text()}]}],
        "generation_config": {"response_mime_type": "application/json"}
    });
    let body = post_legacy(&proxy, GEMINI_STREAM, request).await;
    let text: String = sse_payloads(&body)
        .iter()
        .filter_map(|payload| payload["candidates"][0]["content"]["parts"][0]["text"].as_str())
        .collect();
    assert_exact_json("gemini stream JSON-mode parts[].text", &text);
}

#[tokio::test]
async fn gemini_plain_text_restores_byte_exact() {
    let upstream = spawn_upstream(|_, tokens| {
        let body = gemini_candidate(json!([{"text": tokens.plain_text()}]));
        ("application/json", body.to_string())
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;

    let body = post_legacy(&proxy, GEMINI_GENERATE, gemini_request(false)).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let text = response["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .expect("text part is a string");
    assert_exact_text("gemini plain parts[].text", text);
}

// ---------------------------------------------------------------------------------------------
// Anthropic Messages (codec-proved direct adapter)
// ---------------------------------------------------------------------------------------------

async fn spawn_anthropic(upstream: &Upstream) -> Proxy {
    let origin = upstream.base_url.clone();
    spawn_proxy(move |bind| ProxyConfig::anthropic_direct(bind, AnthropicAdapter::new(origin)))
        .await
}

async fn post_anthropic(proxy: &Proxy, stream: bool) -> String {
    let response = Client::new()
        .post(format!("{}/v1/messages", proxy.base_url))
        .header("x-api-key", "synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "claude-test",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": user_text()}],
            "stream": stream
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(
        status.is_success(),
        "/v1/messages returned {status}: {text}"
    );
    text
}

fn anthropic_frame(payload: &Value) -> String {
    format!(
        "event: {}\ndata: {payload}\n\n",
        payload["type"].as_str().expect("frame type")
    )
}

/// Cuts `document` in the middle of every token, so each token spans two SSE frames.
fn fragments_splitting_tokens(document: &str, tokens: &Tokens) -> Vec<String> {
    let mut cuts = Vec::new();
    for token in [&tokens.password, &tokens.delivery, &tokens.share] {
        let Some(start) = document.find(token.as_str()) else {
            continue;
        };
        let mut middle = start + token.len() / 2;
        while !document.is_char_boundary(middle) {
            middle += 1;
        }
        cuts.push(middle);
    }
    cuts.sort_unstable();
    let mut fragments = Vec::new();
    let mut last = 0;
    for cut in cuts {
        fragments.push(document[last..cut].to_string());
        last = cut;
    }
    fragments.push(document[last..].to_string());
    fragments
}

#[tokio::test]
async fn anthropic_tool_use_input_and_text_restore_to_the_exact_values() {
    let upstream = spawn_upstream(|_, tokens| {
        let body = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude-test",
            "content": [
                {"type": "text", "text": tokens.plain_text()},
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "save_contact",
                    "input": {"password": tokens.password, "delivery": tokens.delivery}
                }
            ],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        });
        ("application/json", body.to_string())
    })
    .await;
    let proxy = spawn_anthropic(&upstream).await;

    let body = post_anthropic(&proxy, false).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    assert_exact_text(
        "anthropic text block",
        response["content"][0]["text"].as_str().expect("text block"),
    );
    assert_exact_json(
        "anthropic tool_use.input",
        &response["content"][1]["input"].to_string(),
    );
}

#[tokio::test]
async fn anthropic_stream_input_json_delta_and_text_delta_restore_tokens_split_across_frames() {
    let upstream = spawn_upstream(|_, tokens| {
        let mut frames = vec![
            json!({"type": "message_start", "message": {
                "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-test",
                "content": [], "stop_reason": null, "stop_sequence": null,
                "usage": {"input_tokens": 1, "output_tokens": 0}
            }}),
            json!({"type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}}),
        ];
        for fragment in fragments_splitting_tokens(&tokens.plain_text(), tokens) {
            frames.push(json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": fragment}}));
        }
        frames.push(json!({"type": "content_block_stop", "index": 0}));
        frames.push(
            json!({"type": "content_block_start", "index": 1, "content_block": {
                "type": "tool_use", "id": "toolu_1", "name": "save_contact", "input": {}
            }}),
        );
        for fragment in fragments_splitting_tokens(&tokens.json_document(), tokens) {
            frames.push(json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": fragment}}));
        }
        frames.push(json!({"type": "content_block_stop", "index": 1}));
        frames.push(json!({"type": "message_delta",
            "delta": {"stop_reason": "tool_use", "stop_sequence": null},
            "usage": {"output_tokens": 1}}));
        frames.push(json!({"type": "message_stop"}));
        let body: String = frames.iter().map(anthropic_frame).collect();
        ("text/event-stream", body)
    })
    .await;
    let proxy = spawn_anthropic(&upstream).await;

    let body = post_anthropic(&proxy, true).await;
    let payloads = sse_payloads(&body);
    let text: String = payloads
        .iter()
        .filter(|payload| payload["index"] == 0)
        .filter_map(|payload| payload["delta"]["text"].as_str())
        .collect();
    let partial_json: String = payloads
        .iter()
        .filter(|payload| payload["index"] == 1)
        .filter_map(|payload| payload["delta"]["partial_json"].as_str())
        .collect();
    assert_exact_text("anthropic stream text_delta", &text);
    assert_exact_json("anthropic stream input_json_delta", &partial_json);
}

// ---------------------------------------------------------------------------------------------
// Values captured in their JSON-escaped source spelling
// ---------------------------------------------------------------------------------------------

/// A JSON document as a client sends it in a text block or a tool result: one value with escaped
/// quotes, one with the `\u00fc` escape Python's `json.dumps(ensure_ascii=True)` writes. The
/// `customer_name` recognizer captures both values in this escaped source spelling.
const JSON_CUSTOMER_DOCUMENT: &str =
    r#"{"customer": "Robert \"Bob\" Smith", "contact": "Robert M\u00fcller Smith"}"#;
/// The values the document above means once a JSON parser reads it.
const SEMANTIC_CUSTOMER: &str = "Robert \"Bob\" Smith";
const SEMANTIC_CONTACT: &str = "Robert M\u{fc}ller Smith";
/// The same values in the exact bytes the client sent.
const SOURCE_CUSTOMER: &str = r#"Robert \"Bob\" Smith"#;
const SOURCE_CONTACT: &str = r"Robert M\u00fcller Smith";

/// A UNC path written in plain text. The same bytes inside a JSON string literal would decode
/// `\\` to one backslash and `\n` to a newline.
const PLAIN_UNC_SECRET: &str = r"\\srv\new";

/// The forwarded `customer_name` tokens, in request order.
fn customer_tokens(request: &Value) -> Vec<String> {
    let serialized = request.to_string();
    let names: Vec<String> = token_shape::find_tokens(&serialized)
        .filter(|token| token.contains("customer_name"))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        names.len(),
        2,
        "two customer names tokenized in {serialized}"
    );
    assert!(
        !serialized.contains("Bob") && !serialized.contains("ller Smith"),
        "raw customer names reached the provider: {serialized}"
    );
    names
}

fn unc_secret_token(request: &Value) -> String {
    let serialized = request.to_string();
    let found: Vec<&str> = token_shape::find_tokens(&serialized)
        .filter(|token| token.contains("unc_secret"))
        .collect();
    assert!(
        !found.is_empty() && found.iter().all(|token| *token == found[0]),
        "one unc_secret token, minted once and reused, expected in {serialized}"
    );
    assert!(
        !serialized.contains("srv"),
        "raw secret reached the provider: {serialized}"
    );
    found[0].to_owned()
}

async fn post_anthropic_messages(proxy: &Proxy, messages: Value, stream: bool) -> String {
    let response = Client::new()
        .post(format!("{}/v1/messages", proxy.base_url))
        .header("x-api-key", "synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "claude-test",
            "max_tokens": 64,
            "messages": messages,
            "stream": stream
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(
        status.is_success(),
        "/v1/messages returned {status}: {text}"
    );
    text
}

/// A user turn whose second text block is a JSON document.
fn json_text_block_messages() -> Value {
    json!([{
        "role": "user",
        "content": [
            {"type": "text", "text": user_text()},
            {"type": "text", "text": JSON_CUSTOMER_DOCUMENT}
        ]
    }])
}

fn anthropic_message(content: Value) -> String {
    json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-test",
        "content": content,
        "stop_reason": "tool_use",
        "stop_sequence": null,
        "usage": {"input_tokens": 1, "output_tokens": 1}
    })
    .to_string()
}

/// `tool_use.input` is a decoded JSON value: the agent reads `customer` as the string the client's
/// JSON document meant, not as its escaped spelling. A text block has no JSON syntax to decode,
/// so it restores the exact bytes the client sent.
#[tokio::test]
async fn anthropic_tool_use_input_restores_the_semantic_value_of_a_json_spelled_capture() {
    let upstream = spawn_upstream(|request, _| {
        let names = customer_tokens(request);
        let body = anthropic_message(json!([
            {"type": "text", "text": format!("{} / {}", names[0], names[1])},
            {
                "type": "tool_use",
                "id": "toolu_01Lookup",
                "name": "send_letter",
                "input": {"customer": names[0], "contact": names[1]}
            }
        ]));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_anthropic(&upstream).await;

    let body = post_anthropic_messages(&proxy, json_text_block_messages(), false).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let input = &response["content"][1]["input"];
    assert_eq!(input["customer"], SEMANTIC_CUSTOMER, "{input}");
    assert_eq!(input["contact"], SEMANTIC_CONTACT, "{input}");
    assert_eq!(
        response["content"][0]["text"],
        format!("{SOURCE_CUSTOMER} / {SOURCE_CONTACT}"),
        "a plain-text destination restores the source bytes"
    );
}

#[tokio::test]
async fn anthropic_stream_input_json_delta_restores_the_semantic_value_of_a_json_spelled_capture() {
    let upstream = spawn_upstream(|request, _| {
        let names = customer_tokens(request);
        let document = json!({"customer": names[0], "contact": names[1]}).to_string();
        let middle = document.find(names[0].as_str()).expect("token in document") + 3;
        let mut frames = vec![
            json!({"type": "message_start", "message": {
                "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-test",
                "content": [], "stop_reason": null, "stop_sequence": null,
                "usage": {"input_tokens": 1, "output_tokens": 0}
            }}),
            json!({"type": "content_block_start", "index": 0, "content_block": {
                "type": "tool_use", "id": "toolu_01Lookup", "name": "send_letter", "input": {}
            }}),
        ];
        for fragment in [&document[..middle], &document[middle..]] {
            frames.push(json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": fragment}}));
        }
        frames.push(json!({"type": "content_block_stop", "index": 0}));
        frames.push(json!({"type": "message_delta",
            "delta": {"stop_reason": "tool_use", "stop_sequence": null},
            "usage": {"output_tokens": 1}}));
        frames.push(json!({"type": "message_stop"}));
        let body: String = frames.iter().map(anthropic_frame).collect();
        ("text/event-stream", body)
    })
    .await;
    let proxy = spawn_anthropic(&upstream).await;

    let body = post_anthropic_messages(&proxy, json_text_block_messages(), true).await;
    let partial_json: String = sse_payloads(&body)
        .iter()
        .filter_map(|payload| payload["delta"]["partial_json"].as_str())
        .collect();
    let input: Value = serde_json::from_str(&partial_json)
        .unwrap_or_else(|error| panic!("restored input does not parse ({error}): {partial_json}"));
    assert_eq!(input["customer"], SEMANTIC_CUSTOMER, "{partial_json}");
    assert_eq!(input["contact"], SEMANTIC_CONTACT, "{partial_json}");
}

/// A JSON tool result carried as a text string is the common case: the agent's own tool returned
/// the record, and the model passes a field of it on to the next tool call.
#[tokio::test]
async fn anthropic_value_from_a_json_tool_result_string_restores_into_tool_use_input_semantically()
{
    let upstream = spawn_upstream(|request, _| {
        let names = customer_tokens(request);
        let body = anthropic_message(json!([{
            "type": "tool_use",
            "id": "toolu_02Answer",
            "name": "send_letter",
            "input": {"customer": names[0], "contact": names[1]}
        }]));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_anthropic(&upstream).await;

    let messages = json!([
        {"role": "user", "content": user_text()},
        {"role": "assistant", "content": [{
            "type": "tool_use", "id": "toolu_01Lookup", "name": "lookup_customer", "input": {"id": "7"}
        }]},
        {"role": "user", "content": [{
            "type": "tool_result", "tool_use_id": "toolu_01Lookup", "content": JSON_CUSTOMER_DOCUMENT
        }]}
    ]);
    let body = post_anthropic_messages(&proxy, messages, false).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let input = &response["content"][0]["input"];
    assert_eq!(input["customer"], SEMANTIC_CUSTOMER, "{input}");
    assert_eq!(input["contact"], SEMANTIC_CONTACT, "{input}");
}

/// The same bytes first appear in plain text, then inside a JSON tool result. The token was
/// minted with the plain-text reading, and it keeps it: `\n` stays a backslash and an `n`.
#[tokio::test]
async fn anthropic_token_keeps_the_plain_reading_it_was_first_minted_with() {
    let upstream = spawn_upstream(|request, _| {
        let secret = unc_secret_token(request);
        let body = anthropic_message(json!([{
            "type": "tool_use",
            "id": "toolu_02Answer",
            "name": "open_share",
            "input": {"path": secret}
        }]));
        ("application/json", body)
    })
    .await;
    let proxy = spawn_anthropic(&upstream).await;

    let messages = json!([
        {"role": "user", "content": format!("{}Secret: {PLAIN_UNC_SECRET}\n", user_text())},
        {"role": "assistant", "content": [{
            "type": "tool_use", "id": "toolu_01Lookup", "name": "read_note", "input": {"id": "7"}
        }]},
        {"role": "user", "content": [{
            "type": "tool_result",
            "tool_use_id": "toolu_01Lookup",
            "content": format!("{{\"note\": \"Secret: {PLAIN_UNC_SECRET}\"}}")
        }]}
    ]);
    let body = post_anthropic_messages(&proxy, messages, false).await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    assert_eq!(
        response["content"][0]["input"]["path"], PLAIN_UNC_SECRET,
        "the token flipped to a JSON reading"
    );
}

async fn post_legacy_in_session(proxy: &Proxy, session: &str, body: Value) -> String {
    let response = Client::new()
        .post(format!("{}/v1/chat/completions", proxy.base_url))
        .header("x-gaze-session-id", session)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(status.is_success(), "returned {status}: {text}");
    text
}

/// Turn 1 mints the token from plain text. Turn 2 of the same conversation carries the same bytes
/// inside a JSON tool result. Restoring into tool-call arguments must still yield the plain path.
#[tokio::test]
async fn openai_chat_token_keeps_the_plain_reading_across_turns() {
    let upstream = spawn_upstream(|request, _| {
        let tool_turn = request["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| message["role"] == "tool"));
        let content = if tool_turn {
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_2",
                    "type": "function",
                    "function": {
                        "name": "open_share",
                        "arguments": json!({"path": unc_secret_token(request)}).to_string()
                    }
                }]
            })
        } else {
            json!({"role": "assistant", "content": "noted"})
        };
        ("application/json", chat_completion(content))
    })
    .await;
    let proxy = spawn_legacy(&upstream).await;
    let session = "json-spelling-collision";

    post_legacy_in_session(
        &proxy,
        session,
        json!({"model": "gpt-test", "messages": [{
            "role": "user",
            "content": format!("{}Secret: {PLAIN_UNC_SECRET}\n", user_text())
        }]}),
    )
    .await;
    let body = post_legacy_in_session(
        &proxy,
        session,
        json!({
            "model": "gpt-test",
            "messages": [
                {"role": "user", "content": user_text()},
                {"role": "assistant", "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "read_note", "arguments": "{\"id\":7}"}
                }]},
                {"role": "tool", "tool_call_id": "call_1",
                    "content": format!("{{\"note\": \"Secret: {PLAIN_UNC_SECRET}\"}}")}
            ]
        }),
    )
    .await;
    let response: Value = serde_json::from_str(&body).expect("proxy response is JSON");
    let arguments = response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .expect("arguments stay a string");
    let parsed: Value = serde_json::from_str(arguments)
        .unwrap_or_else(|error| panic!("restored arguments do not parse ({error}): {arguments:?}"));
    assert_eq!(
        parsed["path"], PLAIN_UNC_SECRET,
        "the token flipped to a JSON reading: {arguments:?}"
    );
}
