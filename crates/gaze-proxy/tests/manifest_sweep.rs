//! Repeat-value sweep through the proxy: a name found by a
//! rule in one message must not ship raw in another message of the same
//! request. Synthetic names only.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use gaze::{
    Action, Candidate, ConflictTier, DefaultRule, DetectContext, PiiClass, Pipeline, Recognizer,
};
use gaze_proxy::adapters::{AnthropicAdapter, GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

/// Finds `From: <name> <` like the bundled `email.header.name` rule.
struct Header(Option<gaze::EvidenceKind>);

impl Recognizer for Header {
    fn id(&self) -> &str {
        "header"
    }
    fn evidence(&self) -> gaze::EvidenceKind {
        self.0.unwrap_or(gaze::EvidenceKind::Learned)
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze_types::DetectError> {
        if self.0.is_none() {
            return Ok(vec![]);
        }
        let pattern = regex::Regex::new(r"(?m)^From: ([^<\n]+?) <").unwrap();
        Ok(pattern
            .captures_iter(input)
            .map(|caps| {
                Candidate::new(
                    caps.get(1).unwrap().range(),
                    PiiClass::Name,
                    "header",
                    0.9,
                    0,
                    None,
                    "counter",
                    "header",
                    ConflictTier::None,
                    vec![],
                )
            })
            .collect())
    }
}

struct HeaderNet;

impl gaze::SafetyNet for HeaderNet {
    fn id(&self) -> &str {
        "synthetic-header-net"
    }
    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: gaze::SafetyNetContext<'_>,
    ) -> Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
        let pattern = regex::Regex::new(r"(?m)^From: ([^<\n]+?) <").unwrap();
        Ok(pattern
            .captures_iter(text)
            .map(|caps| {
                gaze::LeakSuspect::new(
                    caps.get(1).unwrap().range(),
                    PiiClass::Name,
                    self.id(),
                    Some(0.95),
                    gaze::LeakKind::Uncovered,
                    "synthetic header",
                    None,
                )
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug)]
enum Provider {
    OpenAi,
    Gemini,
    Anthropic,
}

impl Provider {
    fn text(self, body: &Value, index: usize) -> &str {
        match self {
            Self::OpenAi | Self::Anthropic => body["messages"][index]["content"].as_str().unwrap(),
            Self::Gemini => body["contents"][index]["parts"][0]["text"]
                .as_str()
                .unwrap(),
        }
    }

    fn reply(self, text: &str) -> Value {
        match self {
            Self::OpenAi => {
                json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
            }
            Self::Gemini => json!({"candidates": [{"content": {"parts": [{"text": text}]}}]}),
            Self::Anthropic => json!({
                "id": "msg_synthetic", "type": "message", "role": "assistant", "model": "claude-test",
                "content": [{"type": "text", "text": text}], "stop_reason": "end_turn", "stop_sequence": null,
                "usage": {"input_tokens": 1, "output_tokens": 1}
            }),
        }
    }

    fn answer(self, body: &Value) -> Option<&str> {
        match self {
            Self::OpenAi => body["choices"][0]["message"]["content"]
                .as_str()
                .or_else(|| body["choices"][0]["delta"]["content"].as_str()),
            Self::Gemini => body["candidates"][0]["content"]["parts"][0]["text"].as_str(),
            Self::Anthropic => body["content"][0]["text"]
                .as_str()
                .or_else(|| body["delta"]["text"].as_str()),
        }
    }
}

#[derive(Clone)]
struct Capture {
    forwarded: Arc<Mutex<Vec<Value>>>,
    provider: Provider,
    stream: bool,
}

async fn capture(State(state): State<Capture>, Json(body): Json<Value>) -> Response {
    let text = state.provider.text(&body, 0).to_owned();
    state.forwarded.lock().await.push(body);
    if !state.stream {
        return Json(state.provider.reply(&text)).into_response();
    }
    let wire = match state.provider {
        Provider::OpenAi => format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"index":0,"delta":{"content":text}}]})
        ),
        Provider::Gemini => format!("data: {}\n\n", state.provider.reply(&text)),
        Provider::Anthropic => {
            let mut message = state.provider.reply("");
            message["content"] = json!([]);
            message["stop_reason"] = Value::Null;
            let events = [
                json!({"type":"message_start","message":message}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}),
                json!({"type":"message_stop"}),
            ];
            events
                .iter()
                .map(|event| {
                    format!(
                        "event: {}\ndata: {event}\n\n",
                        event["type"].as_str().unwrap()
                    )
                })
                .collect()
        }
    };
    ([("content-type", "text/event-stream")], wire).into_response()
}

const SOURCE: &str = "From: Maria Schneider <m@example.invalid>\nHello.";
const COPY: &str = "hi, this is maria schneider again. Thanks, Maria";
// Word edges, public URL paths and lowercase lone parts are existing sweep exclusions.
const PUBLIC: &str =
    "Mariasch and xMaria and Schneiderei. https://example.invalid/Maria/Schneider and thanks maria";

async fn run_case(
    provider: Provider,
    stream: bool,
    source_first: bool,
    evidence: Option<gaze::EvidenceKind>,
) {
    run_text_case(provider, stream, source_first, evidence, SOURCE, COPY).await;
}

async fn run_text_case(
    provider: Provider,
    stream: bool,
    source_first: bool,
    evidence: Option<gaze::EvidenceKind>,
    source: &str,
    copy: &str,
) {
    let forwarded = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/{*path}", post(capture))
        .with_state(Capture {
            forwarded: forwarded.clone(),
            provider,
            stream,
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let upstream_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = listener.local_addr().unwrap();
    let (adapter, path): (Arc<dyn ProviderAdapter>, &str) = match provider {
        Provider::OpenAi => (
            Arc::new(OpenAiAdapter::new(upstream)),
            "/v1/chat/completions",
        ),
        Provider::Gemini => (
            Arc::new(GeminiAdapter::new(upstream)),
            if stream {
                "/v1beta/models/gemini-test:streamGenerateContent"
            } else {
                "/v1beta/models/gemini-test:generateContent"
            },
        ),
        Provider::Anthropic => (Arc::new(AnthropicAdapter::new(upstream)), "/v1/messages"),
    };
    let config = ProxyConfig::new(bind, vec![adapter]);
    let mut builder = Pipeline::builder()
        .recognizer(Header(evidence))
        .rule(DefaultRule::new(Action::Tokenize));
    if evidence.is_none() {
        builder = builder.register_safety_net(HeaderNet);
    }
    let pipeline = Arc::new(builder.build().unwrap());
    let proxy_task = tokio::spawn(async move {
        gaze_proxy::serve_with_listener(config, pipeline, listener)
            .await
            .unwrap()
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
            "proxy did not start"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let texts = if source_first {
        [source, copy, PUBLIC]
    } else {
        [copy, PUBLIC, source]
    };
    let request = match provider {
        Provider::OpenAi | Provider::Anthropic => json!({
            "model":"claude-test", "max_tokens":128, "stream":stream,
            "messages":texts.map(|text| json!({"role":"user","content":text}))
        }),
        Provider::Gemini => {
            json!({"contents":texts.map(|text| json!({"role":"user","parts":[{"text":text}]}))})
        }
    };
    let response = client
        .post(format!("http://{bind}{path}"))
        .header("x-api-key", "synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .json(&request)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let reply = response.text().await.unwrap();
    assert!(
        status.is_success(),
        "{provider:?} stream={stream} source_first={source_first}: {status} {reply}"
    );
    let captured = forwarded.lock().await.clone();
    assert_eq!(captured.len(), 1);
    for (index, raw) in texts.iter().enumerate() {
        let protected = provider.text(&captured[0], index);
        if *raw == PUBLIC || (*raw == copy && evidence != Some(gaze::EvidenceKind::Rule)) {
            assert_eq!(protected, *raw, "{provider:?} counterweight");
        } else {
            for part in ["maria", "schneider", "johanna"] {
                assert!(
                    !protected.to_lowercase().contains(part),
                    "{provider:?} stream={stream} source_first={source_first}: {protected}"
                );
            }
        }
    }
    let restored = if stream {
        reply
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .filter_map(|body| provider.answer(&body).map(str::to_owned))
            .collect::<String>()
    } else {
        provider
            .answer(&serde_json::from_str::<Value>(&reply).unwrap())
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        restored, texts[0],
        "{provider:?} stream={stream} exact restore"
    );
    proxy_task.abort();
    upstream_task.abort();
}

async fn check_rule_source_order(provider: Provider) {
    for stream in [false, true] {
        for source_first in [false, true] {
            run_case(
                provider,
                stream,
                source_first,
                Some(gaze::EvidenceKind::Rule),
            )
            .await;
        }
    }
}

#[tokio::test]
async fn openai_rule_sources_protect_earlier_fields() {
    check_rule_source_order(Provider::OpenAi).await;
}

#[tokio::test]
async fn gemini_rule_sources_protect_earlier_fields() {
    check_rule_source_order(Provider::Gemini).await;
}

#[tokio::test]
async fn anthropic_rule_sources_protect_earlier_fields() {
    check_rule_source_order(Provider::Anthropic).await;
}

#[tokio::test]
async fn learned_sources_never_propagate_across_request_fields() {
    for provider in [Provider::OpenAi, Provider::Gemini, Provider::Anthropic] {
        for evidence in [Some(gaze::EvidenceKind::Learned), None] {
            run_case(provider, false, false, evidence).await;
        }
    }
}

async fn check_partial_overlaps(provider: Provider) {
    for (source, copy) in [
        (
            SOURCE,
            "From: Maria <m@example.invalid>\nHello Maria schneider",
        ),
        (
            SOURCE,
            "From: Schneider <m@example.invalid>\nHello maria Schneider",
        ),
        (
            "From: Maria Johanna Schneider <m@example.invalid>\nHello.",
            "From: Johanna <m@example.invalid>\nHello maria Johanna schneider",
        ),
    ] {
        for stream in [false, true] {
            for source_first in [false, true] {
                run_text_case(
                    provider,
                    stream,
                    source_first,
                    Some(gaze::EvidenceKind::Rule),
                    source,
                    copy,
                )
                .await;
            }
        }
    }
}

#[tokio::test]
async fn openai_partial_overlaps_use_original_fields() {
    check_partial_overlaps(Provider::OpenAi).await;
}

#[tokio::test]
async fn gemini_partial_overlaps_use_original_fields() {
    check_partial_overlaps(Provider::Gemini).await;
}

#[tokio::test]
async fn anthropic_partial_overlaps_use_original_fields() {
    check_partial_overlaps(Provider::Anthropic).await;
}
