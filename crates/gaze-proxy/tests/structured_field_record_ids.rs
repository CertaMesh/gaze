//! Labelled record IDs across real provider routes; loopback fixtures only, no provider calls.
//!
//! The bundled `field_labelled` rules protect labelled string leaves of structured carriers
//! and refuse labelled JSON numbers, typed, before anything is forwarded. Prose and
//! JSON-encoded carriers (a JSON document inside a string) have no field context, so there
//! only the calibrated adopter rule acts, with its legacy numeric rendering.

#[path = "support/structured_record_ids.rs"]
mod record_ids;
mod support;

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use gaze::{CleanDocument, Scope, Session};
use gaze_assembly::CorePipelineConfig;
use gaze_proxy::adapters::{AnthropicAdapter, GeminiAdapter, OpenAiAdapter};
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use url::Url;

#[derive(Clone, Copy, Debug)]
enum Provider {
    Anthropic,
    OpenAi,
    Gemini,
}

impl Provider {
    fn post(self, client: &Client, bind: std::net::SocketAddr) -> reqwest::RequestBuilder {
        let request = client.post(format!("http://{bind}{}", self.path()));
        if matches!(self, Self::Anthropic) {
            request
                .header("anthropic-version", "2023-06-01")
                .header("x-api-key", "synthetic-key")
        } else {
            request
        }
    }
    fn adapter(self, upstream: Url) -> Arc<dyn ProviderAdapter> {
        match self {
            Self::Anthropic => Arc::new(AnthropicAdapter::new(upstream)),
            Self::OpenAi => Arc::new(OpenAiAdapter::new(upstream)),
            Self::Gemini => Arc::new(GeminiAdapter::new(upstream)),
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Anthropic => "/v1/messages",
            Self::OpenAi => "/v1/chat/completions",
            Self::Gemini => "/v1beta/models/gemini-test:generateContent",
        }
    }

    fn response(self) -> Value {
        match self {
            Self::Anthropic => {
                json!({"id":"msg_test","type":"message","role":"assistant","model":"claude-test","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}})
            }
            Self::OpenAi => json!({"choices":[{"message":{"role":"assistant","content":"ok"}}]}),
            Self::Gemini => {
                json!({"candidates":[{"content":{"role":"model","parts":[{"text":"ok"}]}}]})
            }
        }
    }

    fn request(self, payload: Value, carrier: &str) -> Value {
        let text = payload
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| payload.to_string());
        match (self, carrier) {
            (Self::Anthropic, "call") => {
                json!({"model":"claude-test","max_tokens":16,"messages":[{"role":"user","content":"look up the record"},{"role":"assistant","content":[{"type":"tool_use","id":"toolu_synthetic","name":"lookup","input":payload}]}]})
            }
            (Self::Anthropic, "result-string" | "result-blocks") => {
                let content = if carrier == "result-string" {
                    json!(text)
                } else {
                    json!([{"type":"text","text":text}])
                };
                json!({"model":"claude-test","max_tokens":16,"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"toolu_synthetic","name":"lookup","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_synthetic","content":content}]}]})
            }
            (Self::OpenAi, "call") => {
                json!({"model":"gpt-test","messages":[{"role":"user","content":"look up the record"},{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":text}}]}]})
            }
            (Self::OpenAi, "result-string" | "result-blocks") => {
                let content = if carrier == "result-string" {
                    json!(text)
                } else {
                    json!([{"type":"text","text":text}])
                };
                json!({"model":"gpt-test","messages":[{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_1","content":content}]})
            }
            (Self::Gemini, "call") => {
                json!({"contents":[{"role":"user","parts":[{"text":"look up the record"}]},{"role":"model","parts":[{"functionCall":{"name":"lookup","args":payload}}]}]})
            }
            (Self::Gemini, "result-structured") => {
                json!({"contents":[{"role":"model","parts":[{"functionCall":{"name":"lookup","args":{}}}]},{"role":"user","parts":[{"functionResponse":{"name":"lookup","response":payload}}]}]})
            }
            (Self::Gemini, "result-string") => {
                self.request(json!({"result":text}), "result-structured")
            }
            (Self::Anthropic, "prose") => {
                json!({"model":"claude-test","max_tokens":16,"messages":[{"role":"user","content":text}]})
            }
            (Self::OpenAi, "prose") => {
                json!({"model":"gpt-test","messages":[{"role":"user","content":text}]})
            }
            (Self::Gemini, "prose") => {
                json!({"contents":[{"role":"user","parts":[{"text":text}]}]})
            }
            _ => panic!("unsupported fixture: {self:?} {carrier}"),
        }
    }
}

fn assembled() -> gaze_assembly::CorePipeline {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(record_ids::RULEPACK.as_bytes()).unwrap();
    CorePipelineConfig::new()
        .with_rulepack_path(file.path().into())
        .build()
        .unwrap()
}

/// Whether a typed labelled-number refusal is the expected outcome for this route.
fn refuses(provider: Provider, carrier: &str, case: &record_ids::Case) -> bool {
    case.numeric
        && matches!(
            (provider, carrier),
            (Provider::Anthropic | Provider::Gemini, "call")
                | (Provider::Gemini, "result-structured")
        )
}

#[test]
fn whole_json_fields_bind_and_prose_stays_with_the_calibrated_rule() {
    for (core, stock) in [
        (assembled(), false),
        (CorePipelineConfig::new().build().unwrap(), true),
    ] {
        for case in record_ids::cases() {
            for (kind, text) in [
                ("prose", case.prose()),
                ("whole-json", case.document().to_string()),
            ] {
                let session = Session::new(Scope::Ephemeral).unwrap();
                let surface = format!(
                    "{}/{kind}",
                    if stock { "stock-core" } else { "calibration" }
                );
                match core.pseudonymize_text(&session, text.clone()) {
                    Err(gaze::Error::Protection(gaze::ProtectionError::Untransformable {
                        reason,
                        ..
                    })) => {
                        assert!(kind == "whole-json" && case.numeric, "{surface} {case:?}");
                        assert_eq!(
                            reason,
                            gaze::UntransformableReason::JsonNumberNoTextOnlyPath
                        );
                        println!("RECORD_ID {surface} {case:?} REFUSED");
                    }
                    Ok(CleanDocument::Text(clean)) => {
                        assert!(
                            !(kind == "whole-json" && case.numeric),
                            "{surface} {case:?}"
                        );
                        // No free-text label guessing: stock prose keeps its known leak.
                        record_ids::check(&case, &surface, &clean, stock && kind == "prose");
                        if kind == "whole-json" {
                            let parsed: Value = serde_json::from_str(&clean).unwrap();
                            assert_eq!(
                                session.restore_strict_text(&clean).unwrap(),
                                text,
                                "{surface} {case:?} restore"
                            );
                            assert!(parsed.is_object());
                        }
                    }
                    other => panic!("{surface} {case:?}: unexpected {other:?}"),
                }
            }
        }
    }
}

async fn captures(provider: Provider) {
    let (upstream, forwarded, upstream_task) =
        support::spawn_json_upstream(move |_| provider.response()).await;
    let core = assembled();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = listener.local_addr().unwrap();
    let config = ProxyConfig::new(bind, vec![provider.adapter(upstream)])
        .with_locale_chain(core.locale_chain().clone());
    let proxy_task = support::spawn_proxy(
        config,
        || Arc::new(core.into_pipeline()),
        listener,
        support::Readiness::Poll {
            interval: Duration::from_millis(10),
        },
    )
    .await;
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    let carriers = match provider {
        Provider::Gemini => ["call", "result-string", "result-structured"],
        _ => ["call", "result-string", "result-blocks"],
    };
    for case in record_ids::cases() {
        for carrier in carriers.into_iter().chain(["prose"]) {
            let payload = if carrier == "prose" {
                json!(case.prose())
            } else {
                case.document()
            };
            let response = provider
                .post(&client, bind)
                .json(&provider.request(payload, carrier))
                .send()
                .await
                .unwrap();
            if refuses(provider, carrier, &case) {
                assert!(
                    !response.status().is_success(),
                    "{provider:?}/{carrier} {case:?}"
                );
                let body = response.text().await.unwrap();
                assert!(
                    body.contains("json_number_no_text_only_path"),
                    "{provider:?}/{carrier} {case:?}: typed reason expected, got {body}"
                );
                assert!(!body.contains("731284"), "refusal body is value-free");
                assert!(forwarded.lock().await.is_empty(), "nothing forwarded");
                println!("RECORD_ID {provider:?}/{carrier} {case:?} REFUSED");
                continue;
            }
            assert!(
                response.status().is_success(),
                "{provider:?} {carrier} {case:?}: expected successful HTTP status"
            );
            let captured = forwarded
                .lock()
                .await
                .pop()
                .expect("request must reach capture upstream");
            record_ids::check(
                &case,
                &format!("{provider:?}/{carrier}"),
                &captured.to_string(),
                false,
            );
            let encoded = match (provider, carrier) {
                (Provider::Anthropic, "result-string") => Some("/messages/1/content/0/content"),
                (Provider::Anthropic, "result-blocks") => {
                    Some("/messages/1/content/0/content/0/text")
                }
                (Provider::OpenAi, "call") => Some("/messages/1/tool_calls/0/function/arguments"),
                (Provider::OpenAi, "result-string") => Some("/messages/1/content"),
                (Provider::OpenAi, "result-blocks") => Some("/messages/1/content/0/text"),
                (Provider::Gemini, "result-string") => {
                    Some("/contents/1/parts/0/functionResponse/response/result")
                }
                _ => None,
            };
            if let Some(pointer) = encoded {
                // JSON inside a string carries no field context: the calibrated prose rule
                // still renders a number as an unquoted token, the legacy defect this change
                // leaves to the encoded-carrier increment.
                let text = captured.pointer(pointer).and_then(Value::as_str).unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(text).is_ok(),
                    !case.numeric,
                    "encoded-carrier numeric rendering changed at {pointer}; promote when fixed"
                );
            }
        }
    }
    for carrier in carriers.into_iter().chain(["prose"]) {
        let payload = if carrier == "prose" {
            json!("Unlabelled value 731284.")
        } else {
            json!({"count":731284,"description":"731284"})
        };
        let request = provider.request(payload, carrier);
        let response = provider
            .post(&client, bind)
            .json(&request)
            .send()
            .await
            .unwrap();
        // D3: unlabelled numbers pass unchanged on every route, Anthropic tool input included.
        let status = response.status();
        if !status.is_success() {
            panic!(
                "{provider:?}/{carrier} control: {}",
                response.text().await.unwrap()
            );
        }
        let captured = forwarded.lock().await.pop().unwrap();
        assert!(
            captured == request,
            "unlabelled numbers and strings must stay unchanged"
        );
    }
    proxy_task.abort();
    upstream_task.abort();
}

#[tokio::test]
async fn anthropic_structured_calls_protect_strings_and_refuse_labelled_numbers() {
    captures(Provider::Anthropic).await;
}
#[tokio::test]
async fn gemini_structured_calls_and_responses_protect_strings_and_refuse_labelled_numbers() {
    captures(Provider::Gemini).await;
}
#[tokio::test]
async fn openai_serialized_arguments_and_tool_results_are_protected() {
    captures(Provider::OpenAi).await;
}
