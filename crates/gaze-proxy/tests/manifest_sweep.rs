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

struct RepeatNet;
impl gaze::SafetyNet for RepeatNet {
    fn id(&self) -> &str {
        "synthetic-repeat-net"
    }
    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: gaze::SafetyNetContext<'_>,
    ) -> Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
        let mut hits: Vec<_> = regex::Regex::new(r"(?i)(?:maria(?:\s+johanna)?|<[0-9a-f]{8}:Name_[0-9]+>)\s+(?:schneider|<[0-9a-f]{8}:Name_[0-9]+>)(?:\s+(?:maria|<[0-9a-f]{8}:Name_[0-9]+>))?").unwrap()
            .find_iter(text).map(|found| gaze::LeakSuspect::new(found.range(), PiiClass::Name,
                self.id(), Some(0.95), gaze::LeakKind::Uncovered, "synthetic full-name repeat", None)).collect();
        if text.starts_with("Public ") && text.contains(":Name_") {
            hits.extend(
                regex::Regex::new(r"\bschneider\b")
                    .unwrap()
                    .find_iter(text)
                    .map(|found| {
                        gaze::LeakSuspect::new(
                            found.range(),
                            PiiClass::Custom("username".into()),
                            self.id(),
                            Some(0.95),
                            gaze::LeakKind::Uncovered,
                            "synthetic contextual handle",
                            None,
                        )
                    }),
            );
        }
        Ok(hits)
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
    run_seeded_case(provider, stream, source_first, evidence, source, copy, None).await;
}

async fn run_seeded_case(
    provider: Provider,
    stream: bool,
    source_first: bool,
    evidence: Option<gaze::EvidenceKind>,
    source: &str,
    copy: &str,
    seed: Option<&str>,
) {
    run_configured_case(
        provider,
        stream,
        source_first,
        evidence,
        (source, copy, None),
        seed,
        false,
    )
    .await;
}

async fn run_configured_case(
    provider: Provider,
    stream: bool,
    source_first: bool,
    evidence: Option<gaze::EvidenceKind>,
    texts: (&str, &str, Option<(&str, bool)>),
    seed: Option<&str>,
    repeat_net: bool,
) {
    run_options_case(
        provider,
        stream,
        source_first,
        evidence,
        texts,
        seed,
        CaseOptions {
            repeat_net,
            ..CaseOptions::default()
        },
    )
    .await;
}

#[derive(Default)]
struct CaseOptions {
    repeat_net: bool,
    expansion_limit_bytes: Option<usize>,
    resource_refusal: bool,
    carrier_copies: Option<usize>,
    nested_carriers: bool,
}

async fn run_options_case(
    provider: Provider,
    stream: bool,
    source_first: bool,
    evidence: Option<gaze::EvidenceKind>,
    texts: (&str, &str, Option<(&str, bool)>),
    seed: Option<&str>,
    options: CaseOptions,
) {
    let (source, copy, unsupported) = texts;
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
        Provider::Anthropic => (
            Arc::new(
                AnthropicAdapter::builder(upstream)
                    .session_registry_config(gaze_proxy::SessionRegistryConfig::default())
                    .build()
                    .unwrap(),
            ),
            "/v1/messages",
        ),
    };
    let mut config = ProxyConfig::new(bind, vec![adapter]);
    config.request_residual_expansion_limit_bytes = options.expansion_limit_bytes;
    let mut builder = Pipeline::builder()
        .recognizer(Header(evidence))
        .rule(DefaultRule::new(Action::Tokenize));
    if options.repeat_net {
        builder = builder.register_safety_net(RepeatNet);
    }
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
    let mut token = String::new();
    if let Some(seed) = seed {
        let seed_text = format!("From: {seed} <m@example.invalid>");
        let request = match provider {
            Provider::OpenAi | Provider::Anthropic => {
                json!({"model":"claude-test", "max_tokens":128, "stream":stream,
                "messages":[{"role":"user","content":seed_text}]})
            }
            Provider::Gemini => json!({"contents":[{"role":"user","parts":[{"text":seed_text}]}]}),
        };
        let response = client
            .post(format!("http://{bind}{path}"))
            .header("x-gaze-session-id", "00000000-0000-4000-8000-000000000869")
            .header("x-api-key", "synthetic-key")
            .header("anthropic-version", "2023-06-01")
            .json(&request)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let reply = response.text().await.unwrap();
        assert!(status.is_success(), "seed: {provider:?} {status} {reply}");
        let capture = forwarded.lock().await.pop().unwrap();
        token = regex::Regex::new(r"<[0-9a-f]{8}:Name_[0-9]+>")
            .unwrap()
            .find(provider.text(&capture, 0))
            .unwrap()
            .as_str()
            .to_owned();
    }
    let copy_owned = copy.replace("{token}", &token);
    let copy = copy_owned.as_str();
    let texts = if source_first {
        [source, copy, PUBLIC]
    } else {
        [copy, PUBLIC, source]
    };
    let mut request = match provider {
        Provider::OpenAi | Provider::Anthropic => json!({
            "model":"claude-test", "max_tokens":128, "stream":stream,
            "messages":texts.map(|text| json!({"role":"user","content":text}))
        }),
        Provider::Gemini => {
            json!({"contents":texts.map(|text| json!({"role":"user","parts":[{"text":text}]}))})
        }
    };
    let unsupported_text =
        unsupported.map(|(text, reject)| (text.replace("{token}", &token), reject));
    if let Some((text, _)) = &unsupported_text {
        let leaves = vec![text; options.carrier_copies.unwrap_or(1)];
        request["unsupported_carrier"] = if options.nested_carriers {
            json!({"nested": [{"branch": [leaves]}]})
        } else {
            json!({"nested": leaves})
        };
    }
    let response = client
        .post(format!("http://{bind}{path}"))
        .header("x-gaze-session-id", "00000000-0000-4000-8000-000000000869")
        .header("x-api-key", "synthetic-key")
        .header("anthropic-version", "2023-06-01")
        .json(&request)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let reply = response.text().await.unwrap();
    if options.resource_refusal {
        assert_eq!(status, reqwest::StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            serde_json::from_str::<Value>(&reply).unwrap(),
            json!({"error": "ResidualExpansionLimitExceeded"})
        );
        assert!(
            forwarded.lock().await.is_empty(),
            "resource refusal reached upstream"
        );
        proxy_task.abort();
        upstream_task.abort();
        return;
    }
    if unsupported_text.as_ref().is_some_and(|(_, reject)| *reject) {
        assert!(
            !status.is_success(),
            "{provider:?} stream={stream}: unsafe carrier forwarded"
        );
        if !matches!(provider, Provider::Anthropic) {
            assert_eq!(status, reqwest::StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(
                serde_json::from_str::<Value>(&reply).unwrap(),
                json!({"error":"UnsurfacedPii"})
            );
        }
        assert!(
            forwarded.lock().await.is_empty(),
            "request reached upstream"
        );
        assert!(!reply.to_lowercase().contains("schneider"));
        proxy_task.abort();
        upstream_task.abort();
        return;
    }
    assert!(
        status.is_success(),
        "{provider:?} stream={stream} source_first={source_first}: {status} {reply}"
    );
    let captured = forwarded.lock().await.clone();
    assert_eq!(captured.len(), 1);
    if unsupported_text.is_some() {
        assert_eq!(
            captured[0]["unsupported_carrier"],
            request["unsupported_carrier"]
        );
    }
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
    if seed.is_some() && unsupported_text.is_none() {
        let index = usize::from(source_first);
        assert!(
            provider.text(&captured[0], index).contains(&token),
            "owned token changed"
        );
    }
    let expected = if let Some(seed) = seed {
        texts[0].replace(&token, seed)
    } else {
        texts[0].to_owned()
    };
    assert_eq!(
        restored, expected,
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

async fn check_existing_token(provider: Provider, repeat_net: bool) {
    for (seed, source, copy) in [
        ("Maria", SOURCE, "Hello {token} schneider"),
        ("Schneider", SOURCE, "Hello maria {token}"),
        (
            "Johanna",
            "From: Maria Johanna Schneider <m@example.invalid>\nHello.",
            "Hello maria {token} schneider",
        ),
        (
            "Maria",
            "From: Maria Schneider Maria <m@example.invalid>\nHello.",
            "Hello {token} schneider {token}",
        ),
        ("Maria Sch", SOURCE, "Hello {token}neider"),
        ("ia Schneider", SOURCE, "Hello Mar{token}"),
    ] {
        for stream in [false, true] {
            for source_first in [false, true] {
                run_configured_case(
                    provider,
                    stream,
                    source_first,
                    Some(gaze::EvidenceKind::Rule),
                    (source, copy, None),
                    Some(seed),
                    repeat_net,
                )
                .await;
            }
        }
    }
}

#[tokio::test]
async fn learned_and_net_existing_tokens_do_not_seed_longer_repeats() {
    for provider in [Provider::OpenAi, Provider::Gemini, Provider::Anthropic] {
        for evidence in [Some(gaze::EvidenceKind::Learned), None] {
            run_seeded_case(
                provider,
                false,
                false,
                evidence,
                SOURCE,
                "Hello {token} schneider",
                Some("Maria"),
            )
            .await;
        }
    }
}

#[tokio::test]
async fn openai_existing_token_cannot_hide_suffix() {
    check_existing_token(Provider::OpenAi, false).await;
}
#[tokio::test]
async fn gemini_existing_token_cannot_hide_suffix() {
    check_existing_token(Provider::Gemini, false).await;
}
#[tokio::test]
async fn anthropic_existing_token_cannot_hide_suffix() {
    check_existing_token(Provider::Anthropic, false).await;
}

#[tokio::test]
async fn openai_rule_discovery_precedes_full_value_net() {
    check_existing_token(Provider::OpenAi, true).await;
}
#[tokio::test]
async fn gemini_rule_discovery_precedes_full_value_net() {
    check_existing_token(Provider::Gemini, true).await;
}
#[tokio::test]
async fn anthropic_rule_discovery_precedes_full_value_net() {
    check_existing_token(Provider::Anthropic, true).await;
}

async fn check_complete_net_context(provider: Provider) {
    for stream in [false, true] {
        for source_first in [false, true] {
            run_configured_case(
                provider,
                stream,
                source_first,
                Some(gaze::EvidenceKind::Rule),
                (SOURCE, "Public {token}; thanks schneider", None),
                Some("Maria"),
                true,
            )
            .await;
        }
    }
}
#[tokio::test]
async fn openai_complete_leaf_net_preserves_owned_tokens() {
    check_complete_net_context(Provider::OpenAi).await;
}
#[tokio::test]
async fn gemini_complete_leaf_net_preserves_owned_tokens() {
    check_complete_net_context(Provider::Gemini).await;
}
#[tokio::test]
async fn anthropic_complete_leaf_net_preserves_owned_tokens() {
    check_complete_net_context(Provider::Anthropic).await;
}

async fn check_contained_overlap(provider: Provider) {
    for stream in [false, true] {
        for source_first in [false, true] {
            run_configured_case(
                provider,
                stream,
                source_first,
                Some(gaze::EvidenceKind::Rule),
                (
                    "From: Schneider Johanna <m@example.invalid>\nHello.",
                    "Hello {token} johanna",
                    None,
                ),
                Some("Maria Schneider"),
                false,
            )
            .await;
        }
    }
}

#[tokio::test]
async fn openai_contained_overlap_covers_all_raw_intersections() {
    check_contained_overlap(Provider::OpenAi).await;
}
#[tokio::test]
async fn gemini_contained_overlap_covers_all_raw_intersections() {
    check_contained_overlap(Provider::Gemini).await;
}
#[tokio::test]
async fn anthropic_contained_overlap_covers_all_raw_intersections() {
    check_contained_overlap(Provider::Anthropic).await;
}

async fn check_unsupported_rule_carriers(provider: Provider, stream: bool) {
    for (source, carrier, seed) in [
        (SOURCE, "The earlier sender is maria schneider again.", None),
        (SOURCE, "Hello {token} schneider", Some("Maria")),
        (
            "From: Schneider Johanna <m@example.invalid>\nHello.",
            "Hello {token} johanna",
            Some("Maria Schneider"),
        ),
        (
            "From: Schneider Johanna Maria <m@example.invalid>\nHello.",
            "Hello {token} johanna {token}",
            Some("Maria Schneider"),
        ),
        (
            "From: Schneider . Maria <m@example.invalid>\nHello.",
            "Hello {token} . {token}",
            Some("Maria Schneider"),
        ),
        (
            "From: Schneider \u{200b} Maria <m@example.invalid>\nHello.",
            "Hello {token} \u{200b} {token}",
            Some("Maria Schneider"),
        ),
    ] {
        run_configured_case(
            provider,
            stream,
            false,
            Some(gaze::EvidenceKind::Rule),
            (source, "ordinary text", Some((carrier, true))),
            seed,
            false,
        )
        .await;
    }
}

#[tokio::test]
async fn openai_unsupported_rule_carriers_json_refuse_before_forwarding() {
    check_unsupported_rule_carriers(Provider::OpenAi, false).await;
}

#[tokio::test]
async fn openai_unsupported_rule_carriers_sse_refuse_before_forwarding() {
    check_unsupported_rule_carriers(Provider::OpenAi, true).await;
}

#[tokio::test]
async fn gemini_unsupported_rule_carriers_json_refuse_before_forwarding() {
    check_unsupported_rule_carriers(Provider::Gemini, false).await;
}

#[tokio::test]
async fn gemini_unsupported_rule_carriers_sse_refuse_before_forwarding() {
    check_unsupported_rule_carriers(Provider::Gemini, true).await;
}

#[tokio::test]
async fn anthropic_unsupported_carriers_remain_fail_closed() {
    for stream in [false, true] {
        check_unsupported_rule_carriers(Provider::Anthropic, stream).await;
    }
}

#[tokio::test]
async fn unsupported_carrier_counterweights_remain_verbatim() {
    for provider in [Provider::OpenAi, Provider::Gemini] {
        for stream in [false, true] {
            for carrier in [
                PUBLIC,
                "const MariaSchneider = 1; public name field; thanks maria schneiderling",
                "thanks maria; thanks schneider",
            ] {
                run_configured_case(
                    provider,
                    stream,
                    false,
                    Some(gaze::EvidenceKind::Rule),
                    (SOURCE, "ordinary text", Some((carrier, false))),
                    None,
                    false,
                )
                .await;
            }
            run_configured_case(
                provider,
                stream,
                false,
                Some(gaze::EvidenceKind::Rule),
                (SOURCE, "ordinary text", Some(("Hello {token}", false))),
                Some("Maria"),
                false,
            )
            .await;
            run_configured_case(
                provider,
                stream,
                false,
                Some(gaze::EvidenceKind::Rule),
                (
                    SOURCE,
                    "ordinary text",
                    Some(("Literal <deadbeef:Name_1> documentation", false)),
                ),
                None,
                false,
            )
            .await;
            for evidence in [Some(gaze::EvidenceKind::Learned), None] {
                run_configured_case(
                    provider,
                    stream,
                    false,
                    evidence,
                    (SOURCE, "ordinary text", Some(("maria schneider", false))),
                    None,
                    false,
                )
                .await;
            }
        }
    }
}

#[tokio::test]
async fn unsupported_owned_copies_allow_only_whitespace_between_covered_spans() {
    for provider in [Provider::OpenAi, Provider::Gemini] {
        for stream in [false, true] {
            for gap in [" ", "\t\n", "\u{00a0}"] {
                let carrier = format!("Hello {{token}}{gap}{{token}}");
                run_configured_case(
                    provider,
                    stream,
                    false,
                    Some(gaze::EvidenceKind::Rule),
                    (
                        "From: Schneider Maria <m@example.invalid>\nHello.",
                        "ordinary text",
                        Some((&carrier, false)),
                    ),
                    Some("Maria Schneider"),
                    false,
                )
                .await;
            }
        }
    }
}

#[tokio::test]
async fn unsupported_large_owned_carrier_preserves_admission_and_rejects_raw_private_byte() {
    let seed = "X".repeat(2048);
    let safe = "{token} ".repeat(1025);
    let private = format!(
        "{}{token} S {token} {}",
        "{token} ".repeat(512),
        "{token} ".repeat(512),
        token = "{token}"
    );
    for provider in [Provider::OpenAi, Provider::Gemini] {
        for stream in [false, true] {
            for (source, carrier, reject) in [
                ("ordinary source".to_owned(), safe.as_str(), false),
                (
                    format!("From: {seed} S {seed} <m@example.invalid>"),
                    private.as_str(),
                    true,
                ),
            ] {
                run_configured_case(
                    provider,
                    stream,
                    false,
                    Some(gaze::EvidenceKind::Rule),
                    (&source, "ordinary text", Some((carrier, reject))),
                    Some(&seed),
                    false,
                )
                .await;
            }
        }
    }
}

#[tokio::test]
async fn unsupported_adjacent_large_owned_carriers_with_public_suffixes_admit() {
    let seed = "X".repeat(2048);
    for suffix in [".", "42", "https://example.invalid/public", "\u{3000}."] {
        let carrier = format!("{}{suffix}", "{token}".repeat(1025));
        for provider in [Provider::OpenAi, Provider::Gemini] {
            for stream in [false, true] {
                run_configured_case(
                    provider,
                    stream,
                    false,
                    Some(gaze::EvidenceKind::Rule),
                    ("ordinary source", "ordinary text", Some((&carrier, false))),
                    Some(&seed),
                    false,
                )
                .await;
            }
        }
    }
}

#[tokio::test]
#[ignore = "explicit 32 MiB expansion resource measurement"]
async fn unsupported_default_cap_sized_carrier_admits() {
    let seed = "X".repeat(2048);
    let carrier = format!("{}{}", "{token}".repeat(16383), ".".repeat(2036));
    run_configured_case(
        Provider::OpenAi,
        false,
        false,
        Some(gaze::EvidenceKind::Rule),
        ("ordinary source", "ordinary text", Some((&carrier, false))),
        Some(&seed),
        false,
    )
    .await;
}

#[tokio::test]
async fn unsupported_carrier_one_byte_above_configured_expansion_cap_has_resource_error() {
    let seed = "X".repeat(128);
    let carrier = format!("{}.", "{token}".repeat(32));
    for provider in [Provider::OpenAi, Provider::Gemini] {
        for stream in [false, true] {
            run_options_case(
                provider,
                stream,
                false,
                Some(gaze::EvidenceKind::Rule),
                ("ordinary source", "ordinary text", Some((&carrier, false))),
                Some(&seed),
                CaseOptions {
                    expansion_limit_bytes: Some(4096),
                    resource_refusal: true,
                    ..CaseOptions::default()
                },
            )
            .await;
        }
    }
}

#[tokio::test]
async fn reviewer_default_aggregate() {
    let seed = "X".repeat(2048);
    let carrier = "{token}".repeat(8192);
    run_options_case(
        Provider::OpenAi,
        false,
        false,
        Some(gaze::EvidenceKind::Rule),
        ("ordinary source", "ordinary text", Some((&carrier, false))),
        Some(&seed),
        CaseOptions {
            carrier_copies: Some(4),
            resource_refusal: true,
            ..CaseOptions::default()
        },
    )
    .await;
}

#[tokio::test]
async fn reviewer_aggregate_leaf_budget() {
    let seed = "X".repeat(128);
    let carrier = "{token}".repeat(32);
    for provider in [Provider::OpenAi, Provider::Gemini] {
        for stream in [false, true] {
            run_options_case(
                provider,
                stream,
                false,
                Some(gaze::EvidenceKind::Rule),
                ("ordinary source", "ordinary text", Some((&carrier, false))),
                Some(&seed),
                CaseOptions {
                    expansion_limit_bytes: Some(4096),
                    carrier_copies: Some(128),
                    nested_carriers: true,
                    resource_refusal: true,
                    ..CaseOptions::default()
                },
            )
            .await;
        }
    }
}

#[tokio::test]
async fn unsupported_carrier_aggregate_budget_boundaries() {
    let seed = "X".repeat(128);
    let carrier = "{token}".repeat(16);
    // Two 2 KiB expanded leaves plus three immutable `role: "user"` leaves.
    let expanded_request_bytes = 4096 + 12;
    for provider in [Provider::OpenAi, Provider::Gemini] {
        for stream in [false, true] {
            for nested_carriers in [false, true] {
                for (limit, resource_refusal) in [
                    (expanded_request_bytes + 1, false),
                    (expanded_request_bytes, false),
                    (expanded_request_bytes - 1, true),
                ] {
                    run_options_case(
                        provider,
                        stream,
                        false,
                        Some(gaze::EvidenceKind::Rule),
                        ("ordinary source", "ordinary text", Some((&carrier, false))),
                        Some(&seed),
                        CaseOptions {
                            expansion_limit_bytes: Some(limit),
                            carrier_copies: Some(2),
                            nested_carriers,
                            resource_refusal,
                            ..CaseOptions::default()
                        },
                    )
                    .await;
                }
            }
        }
    }
}
