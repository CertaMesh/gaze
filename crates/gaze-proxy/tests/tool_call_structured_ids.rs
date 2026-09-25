//! National IDs and NBSP-grouped IBANs inside OpenAI `tool_calls[].function.arguments`
//! (solo todos #3818 + #3819).
//!
//! The adapter hands the arguments string to the pipeline as one text surface, so a real agent
//! tool call looks like `{"bsn":"111222333"}` to the recognizers. Before the fix the cue rules
//! only matched prose (`BSN: 111222333`) and the IBAN pattern only ASCII-space groups, so all
//! three values below reached the upstream raw. This drives the real proxy end to end and reads
//! the body the upstream received.
//!
//! Fixture values are synthetic, checksum-valid test numbers.

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use gaze::LocaleTag;
use gaze_assembly::CorePipelineConfig;
use gaze_proxy::adapters::OpenAiAdapter;
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

const BSN: &str = "111222333";
const STEUER_ID: &str = "86095742719";
/// `DE89 3704 0044 0532 0130 00` with NO-BREAK SPACE between the groups.
const IBAN_NBSP: &str = "DE89\u{a0}3704\u{a0}0044\u{a0}0532\u{a0}0130\u{a0}00";

type Forwarded = Arc<Mutex<Vec<Value>>>;

async fn capture(State(forwarded): State<Forwarded>, Json(body): Json<Value>) -> Json<Value> {
    forwarded.lock().await.push(body);
    Json(json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}))
}

fn unused_local_addr() -> SocketAddr {
    StdTcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

#[tokio::test]
async fn tool_call_arguments_with_national_ids_and_nbsp_iban_reach_upstream_tokenized() {
    let forwarded: Forwarded = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/{*path}", post(capture))
        .with_state(forwarded.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let upstream_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let locales = [
        LocaleTag::parse("de-DE").unwrap(),
        LocaleTag::parse("nl-NL").unwrap(),
    ];
    let core = CorePipelineConfig::new()
        .with_bundled_rulepack("core-extended")
        .with_locale(&locales)
        .build()
        .expect("core-extended assembles");
    let chain = core.locale_chain().clone();
    let bind = unused_local_addr();
    let config = ProxyConfig::new(
        bind,
        vec![Arc::new(OpenAiAdapter::new(upstream)) as Arc<dyn ProviderAdapter>],
    )
    .with_locale_chain(chain);
    let pipeline = Arc::new(core.into_pipeline());
    let proxy_task =
        tokio::spawn(async move { gaze_proxy::serve(config, pipeline).await.unwrap() });

    let client = Client::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !client
        .get(format!("http://{bind}/_gaze_proxy/healthz"))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "proxy did not start"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let arguments = json!({"bsn": BSN, "steuer_id": STEUER_ID, "iban": IBAN_NBSP}).to_string();
    let response = client
        .post(format!("http://{bind}/v1/chat/completions"))
        .json(&json!({
            "model": "gpt-test",
            "messages": [
                {"role": "user", "content": "look the customer up"},
                {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "lookup_customer", "arguments": arguments}
                    }]
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());

    let bodies = forwarded.lock().await.clone();
    assert_eq!(bodies.len(), 1);
    let sent = bodies[0]["messages"][1]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .expect("arguments stay a string")
        .to_owned();
    let serialized = bodies[0].to_string();
    for raw in [BSN, STEUER_ID, "3704", "0532"] {
        assert!(
            !serialized.contains(raw),
            "{raw:?} reached upstream: {sent}"
        );
    }
    // `core-extended` carries no `iban` anchor cue bucket, so the IBAN resolves to its fail-closed
    // collision-family token (`Custom:family:payment-card-or-iban`), exactly as it does in prose.
    for class in ["Custom:bsn_", "Custom:steuer_id_", "iban_"] {
        assert!(sent.contains(class), "expected a {class} token in {sent}");
    }

    proxy_task.abort();
    upstream_task.abort();
}
