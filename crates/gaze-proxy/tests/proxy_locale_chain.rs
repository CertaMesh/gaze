//! Locale-chain contract for the legacy (non-codec) proxy request path.
//!
//! Solo todo #2403. Before the fix, `redact_surfaces` called [`gaze::Pipeline::redact`], which
//! pins `[LocaleTag::Global]`, and the outbound residual re-scan pinned the same chain
//! deliberately. Consequence: every recognizer declaring `locales = [...]` — `postal.de`,
//! `postal.us`, the national phone recognizers — was inert on proxied traffic no matter what
//! locale the adopter had configured. An adopter running `de-DE` got no German detection and
//! had no way to observe that. Axis-1 leak, not a precision issue.
//!
//! The fix routes both passes through one source of truth,
//! [`gaze_proxy::ProxyConfig::with_locale_chain`]. These tests hold three things:
//!
//! 1. CALIBRATION — the recognizer under test really is locale-gated, proven against the
//!    pipeline directly, so a proxy-level failure below cannot be a detector miss.
//! 2. ACTIVATION — a configured locale makes the gated recognizer fire on the proxy path,
//!    and the unconfigured default still behaves exactly as it did before.
//! 3. AGREEMENT — the primary surface pass and the residual re-scan reach the same verdict
//!    for every fixture under a configured non-global locale. Neither may be weaker or
//!    stronger than the other; a surfaced position that tokenizes must correspond to an
//!    unsurfaced position that fails closed, and a fixture the primary leaves alone must not
//!    be rejected by the residual.
//!
//! Fixtures are synthetic-only per AGENTS.md rule 2. `10115` is a published Berlin postal
//! prefix, not personal data; it carries no identity on its own and is used purely as a
//! locale-gated recognizer trigger.

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use gaze::{CleanDocument, LocaleChain, LocaleTag, Pipeline, RawDocument, Scope, Session};
use gaze_assembly::CorePipelineConfig;
use gaze_proxy::adapters::OpenAiAdapter;
use gaze_proxy::{ProviderAdapter, ProxyConfig};
use reqwest::{Client, Response, StatusCode};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

/// German postal code. Locale-gated: detected by `postal.de` under a `de-DE` chain only.
const DE_POSTAL_CONTEXT: &str = "Berlin 10115";
const DE_POSTAL_DIGITS: &str = "10115";
/// Locale-agnostic control. `email.basic` is not locale-gated, so it must behave identically
/// under every chain — it separates "the locale chain moved" from "detection broke".
const EMAIL: &str = "alice@example.invalid";
/// Carries no PII under any chain. Guards the residual against over-rejection.
const INERT: &str = "please summarize the attached report";

// ---------------------------------------------------------------------------
// Pipelines
// ---------------------------------------------------------------------------

/// Builds the `core-extended` pipeline plus the locale chain it was assembled under.
///
/// This mirrors what an adopter does: assembly decides which recognizers are REGISTERED,
/// and the returned chain is what decides which of them may FIRE. Handing the same chain to
/// [`ProxyConfig::with_locale_chain`] is the whole contract under test.
fn assembled(locales: &[LocaleTag]) -> (Pipeline, LocaleChain) {
    let core = CorePipelineConfig::new()
        .with_bundled_rulepack("core-extended")
        .with_locale(locales)
        .build()
        .expect("core-extended assembles");
    let chain = core.locale_chain().clone();
    (core.into_pipeline(), chain)
}

fn tokenize_directly(pipeline: &Pipeline, chain: &[LocaleTag], text: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let CleanDocument::Text(clean) = pipeline
        .pseudonymize_with_context(&session, RawDocument::Text(text.to_owned()), chain)
        .expect("pipeline runs")
    else {
        panic!("text variant expected");
    };
    clean
}

// ---------------------------------------------------------------------------
// (1) CALIBRATION — the recognizer is genuinely locale-gated
// ---------------------------------------------------------------------------

/// Proves `postal.de` fires under `de-DE` and is inert under `[Global]`, against the pipeline
/// directly. Without this, an activation failure on the proxy path would be ambiguous between
/// "the proxy pinned the wrong chain" and "the recognizer never matches this fixture".
#[test]
fn calibration_postal_de_is_locale_gated_at_the_pipeline() {
    let (de_pipeline, de_chain) = assembled(&[LocaleTag::DeDe]);
    let de_clean = tokenize_directly(&de_pipeline, de_chain.as_slice(), DE_POSTAL_CONTEXT);
    assert!(
        !de_clean.contains(DE_POSTAL_DIGITS),
        "postal.de must tokenize under de-DE, got {de_clean}"
    );

    let global_clean = tokenize_directly(&de_pipeline, &[LocaleTag::Global], DE_POSTAL_CONTEXT);
    assert!(
        global_clean.contains(DE_POSTAL_DIGITS),
        "postal.de must be inert under a bare Global chain, got {global_clean}"
    );

    // Locale-agnostic control: identical verdict under both chains.
    for chain in [de_chain.as_slice(), &[LocaleTag::Global]] {
        let clean = tokenize_directly(&de_pipeline, chain, EMAIL);
        assert!(
            !clean.contains(EMAIL),
            "email must tokenize under every chain, got {clean}"
        );
    }
}

// ---------------------------------------------------------------------------
// (2) ACTIVATION — the configured chain reaches the proxy primary pass
// ---------------------------------------------------------------------------

/// REGRESSION (todo #2403): a configured `de-DE` chain makes `postal.de` fire on the proxy
/// path. Before the fix this position egressed the raw postal code because
/// `redact_surfaces` pinned `[LocaleTag::Global]`.
#[tokio::test]
async fn regression_configured_locale_activates_gated_recognizer_on_proxy_path() {
    let (pipeline, chain) = assembled(&[LocaleTag::DeDe]);
    let (upstream, proxy) = spawn_openai(pipeline, Some(chain)).await;

    post_accepted(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": format!("ship it to {DE_POSTAL_CONTEXT}")}]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    let content = forwarded["messages"][0]["content"]
        .as_str()
        .expect("string content");
    assert!(
        !content.contains(DE_POSTAL_DIGITS),
        "de-DE was configured, so postal.de must tokenize on the proxy path; forwarded {content}"
    );
    assert!(
        content.contains('<') && content.contains('>'),
        "expected a gaze token in {content}"
    );
}

/// The unconfigured default is unchanged: no locale configured still means `[Global]`, so a
/// locale-gated recognizer stays inert and this adopter sees exactly the pre-fix behavior.
///
/// This is deliberately NOT a leak the fix closes. Widening the no-configuration default would
/// activate `postal.us` / `postal.de` for every adopter at once and is coupled to the open
/// values-vs-bounds numeric-carrier decision (Solo todo #2400).
#[tokio::test]
async fn unconfigured_locale_default_is_unchanged() {
    let (pipeline, _chain) = assembled(&[LocaleTag::DeDe]);
    let (upstream, proxy) = spawn_openai(pipeline, None).await;

    post_accepted(
        &proxy,
        "/v1/chat/completions",
        json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": format!("ship it to {DE_POSTAL_CONTEXT} for {EMAIL}")}]
        }),
    )
    .await;

    let forwarded = upstream.first_forwarded().await;
    let content = forwarded["messages"][0]["content"]
        .as_str()
        .expect("string content");
    assert!(
        content.contains(DE_POSTAL_DIGITS),
        "no locale configured must keep the pre-fix Global behavior; forwarded {content}"
    );
    assert!(
        !content.contains(EMAIL),
        "locale-agnostic detection must still run under the default chain; forwarded {content}"
    );
}

// ---------------------------------------------------------------------------
// (3) AGREEMENT — primary pass and residual re-scan reach the same verdict
// ---------------------------------------------------------------------------

/// The two passes must move together. For each fixture, the same bytes are sent twice under
/// one configured `de-DE` proxy: once in a surfaced position (`messages[].content`, handled by
/// the primary pass) and once in an unsurfaced position (`safety_identifier`, handled only by
/// the residual re-scan). The verdicts must match — tokenized on the primary side if and only
/// if failed closed on the residual side.
///
/// This is the test the mutation probe in the PR body drives RED: pinning the residual back to
/// `[LocaleTag::Global]` while the primary stays on `de-DE` makes the `postal.de` fixture
/// tokenize on the primary side and forward untouched on the residual side.
#[tokio::test]
async fn regression_residual_rescan_agrees_with_primary_under_configured_locale() {
    // (fixture, expected to carry PII under the configured de-DE chain)
    let fixtures = [(DE_POSTAL_CONTEXT, true), (EMAIL, true), (INERT, false)];

    for (fixture, expected_pii) in fixtures {
        // Primary pass: a surfaced position.
        let (pipeline, chain) = assembled(&[LocaleTag::DeDe]);
        let (upstream, proxy) = spawn_openai(pipeline, Some(chain)).await;
        post_accepted(
            &proxy,
            "/v1/chat/completions",
            json!({
                "model": "gpt-test",
                "messages": [{"role": "user", "content": fixture}]
            }),
        )
        .await;
        let forwarded = upstream.first_forwarded().await;
        let content = forwarded["messages"][0]["content"]
            .as_str()
            .expect("string content");
        let primary_saw_pii = content != fixture;
        drop((upstream, proxy));

        // Residual re-scan: an unsurfaced position. `safety_identifier` is a real OpenAI
        // request field that `OpenAiAdapter::request_pii_surfaces` does not claim, so it falls
        // through to `_ => {}` and is reached by the residual re-scan and nothing else.
        let (pipeline, chain) = assembled(&[LocaleTag::DeDe]);
        let (upstream, proxy) = spawn_openai(pipeline, Some(chain)).await;
        let response = post_json(
            &proxy,
            "/v1/chat/completions",
            json!({
                "model": "gpt-test",
                "messages": [{"role": "user", "content": "look it up"}],
                "safety_identifier": fixture
            }),
        )
        .await;
        let residual_saw_pii = response.status() == StatusCode::UNPROCESSABLE_ENTITY;

        assert_eq!(
            primary_saw_pii,
            residual_saw_pii,
            "primary and residual disagreed on {fixture:?}: primary_saw_pii={primary_saw_pii}, \
             residual_saw_pii={residual_saw_pii} (residual status {})",
            response.status()
        );
        assert_eq!(
            primary_saw_pii, expected_pii,
            "unexpected verdict for {fixture:?} under de-DE; forwarded {content}"
        );
        drop((upstream, proxy));
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct UpstreamState {
    forwarded: Arc<Mutex<Vec<Value>>>,
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

async fn spawn_upstream() -> MockUpstream {
    let forwarded = Arc::new(Mutex::new(Vec::new()));
    let state = UpstreamState {
        forwarded: forwarded.clone(),
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
    state.forwarded.lock().await.push(body);
    Json(json!({"choices": [{"text": "ok"}]}))
}

async fn spawn_openai(
    pipeline: Pipeline,
    locale_chain: Option<LocaleChain>,
) -> (MockUpstream, ProxyServer) {
    let upstream = spawn_upstream().await;
    let adapter: Arc<dyn ProviderAdapter> = Arc::new(OpenAiAdapter::new(upstream.base_url.clone()));
    let bind = unused_local_addr();
    let mut config = ProxyConfig::new(bind, vec![adapter]);
    if let Some(chain) = locale_chain {
        config = config.with_locale_chain(chain);
    }
    let pipeline = Arc::new(pipeline);
    let handle = tokio::spawn(async move {
        gaze_proxy::serve(config, pipeline).await.unwrap();
    });
    wait_for_proxy(bind).await;
    (
        upstream,
        ProxyServer {
            base_url: format!("http://{bind}"),
            handle,
        },
    )
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

async fn post_accepted(proxy: &ProxyServer, path: &str, body: Value) {
    let response = post_json(proxy, path, body).await;
    assert!(
        response.status().is_success(),
        "expected the request to be forwarded, got {}",
        response.status()
    );
}
