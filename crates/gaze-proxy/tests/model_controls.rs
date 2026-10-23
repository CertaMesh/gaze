//! Routing identifiers remain immutable, with rules active even when learned probes disagree.
use std::sync::Arc;

use gaze::{
    Action, DefaultRule, Detection, Detector, DictionaryBundle, EvidenceKind, LeakKind,
    LeakSuspect, LocaleTag, PiiClass, Pipeline, RulepackDict, SafetyNet, SafetyNetContext,
    SafetyNetError,
};
use gaze_proxy::adapters::{AnthropicAdapter, GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use gaze_recognizers::{DictionaryRecognizer, RegexDetector};
use reqwest::{Client, StatusCode};
use serde_json::json;

mod support;

const NAME: &str = "Anna Schneider";
const MODELS: &[&str] = &[
    "claude-sonnet-4-5",
    "claude-sonnet-5",
    "claude-sonnet-4-6",
    "claude-sonnet-4-5-20250929",
    "claude-opus-4-1",
    "publisher/model.v1:latest@region",
];

struct LearnedNames;
impl Detector for LearnedNames {
    fn detect(&self, input: &str) -> Vec<Detection> {
        MODELS
            .iter()
            .copied()
            .chain([NAME, "Claudé", "claude sonnet"])
            .filter_map(|value| {
                input.find(value).map(|at| {
                    Detection::new(at..at + value.len(), PiiClass::Name, "synthetic.learned")
                })
            })
            .collect()
    }
}

struct CuedRule;
impl Detector for CuedRule {
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Rule
    }
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .find("catalog: session-secret")
            .map(|at| {
                Detection::new(
                    at + 9..at + 9 + "session-secret".len(),
                    PiiClass::Custom("catalog".into()),
                    "synthetic.cued",
                )
            })
            .into_iter()
            .collect()
    }
}

struct LearnedUsernames;
impl SafetyNet for LearnedUsernames {
    fn id(&self) -> &str {
        "synthetic.username-net"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        input: &str,
        _: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        Ok(MODELS
            .iter()
            .filter_map(|value| {
                input.find(value).map(|at| {
                    LeakSuspect::new(
                        at..at + value.len(),
                        PiiClass::Custom("username".into()),
                        self.id(),
                        Some(0.99),
                        LeakKind::Uncovered,
                        "USERNAME",
                        None,
                    )
                })
            })
            .collect())
    }
}

fn pipeline(ner: bool, net: bool) -> Pipeline {
    let mut builder = Pipeline::builder()
        .detector(CuedRule)
        .detector(RegexDetector::emails().unwrap())
        .detector(
            RegexDetector::new(r"\b202-555-0142\b", PiiClass::Custom("phone".into())).unwrap(),
        )
        .detector(
            RegexDetector::new(
                r"\bDE89370400440532013000\b",
                PiiClass::Custom("iban".into()),
            )
            .unwrap(),
        )
        .recognizer(DictionaryRecognizer::new(
            "dictionary.routing",
            PiiClass::Custom("catalog".into()),
            "routing",
            true,
            "catalog",
        ))
        .rule(DefaultRule::new(Action::Tokenize));
    if ner {
        builder = builder.detector(LearnedNames);
    }
    let pipeline = builder.build().unwrap();
    if net {
        pipeline.with_safety_net(LearnedUsernames)
    } else {
        pipeline
    }
}

async fn exercise_models(ner: bool, net: bool) {
    for provider in ["anthropic", "openai", "gemini"] {
        let (upstream, forwarded, upstream_task) = support::spawn_json_upstream(|request| {
            json!({"id":"msg_test","type":"message","role":"assistant","model":request.get("model").cloned().unwrap_or(json!("safe-model")),
                "content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn",
                "stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}})
        })
        .await;
        let adapter: Arc<dyn ProviderAdapter> = match provider {
            "anthropic" => Arc::new(AnthropicAdapter::new(upstream)),
            "openai" => Arc::new(OpenAiAdapter::new(upstream)),
            "gemini" => Arc::new(GeminiAdapter::new(upstream)),
            _ => unreachable!(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bind = listener.local_addr().unwrap();
        let dictionaries = DictionaryBundle::from_rulepack_terms(&[RulepackDict::new(
            "routing",
            vec!["catalog-secret".into()],
            true,
        )]);
        let config = ProxyConfig::new(bind, vec![adapter]).with_dictionaries(dictionaries);
        let pipeline = Arc::new(pipeline(ner, net));
        let proxy = support::spawn_proxy(
            config,
            move || pipeline,
            listener,
            support::Readiness::HealthJson { adapter: provider },
        )
        .await;
        let client = Client::new();
        for (model, accepted) in MODELS
            .iter()
            .map(|model| ((*model).to_string(), true))
            .chain([(format!("claude-sonnet-5{}.", ".xxxxxxx".repeat(14)), true)])
            .chain(
                [
                    "alice@example.invalid".to_string(),
                    "202-555-0142".to_string(), // NANPA fictional range.
                    "DE89370400440532013000".to_string(),
                    "catalog-secret".to_string(),
                ]
                .into_iter()
                .chain(ner.then_some(NAME.to_string()))
                .chain(ner.then_some("claude sonnet".to_string()))
                .chain(ner.then_some("Claudé".to_string()))
                .chain(ner.then(|| format!("{}claude-sonnet-5", "x.".repeat(65))))
                .map(|model| (model, false)),
            )
        {
            let (path, body) = match provider {
                "anthropic" => (
                    "/v1/messages".to_string(),
                    json!({"model":model,"max_tokens":16,"messages":[{"role":"user","content":"hello"}]}),
                ),
                "openai" => (
                    "/v1/chat/completions".to_string(),
                    json!({"model":model,"messages":[{"role":"user","content":"hello"}]}),
                ),
                "gemini" => {
                    let mut url = url::Url::parse(&format!("http://{bind}/")).unwrap();
                    url.path_segments_mut().unwrap().extend([
                        "v1beta",
                        "models",
                        &format!("{model}:generateContent"),
                    ]);
                    (
                        url.path().to_string(),
                        json!({"contents":[{"parts":[{"text":"hello"}]}]}),
                    )
                }
                _ => unreachable!(),
            };
            let before = forwarded.lock().await.len();
            let response = client
                .post(format!("http://{bind}{path}"))
                .header("x-api-key", "synthetic-key")
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let result = response.text().await.unwrap();
            assert_eq!(
                status,
                if accepted {
                    StatusCode::OK
                } else {
                    StatusCode::UNPROCESSABLE_ENTITY
                },
                "{provider} {model}: {result}"
            );
            if !accepted
                && [
                    "alice@example.invalid",
                    "202-555-0142",
                    "DE89370400440532013000",
                    "catalog-secret",
                ]
                .contains(&model.as_str())
            {
                let error: serde_json::Value = serde_json::from_str(&result).unwrap();
                assert_eq!(
                    if provider == "anthropic" {
                        &error["error"]["code"]
                    } else {
                        &error["error"]
                    },
                    "ControlWouldMutate"
                );
            }
            let requests = forwarded.lock().await;
            assert_eq!(
                requests.len(),
                before + usize::from(accepted),
                "{provider} {model}"
            );
            if accepted && provider != "gemini" {
                assert_eq!(requests.last().unwrap()["model"], model);
            }
        }
        let prompt = if ner { NAME } else { "claude-sonnet-5" };
        let (path, body) = match provider {
            "anthropic" => (
                "/v1/messages",
                json!({"model":"safe-model","max_tokens":16,"messages":[{"role":"user","content":prompt}]}),
            ),
            "openai" => (
                "/v1/chat/completions",
                json!({"model":"safe-model","messages":[{"role":"user","content":prompt}]}),
            ),
            "gemini" => (
                "/v1beta/models/safe-model:generateContent",
                json!({"contents":[{"parts":[{"text":prompt}]}]}),
            ),
            _ => unreachable!(),
        };
        let response = client
            .post(format!("http://{bind}{path}"))
            .header("x-api-key", "synthetic-key")
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{provider}: {}",
            response.text().await.unwrap()
        );
        let requests = forwarded.lock().await;
        let request = requests.last().unwrap();
        let content = if provider == "gemini" {
            &request["contents"][0]["parts"][0]["text"]
        } else {
            &request["messages"][0]["content"]
        };
        assert!(
            !content.as_str().unwrap().contains(prompt),
            "learned protection must remain active in messages: {request}"
        );
        drop(requests);

        let mut other_control = body;
        match provider {
            "anthropic" => {
                other_control["messages"][0]["content"] = json!("hello");
                other_control["stop_sequences"] = json!(["claude-sonnet-5"]);
            }
            "openai" => {
                other_control["messages"][0]["content"] = json!("hello");
                other_control["previous_response_id"] = json!("claude-sonnet-5");
            }
            "gemini" => {
                other_control["contents"][0]["parts"][0]["text"] = json!("hello");
                // Gemini routes by URI. A JSON field called model gets no exemption.
                other_control["model"] = json!("claude-sonnet-5");
            }
            _ => unreachable!(),
        }
        let before = forwarded.lock().await.len();
        let response = client
            .post(format!("http://{bind}{path}"))
            .header("x-api-key", "synthetic-key")
            .header("anthropic-version", "2023-06-01")
            .json(&other_control)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{provider}: other controls must retain learned scans: {}",
            response.text().await.unwrap()
        );
        assert_eq!(forwarded.lock().await.len(), before);

        let mut known_body = other_control;
        let known_path = match provider {
            "anthropic" => {
                known_body.as_object_mut().unwrap().remove("stop_sequences");
                known_body["model"] = json!("session-secret");
                known_body["messages"][0]["content"] = json!("catalog: session-secret");
                path
            }
            "openai" => {
                known_body
                    .as_object_mut()
                    .unwrap()
                    .remove("previous_response_id");
                known_body["model"] = json!("session-secret");
                known_body["messages"][0]["content"] = json!("catalog: session-secret");
                path
            }
            "gemini" => {
                known_body.as_object_mut().unwrap().remove("model");
                known_body["contents"][0]["parts"][0]["text"] = json!("catalog: session-secret");
                "/v1beta/models/session-secret:generateContent"
            }
            _ => unreachable!(),
        };
        for outside_grammar in [false, true] {
            let mut known_body = known_body.clone();
            let known_path = if outside_grammar {
                if provider == "gemini" {
                    "/v1beta/models/session-secret%20:generateContent"
                } else {
                    known_body["model"] = json!("session-secret ");
                    known_path
                }
            } else {
                known_path
            };
            let model = known_body.as_object_mut().unwrap().remove("model");
            let remaining = known_body.to_string();
            let payload = model.map_or_else(
                || remaining.clone(),
                |model| format!("{{\"model\":{model},{}", &remaining[1..]),
            );
            let response = client
                .post(format!("http://{bind}{known_path}"))
                .header("x-api-key", "synthetic-key")
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .body(payload)
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{provider}: known rule-backed PII in the model must not reach upstream: {}",
                response.text().await.unwrap()
            );
            assert_eq!(forwarded.lock().await.len(), before);
        }
        proxy.abort();
        upstream_task.abort();
    }
}

#[tokio::test]
async fn model_controls_skip_ner_but_keep_rules() {
    exercise_models(true, false).await;
}

#[tokio::test]
async fn model_controls_skip_safety_nets_but_keep_rules() {
    exercise_models(false, true).await;
}

fn response_message(model: &str) -> serde_json::Value {
    json!({"id":"msg_test","type":"message","role":"assistant","model":model,
        "content":[],"stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":1,"output_tokens":1}})
}

#[tokio::test]
async fn response_models_keep_deterministic_and_full_fallback_checks() {
    use axum::{routing::post, Router};
    use std::sync::Mutex;

    for provider in ["anthropic", "openai", "gemini"] {
        for (stream, nested) in [(false, false), (true, false), (true, true)] {
            if nested && provider != "openai" {
                continue;
            }
            let reply = Arc::new(Mutex::new(response_message("safe-model")));
            let upstream_reply = Arc::clone(&reply);
            let app = Router::new().route("/{*path}", post(move || {
                let message = upstream_reply.lock().unwrap().clone();
                async move {
                    if !stream {
                        return ([("content-type", "application/json")], message.to_string());
                    }
                    let frames = if provider == "anthropic" {
                        vec![
                            ("message_start", json!({"type":"message_start","message":message})),
                            ("message_delta", json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}})),
                            ("message_stop", json!({"type":"message_stop"})),
                        ]
                    } else if nested {
                        // Responses API lifecycle events nest model under response.
                        vec![("response.completed", json!({"type":"response.completed","response":message}))]
                    } else {
                        vec![("", message)]
                    };
                    let body = frames.into_iter().map(|(event, data)| {
                        format!("event: {event}\ndata: {data}\n\n")
                    }).collect::<String>();
                    ([("content-type", "text/event-stream")], body)
                }
            }));
            let (address, upstream_task) = support::spawn_upstream(app).await;
            let upstream = url::Url::parse(&format!("http://{address}")).unwrap();
            let adapter: Arc<dyn ProviderAdapter> = match provider {
                "anthropic" => Arc::new(AnthropicAdapter::new(upstream)),
                "openai" => Arc::new(OpenAiAdapter::new(upstream)),
                "gemini" => Arc::new(GeminiAdapter::new(upstream)),
                _ => unreachable!(),
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let bind = listener.local_addr().unwrap();
            let pipeline = Arc::new(pipeline(true, true));
            let proxy = support::spawn_proxy(
                ProxyConfig::new(bind, vec![adapter]),
                move || pipeline,
                listener,
                support::Readiness::HealthJson { adapter: provider },
            )
            .await;
            for (field, model, accepted) in [
                ("claude-sonnet-4-5".to_string(), true),
                ("alice@example.invalid".to_string(), false),
                ("202-555-0142".to_string(), false), // NANPA fictional range.
                (NAME.to_string(), false),
                ("Claudé".to_string(), false),
                (format!("{}claude-sonnet-5", "x.".repeat(65)), false),
                ("session-secret".to_string(), false),
                ("session-secret ".to_string(), false),
            ]
            .into_iter()
            .map(|(model, accepted)| ("model", model, accepted))
            .chain((provider == "anthropic").then(|| ("id", "claude-sonnet-5".to_string(), false)))
            {
                let mut message = response_message("safe-model");
                message[field] = json!(model);
                *reply.lock().unwrap() = message;
                let (path, body) = match provider {
                    "anthropic" => (
                        "/v1/messages",
                        json!({"model":"safe-model","max_tokens":16,"stream":stream,"messages":[{"role":"user","content":"catalog: session-secret"}]}),
                    ),
                    "openai" => (
                        "/v1/responses",
                        json!({"model":"safe-model","stream":stream,"input":"catalog: session-secret"}),
                    ),
                    "gemini" => (
                        if stream {
                            "/v1beta/models/safe-model:streamGenerateContent"
                        } else {
                            "/v1beta/models/safe-model:generateContent"
                        },
                        json!({"contents":[{"parts":[{"text":"catalog: session-secret"}]}]}),
                    ),
                    _ => unreachable!(),
                };
                let response = Client::new()
                    .post(format!("http://{bind}{path}"))
                    .header("x-api-key", "synthetic-key")
                    .header("anthropic-version", "2023-06-01")
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                let status = response.status();
                let result = response.text().await.unwrap();
                if stream && provider == "anthropic" && !accepted {
                    // Direct streaming has already sent headers; the sanitized error frame closes it.
                    assert!(
                        result.contains("proxy_validation_failed"),
                        "{provider} {model}: {result}"
                    );
                    assert!(!result.contains(&model), "rejected bytes escaped: {result}");
                } else {
                    assert_eq!(
                        status,
                        if accepted {
                            StatusCode::OK
                        } else {
                            StatusCode::BAD_GATEWAY
                        },
                        "{provider} stream={stream} {model}: {result}"
                    );
                    if accepted {
                        assert!(result.contains(&model), "echo changed: {result}");
                    } else {
                        assert!(
                            result.contains("ProviderOriginPii"),
                            "{provider} {model}: {result}"
                        );
                        assert!(!result.contains(&model), "rejected bytes escaped: {result}");
                    }
                }
            }
            proxy.abort();
            upstream_task.abort();
        }
    }
}
