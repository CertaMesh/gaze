use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use gaze::{token_shape, CleanDocument, Pipeline, RawDocument, Scope, Session};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use url::Url;

use crate::adapter::{ProviderAdapter, SseEvent};
use crate::error::ProxyError;
use crate::ProxyConfig;

const SESSION_HEADER: &str = "x-gaze-session-id";

#[derive(Clone)]
struct AppState {
    config: Arc<ProxyConfig>,
    pipeline: Arc<Pipeline>,
    client: Client,
    sessions: Arc<RwLock<HashMap<String, SessionEntry>>>,
    started_at: Instant,
}

struct SessionEntry {
    session: Arc<Session>,
    expires_at: Instant,
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct HealthSnapshot {
    pub uptime_secs: u64,
    pub bind: String,
    pub adapters: Vec<AdapterSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct AdapterSnapshot {
    pub name: &'static str,
    pub upstream: String,
}

pub async fn serve(config: ProxyConfig, pipeline: Arc<Pipeline>) -> Result<(), ProxyError> {
    let bind = config.bind;
    let state = AppState {
        config: Arc::new(config),
        pipeline,
        client: Client::new(),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        started_at: Instant::now(),
    };
    let app = Router::new()
        .route("/_gaze_proxy/healthz", get(healthz))
        .fallback(proxy)
        .with_state(state);
    let listener = TcpListener::bind(bind)
        .await
        .map_err(|source| ProxyError::Server { source })?;
    axum::serve(listener, app)
        .await
        .map_err(|source| ProxyError::Server { source })
}

async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(health_snapshot(&state))
}

fn health_snapshot(state: &AppState) -> HealthSnapshot {
    HealthSnapshot {
        uptime_secs: state.started_at.elapsed().as_secs(),
        bind: state.config.bind.to_string(),
        adapters: state
            .config
            .adapters
            .iter()
            .map(|adapter| AdapterSnapshot {
                name: adapter.name(),
                upstream: adapter.upstream_base().to_string(),
            })
            .collect(),
    }
}

async fn proxy(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match proxy_inner(state, method, uri, headers, body).await {
        Ok(response) => response,
        Err(err) => proxy_error_response(err),
    }
}

async fn proxy_inner(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ProxyError> {
    if body.len() as u64 > state.config.body_limit_bytes {
        return Err(ProxyError::BodyTooLarge {
            limit_bytes: state.config.body_limit_bytes,
        });
    }
    let path = uri.path();
    let adapter = state
        .config
        .adapters
        .iter()
        .find(|adapter| adapter.matches_path(&method, path))
        .cloned()
        .ok_or_else(|| ProxyError::AdapterNotFound {
            path: path.to_string(),
            method: method.clone(),
        })?;
    let session = session_for(&state, &headers).await?;
    let mut json: Value =
        serde_json::from_slice(&body).map_err(|source| ProxyError::InvalidJson { source })?;
    redact_surfaces(
        &state.pipeline,
        &session,
        adapter.request_pii_surfaces(&mut json),
    )?;

    let upstream_url = upstream_url(adapter.upstream_base(), &uri)?;
    let mut request = state
        .client
        .request(method.clone(), upstream_url.clone())
        .json(&json);
    request = forward_headers(request, &headers);
    let upstream = request
        .send()
        .await
        .map_err(|source| ProxyError::UpstreamUnreachable {
            url: upstream_url.clone(),
            source,
        })?;

    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let content_type = upstream_headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = upstream
        .bytes()
        .await
        .map_err(|source| ProxyError::UpstreamUnreachable {
            url: upstream_url,
            source,
        })?;

    let body = if content_type.contains("text/event-stream") {
        transform_sse(&adapter, &session, &bytes)?
    } else {
        let mut response_json: Value =
            serde_json::from_slice(&bytes).map_err(|source| ProxyError::InvalidJson { source })?;
        restore_surfaces(&session, adapter.response_pii_surfaces(&mut response_json));
        serde_json::to_vec(&response_json).map_err(|source| ProxyError::InvalidJson { source })?
    };

    let mut response = Response::builder().status(status);
    for (name, value) in upstream_headers {
        if let Some(name) = name {
            if name == axum::http::header::CONTENT_LENGTH {
                continue;
            }
            response = response.header(name, value);
        }
    }
    response
        .body(axum::body::Body::from(body))
        .map_err(|source| ProxyError::DaemonConfig {
            detail: source.to_string(),
        })
}

async fn session_for(state: &AppState, headers: &HeaderMap) -> Result<Arc<Session>, ProxyError> {
    let now = Instant::now();
    let id = headers
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    {
        let sessions = state.sessions.read().await;
        if let Some(entry) = sessions.get(&id) {
            if entry.expires_at > now {
                return Ok(entry.session.clone());
            }
        }
    }

    let session = Arc::new(
        Session::new(Scope::Conversation(id.clone()))
            .map_err(|source| ProxyError::Pipeline { source })?,
    );
    let mut sessions = state.sessions.write().await;
    sessions.retain(|_, entry| entry.expires_at > now);
    sessions.insert(
        id,
        SessionEntry {
            session: session.clone(),
            expires_at: now + state.config.session_ttl,
        },
    );
    Ok(session)
}

fn redact_surfaces(
    pipeline: &Pipeline,
    session: &Session,
    surfaces: Vec<crate::adapter::PiiSurface<'_>>,
) -> Result<(), ProxyError> {
    for surface in surfaces {
        let clean = pipeline
            .redact(session, RawDocument::Text(surface.text.clone()))
            .map_err(|source| ProxyError::Pipeline { source })?;
        if let CleanDocument::Text(text) = clean {
            *surface.text = text;
        }
    }
    Ok(())
}

fn restore_surfaces(session: &Session, surfaces: Vec<crate::adapter::PiiSurface<'_>>) {
    for surface in surfaces {
        *surface.text = restore_text(session, surface.text);
    }
}

fn restore_text(session: &Session, clean: &str) -> String {
    let mut restored = String::new();
    let mut last = 0;
    for matched in token_shape::pattern().find_iter(clean) {
        restored.push_str(&clean[last..matched.start()]);
        if let Some(raw) = session.restore(matched.as_str()) {
            restored.push_str(&raw);
        } else {
            restored.push_str(matched.as_str());
        }
        last = matched.end();
    }
    restored.push_str(&clean[last..]);
    restored
}

fn transform_sse(
    adapter: &Arc<dyn ProviderAdapter>,
    session: &Session,
    bytes: &[u8],
) -> Result<Vec<u8>, ProxyError> {
    let text = std::str::from_utf8(bytes).map_err(|err| ProxyError::SsePartialFrame {
        reason: err.to_string(),
    })?;
    let mut out = String::new();
    for frame in text.split("\n\n") {
        if frame.trim().is_empty() {
            continue;
        }
        let mut event_name = None;
        let mut data_lines = Vec::new();
        for line in frame.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event_name = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim_start());
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        let data = data_lines.join("\n");
        if data == "[DONE]" {
            if let Some(name) = event_name {
                out.push_str("event: ");
                out.push_str(&name);
                out.push('\n');
            }
            out.push_str("data: [DONE]\n\n");
            continue;
        }
        let mut event = SseEvent {
            event: event_name.clone(),
            data: serde_json::from_str(&data)
                .map_err(|source| ProxyError::InvalidJson { source })?,
        };
        restore_surfaces(session, adapter.sse_event_pii_surfaces(&mut event));
        if let Some(name) = event_name {
            out.push_str("event: ");
            out.push_str(&name);
            out.push('\n');
        }
        out.push_str("data: ");
        out.push_str(
            &serde_json::to_string(&event.data)
                .map_err(|source| ProxyError::InvalidJson { source })?,
        );
        out.push_str("\n\n");
    }
    Ok(out.into_bytes())
}

fn upstream_url(base: &Url, uri: &Uri) -> Result<Url, ProxyError> {
    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    base.join(path).map_err(|err| ProxyError::DaemonConfig {
        detail: err.to_string(),
    })
}

fn forward_headers(
    mut request: reqwest::RequestBuilder,
    headers: &HeaderMap,
) -> reqwest::RequestBuilder {
    const FORWARDED: &[&str] = &[
        "authorization",
        "x-api-key",
        "anthropic-version",
        "openai-beta",
        "content-type",
    ];
    for name in FORWARDED {
        let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        if let Some(value) = headers.get(&header_name) {
            request = request.header(name.to_string(), value.clone());
        }
    }
    request
}

fn proxy_error_response(err: ProxyError) -> Response {
    let status = match err {
        ProxyError::AdapterNotFound { .. } => StatusCode::NOT_FOUND,
        ProxyError::BodyTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        ProxyError::InvalidJson { .. } => StatusCode::BAD_REQUEST,
        ProxyError::UpstreamUnreachable { .. } => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let body = serde_json::json!({
        "error": proxy_error_name(&err),
    });
    (
        status,
        [(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        body.to_string(),
    )
        .into_response()
}

fn proxy_error_name(err: &ProxyError) -> &'static str {
    match err {
        ProxyError::UpstreamUnreachable { .. } => "UpstreamUnreachable",
        ProxyError::AdapterNotFound { .. } => "AdapterNotFound",
        ProxyError::BodyTooLarge { .. } => "BodyTooLarge",
        ProxyError::InvalidJson { .. } => "InvalidJson",
        ProxyError::SsePartialFrame { .. } => "SsePartialFrame",
        ProxyError::Pipeline { .. } => "Pipeline",
        ProxyError::Server { .. } => "Server",
        ProxyError::DaemonAlreadyRunning { .. } => "DaemonAlreadyRunning",
        ProxyError::DaemonNotRunning => "DaemonNotRunning",
        ProxyError::DaemonPidfileStale { .. } => "DaemonPidfileStale",
        ProxyError::DaemonIo { .. } => "DaemonIo",
        ProxyError::DaemonConfig { .. } => "DaemonConfig",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_text_leaves_unknown_tokens() {
        let session = Session::new(Scope::Conversation("test".to_string())).unwrap();
        assert_eq!(restore_text(&session, "hello <Email_1>"), "hello <Email_1>");
    }

    #[test]
    fn upstream_url_preserves_path_and_query() {
        let base = Url::parse("https://api.openai.com").unwrap();
        let uri: Uri = "/v1/chat/completions?x=1".parse().unwrap();
        assert_eq!(
            upstream_url(&base, &uri).unwrap().as_str(),
            "https://api.openai.com/v1/chat/completions?x=1"
        );
    }

    #[test]
    fn health_lists_adapters() {
        let config = ProxyConfig::new(
            "127.0.0.1:0".parse().unwrap(),
            vec![Arc::new(crate::adapters::OpenAiAdapter::new(
                Url::parse("https://api.openai.com").unwrap(),
            ))],
        );
        let state = AppState {
            config: Arc::new(config),
            pipeline: Arc::new(Pipeline::builder().build().unwrap()),
            client: Client::new(),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            started_at: Instant::now(),
        };
        assert_eq!(health_snapshot(&state).adapters[0].name, "openai");
    }
}
