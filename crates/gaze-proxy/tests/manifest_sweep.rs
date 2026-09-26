//! Repeat-value sweep (solo todo 3849) through the proxy: a name found by a
//! rule in one message must not ship raw in another message of the same
//! request. Synthetic names only.

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use gaze::{
    Action, Candidate, ConflictTier, DefaultRule, DetectContext, PiiClass, Pipeline, Recognizer,
};
use gaze_proxy::adapters::OpenAiAdapter;
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

/// Finds `From: <name> <` like the bundled `email.header.name` rule.
struct Header;

impl Recognizer for Header {
    fn id(&self) -> &str {
        "header"
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

async fn capture(
    State(forwarded): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> Json<Value> {
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
async fn a_rule_found_name_is_swept_in_every_message_of_the_request() {
    let forwarded = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/{*path}", post(capture))
        .with_state(forwarded.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let upstream_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let bind = unused_local_addr();
    let config = ProxyConfig::new(
        bind,
        vec![Arc::new(OpenAiAdapter::new(upstream)) as Arc<dyn ProviderAdapter>],
    );
    let pipeline = Arc::new(
        Pipeline::builder()
            .recognizer(Header)
            .rule(DefaultRule::new(Action::Tokenize))
            .build()
            .unwrap(),
    );
    let proxy_task =
        tokio::spawn(async move { gaze_proxy::serve(config, pipeline).await.unwrap() });
    let client = Client::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !client
        .get(format!("http://{bind}/_gaze_proxy/healthz"))
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

    let response = client
        .post(format!("http://{bind}/v1/chat/completions"))
        .json(&json!({
            "model": "gpt-test",
            "messages": [
                {"role": "user", "content": "From: Maria Schneider <m@example.invalid>\nHello."},
                {"role": "user", "content": "hi, this is maria schneider again. Thanks, Maria"}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    let forwarded = forwarded.lock().await.clone();
    assert_eq!(forwarded.len(), 1);
    let serialized = forwarded[0].to_string().to_lowercase();
    assert!(!serialized.contains("maria"), "{serialized}");
    assert!(!serialized.contains("schneider"), "{serialized}");
    proxy_task.abort();
    upstream_task.abort();
}
