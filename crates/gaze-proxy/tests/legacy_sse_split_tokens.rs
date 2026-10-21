//! A pseudonym token the model streams across several SSE events must still restore.
//!
//! Real provider streams emit roughly one model token per event, so a pseudonym such as
//! `<hex:Custom:person_1>` almost always arrives in pieces: `<hex:Cus` in one
//! `delta.content`, `tom:person_1>` in the next. Restoring each event on its own never sees a
//! whole token, and the client reassembles the pseudonym instead of the owner's value.
//!
//! Every test drives the real legacy proxy end to end. The request carries the raw values in
//! prose, an adopter rulepack captures them, and the mocked provider streams the tokens it
//! received back in three-byte fragments, one fragment per event. The client reassembles the
//! stream the way an SDK does and must see the raw values; a token the session never minted
//! must stay pseudonymized.

use std::collections::BTreeMap;
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
use gaze_proxy::adapters::{GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use reqwest::Client;
use serde_json::{json, Value};
use url::Url;

const RAW_PERSON: &str = "Robert Q Smith";
/// A double quote and a backslash: inside tool-call `arguments` the restored value must be
/// JSON-escaped, or the agent's tool call stops parsing.
const RAW_ALIAS: &str = r#"Der "Doktor" \ Nord"#;
/// Well-formed but never minted by the session.
const UNKNOWN_TOKEN: &str = "<deadbeef:Custom:person_9>";

const ADOPTER_RULEPACK: &str = r#"
schema_version = "0.1.0"
rulepack_id = "adopter-split-stream"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "adopter.person"
safety_tier = "safe_default"
class = "custom:person"
enabled = true
locales = ["global"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''\bRobert \S+ Smith\b'''

[[recognizers]]
id = "adopter.alias"
safety_tier = "safe_default"
class = "custom:alias"
enabled = true
locales = ["global"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''(?m)^Alias: (.+)$'''
capture_groups = [1]
"#;

fn user_text() -> String {
    format!("Write to {RAW_PERSON}.\nAlias: {RAW_ALIAS}\n")
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
        .with_rulepack_path(rulepack.path().to_path_buf())
        .build()
        .expect("core + adopter rulepack assemble");
    let chain = core.locale_chain().clone();
    (core.into_pipeline(), chain)
}

/// The tokens the proxy substituted into the forwarded request.
struct Tokens {
    person: String,
    alias: String,
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
        assert!(
            !serialized.contains("Robert") && !serialized.contains("Doktor"),
            "raw values reached the provider: {serialized}"
        );
        Self {
            person: pick("person"),
            alias: pick("alias"),
        }
    }
}

/// Three-byte fragments: every token is cut at least twice, mid-token, as a real stream does.
fn fragments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        current.push(character);
        if current.len() >= 3 {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

type Responder = Arc<dyn Fn(&Tokens) -> String + Send + Sync>;

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
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .body(Body::from(responder(&tokens)))
        .unwrap()
}

/// A provider that answers every request with the SSE body `responder` builds.
async fn spawn_upstream(responder: impl Fn(&Tokens) -> String + Send + Sync + 'static) -> Upstream {
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

async fn spawn_proxy(upstream: &Upstream) -> Proxy {
    let (pipeline, chain) = pipeline();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind: SocketAddr = listener.local_addr().unwrap();
    let base = upstream.base_url.clone();
    let config = ProxyConfig::new(
        bind,
        vec![
            Arc::new(OpenAiAdapter::new(base.clone())) as Arc<dyn ProviderAdapter>,
            Arc::new(GeminiAdapter::new(base)) as Arc<dyn ProviderAdapter>,
        ],
    )
    .with_locale_chain(chain);
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

/// Streams `body` through the proxy and returns the JSON payloads the client receives.
async fn stream_through_proxy(
    responder: impl Fn(&Tokens) -> String + Send + Sync + 'static,
    path: &str,
    request: Value,
) -> Vec<Value> {
    let upstream = spawn_upstream(responder).await;
    let proxy = spawn_proxy(&upstream).await;
    let response = Client::new()
        .post(format!("{}{path}", proxy.base_url))
        .json(&request)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(status.is_success(), "{path} returned {status}: {text}");
    sse_payloads(&text)
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

fn sse_body(events: impl IntoIterator<Item = (Option<&'static str>, Value)>) -> String {
    let mut body = String::new();
    for (name, payload) in events {
        if let Some(name) = name {
            body.push_str("event: ");
            body.push_str(name);
            body.push('\n');
        }
        body.push_str("data: ");
        body.push_str(&payload.to_string());
        body.push_str("\n\n");
    }
    body
}

fn chat_sse(chunks: Vec<Value>) -> String {
    let mut body = sse_body(chunks.into_iter().map(|chunk| (None, chunk)));
    body.push_str("data: [DONE]\n\n");
    body
}

fn chat_request() -> Value {
    json!({
        "model": "gpt-test",
        "messages": [{"role": "user", "content": user_text()}],
        "stream": true,
    })
}

fn chunk(choice_index: u64, delta: Value) -> Value {
    json!({
        "id": "chatcmpl-1",
        "object": "chat.completion.chunk",
        "choices": [{"index": choice_index, "delta": delta, "finish_reason": null}],
    })
}

/// Reassembles chat content per choice `index`, as an SDK does.
fn chat_contents(payloads: &[Value]) -> BTreeMap<u64, String> {
    let mut contents = BTreeMap::<u64, String>::new();
    for payload in payloads {
        for choice in payload["choices"].as_array().into_iter().flatten() {
            if let Some(text) = choice["delta"]["content"].as_str() {
                let index = choice["index"].as_u64().expect("choice index");
                contents.entry(index).or_default().push_str(text);
            }
        }
    }
    contents
}

/// Reassembles tool-call arguments per (choice `index`, tool call `index`), as an SDK does.
fn chat_tool_arguments(payloads: &[Value]) -> BTreeMap<(u64, u64), String> {
    let mut arguments = BTreeMap::<(u64, u64), String>::new();
    for payload in payloads {
        for choice in payload["choices"].as_array().into_iter().flatten() {
            let choice_index = choice["index"].as_u64().expect("choice index");
            for call in choice["delta"]["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(fragment) = call["function"]["arguments"].as_str() {
                    let call_index = call["index"].as_u64().expect("tool call index");
                    arguments
                        .entry((choice_index, call_index))
                        .or_default()
                        .push_str(fragment);
                }
            }
        }
    }
    arguments
}

/// The tool-call chunks of one call: a header chunk with the id and name, then one chunk per
/// arguments fragment. Every chunk carries the call at array position 0, as OpenAI does.
fn tool_call_chunks(call_index: u64, name: &str, arguments: &str) -> Vec<Value> {
    let mut chunks = vec![chunk(
        0,
        json!({"tool_calls": [{
            "index": call_index,
            "id": format!("call_{call_index}"),
            "type": "function",
            "function": {"name": name, "arguments": ""},
        }]}),
    )];
    chunks.extend(fragments(arguments).into_iter().map(|fragment| {
        chunk(
            0,
            json!({"tool_calls": [{"index": call_index, "function": {"arguments": fragment}}]}),
        )
    }));
    chunks
}

fn assert_no_token_left(stream: &str, text: &str) {
    assert!(
        token_shape::find_tokens(text).next().is_none(),
        "{stream}: a pseudonym token reached the client unrestored: {text:?}"
    );
}

#[tokio::test]
async fn openai_chat_content_token_split_across_events_restores() {
    let payloads = stream_through_proxy(
        |tokens| {
            let content = format!("To {}.", tokens.person);
            chat_sse(
                fragments(&content)
                    .into_iter()
                    .map(|fragment| chunk(0, json!({"content": fragment})))
                    .collect(),
            )
        },
        "/v1/chat/completions",
        chat_request(),
    )
    .await;
    let contents = chat_contents(&payloads);
    assert_eq!(contents[&0], format!("To {RAW_PERSON}."));
}

#[tokio::test]
async fn openai_chat_tool_arguments_token_split_across_events_restores_json_escaped() {
    let payloads = stream_through_proxy(
        |tokens| {
            let arguments = json!({"to": tokens.person, "alias": tokens.alias}).to_string();
            chat_sse(tool_call_chunks(0, "send_mail", &arguments))
        },
        "/v1/chat/completions",
        chat_request(),
    )
    .await;
    let arguments = chat_tool_arguments(&payloads);
    let restored = &arguments[&(0, 0)];
    assert_no_token_left("chat tool_calls arguments", restored);
    let parsed: Value = serde_json::from_str(restored)
        .unwrap_or_else(|error| panic!("restored arguments do not parse ({error}): {restored:?}"));
    assert_eq!(parsed, json!({"to": RAW_PERSON, "alias": RAW_ALIAS}));
}

#[tokio::test]
async fn openai_chat_parallel_tool_calls_restore_separately() {
    let payloads = stream_through_proxy(
        |tokens| {
            let first = tool_call_chunks(0, "send_mail", &json!({"to": tokens.person}).to_string());
            let second =
                tool_call_chunks(1, "set_alias", &json!({"alias": tokens.alias}).to_string());
            // Interleave the two calls' fragments, as a provider streaming them in parallel can.
            let mut chunks = Vec::new();
            let mut first = first.into_iter();
            let mut second = second.into_iter();
            loop {
                match (first.next(), second.next()) {
                    (None, None) => break,
                    (a, b) => chunks.extend(a.into_iter().chain(b)),
                }
            }
            chat_sse(chunks)
        },
        "/v1/chat/completions",
        chat_request(),
    )
    .await;
    let arguments = chat_tool_arguments(&payloads);
    assert_eq!(arguments.len(), 2, "the two calls merged: {arguments:?}");
    let first: Value = serde_json::from_str(&arguments[&(0, 0)]).expect("call 0 parses");
    let second: Value = serde_json::from_str(&arguments[&(0, 1)]).expect("call 1 parses");
    assert_eq!(first, json!({"to": RAW_PERSON}));
    assert_eq!(second, json!({"alias": RAW_ALIAS}));
}

#[tokio::test]
async fn openai_chat_two_choices_restore_separately() {
    let payloads = stream_through_proxy(
        |tokens| {
            let zero = fragments(&format!("To {}.", tokens.person));
            let one = fragments(&format!("Dear {}!", tokens.person));
            let mut chunks = Vec::new();
            for step in 0..zero.len().max(one.len()) {
                if let Some(fragment) = zero.get(step) {
                    chunks.push(chunk(0, json!({"content": fragment})));
                }
                if let Some(fragment) = one.get(step) {
                    chunks.push(chunk(1, json!({"content": fragment})));
                }
            }
            chat_sse(chunks)
        },
        "/v1/chat/completions",
        chat_request(),
    )
    .await;
    let contents = chat_contents(&payloads);
    assert_eq!(contents[&0], format!("To {RAW_PERSON}."));
    assert_eq!(contents[&1], format!("Dear {RAW_PERSON}!"));
}

#[tokio::test]
async fn split_unknown_token_stays_pseudonymized() {
    let payloads = stream_through_proxy(
        |tokens| {
            let content = format!("To {} and {UNKNOWN_TOKEN}.", tokens.person);
            chat_sse(
                fragments(&content)
                    .into_iter()
                    .map(|fragment| chunk(0, json!({"content": fragment})))
                    .collect(),
            )
        },
        "/v1/chat/completions",
        chat_request(),
    )
    .await;
    let contents = chat_contents(&payloads);
    assert_eq!(
        contents[&0],
        format!("To {RAW_PERSON} and {UNKNOWN_TOKEN}.")
    );
}

#[tokio::test]
async fn gemini_stream_text_token_split_across_events_restores() {
    let payloads = stream_through_proxy(
        |tokens| {
            let text = format!("To {}.", tokens.person);
            let pieces = fragments(&text);
            let last = pieces.len() - 1;
            // Proto3 JSON omits the default candidate index 0 until the final chunk.
            sse_body(pieces.into_iter().enumerate().map(|(step, fragment)| {
                let mut candidate =
                    json!({"content": {"role": "model", "parts": [{"text": fragment}]}});
                if step == last {
                    candidate["index"] = json!(0);
                    candidate["finishReason"] = json!("STOP");
                }
                (None, json!({"candidates": [candidate]}))
            }))
        },
        "/v1beta/models/gemini-test:streamGenerateContent?alt=sse",
        json!({"contents": [{"role": "user", "parts": [{"text": user_text()}]}]}),
    )
    .await;
    let text: String = payloads
        .iter()
        .flat_map(|payload| {
            payload["candidates"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .flat_map(|candidate| {
            candidate["content"]["parts"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|part| part["text"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(text, format!("To {RAW_PERSON}."));
}

#[tokio::test]
async fn openai_responses_stream_deltas_split_across_events_restore() {
    let payloads = stream_through_proxy(
        |tokens| {
            let text = format!("To {}.", tokens.person);
            let arguments = json!({"alias": tokens.alias}).to_string();
            let mut events = Vec::new();
            for fragment in fragments(&text) {
                events.push((
                    Some("response.output_text.delta"),
                    json!({
                        "type": "response.output_text.delta",
                        "item_id": "msg_1",
                        "output_index": 0,
                        "content_index": 0,
                        "delta": fragment,
                    }),
                ));
            }
            events.push((
                Some("response.output_text.done"),
                json!({
                    "type": "response.output_text.done",
                    "item_id": "msg_1",
                    "output_index": 0,
                    "content_index": 0,
                    "text": text,
                }),
            ));
            for fragment in fragments(&arguments) {
                events.push((
                    Some("response.function_call_arguments.delta"),
                    json!({
                        "type": "response.function_call_arguments.delta",
                        "item_id": "fc_1",
                        "output_index": 1,
                        "delta": fragment,
                    }),
                ));
            }
            events.push((
                Some("response.function_call_arguments.done"),
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": "fc_1",
                    "output_index": 1,
                    "arguments": arguments,
                }),
            ));
            events.push((
                Some("response.completed"),
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_1",
                        "object": "response",
                        "status": "completed",
                        "output": [
                            {
                                "type": "message",
                                "id": "msg_1",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": text, "annotations": []}],
                            },
                            {
                                "type": "function_call",
                                "id": "fc_1",
                                "call_id": "call_1",
                                "name": "set_alias",
                                "arguments": arguments,
                            },
                        ],
                    },
                }),
            ));
            sse_body(events)
        },
        "/v1/responses",
        json!({"model": "gpt-test", "input": user_text(), "stream": true}),
    )
    .await;

    let by_type = |kind: &str| -> Vec<&Value> {
        payloads
            .iter()
            .filter(|payload| payload["type"] == kind)
            .collect()
    };
    let text: String = by_type("response.output_text.delta")
        .iter()
        .filter_map(|event| event["delta"].as_str())
        .collect();
    assert_eq!(text, format!("To {RAW_PERSON}."));
    let done = by_type("response.output_text.done");
    assert_eq!(done[0]["text"], json!(format!("To {RAW_PERSON}.")));

    let arguments: String = by_type("response.function_call_arguments.delta")
        .iter()
        .filter_map(|event| event["delta"].as_str())
        .collect();
    let parsed: Value = serde_json::from_str(&arguments)
        .unwrap_or_else(|error| panic!("restored arguments do not parse ({error}): {arguments:?}"));
    assert_eq!(parsed, json!({"alias": RAW_ALIAS}));
    let done = by_type("response.function_call_arguments.done");
    let parsed: Value =
        serde_json::from_str(done[0]["arguments"].as_str().expect("arguments string"))
            .expect("done arguments parse");
    assert_eq!(parsed, json!({"alias": RAW_ALIAS}));

    let completed = by_type("response.completed");
    let output = &completed[0]["response"]["output"];
    assert_eq!(
        output[0]["content"][0]["text"],
        json!(format!("To {RAW_PERSON}."))
    );
    let parsed: Value =
        serde_json::from_str(output[1]["arguments"].as_str().expect("arguments string"))
            .expect("completed arguments parse");
    assert_eq!(parsed, json!({"alias": RAW_ALIAS}));
}
