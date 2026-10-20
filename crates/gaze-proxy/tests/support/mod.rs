use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use gaze::Pipeline;
use gaze_proxy::ProxyConfig;
use reqwest::Client;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use url::Url;

// Each integration test compiles this module separately and uses one readiness mode.
#[allow(dead_code)]
pub enum Readiness {
    Poll { interval: Duration },
    HealthJson { adapter: &'static str },
}

pub async fn spawn_upstream(app: Router) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (address, handle)
}

pub async fn spawn_proxy(
    config: ProxyConfig,
    pipeline: impl FnOnce() -> Arc<Pipeline> + Send + 'static,
    listener: TcpListener,
    readiness: Readiness,
) -> JoinHandle<()> {
    let bind = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        gaze_proxy::serve_with_listener(config, pipeline(), listener)
            .await
            .unwrap();
    });
    wait_for_proxy(bind, readiness).await;
    handle
}

pub async fn wait_for_proxy(bind: SocketAddr, readiness: Readiness) {
    let interval = match readiness {
        Readiness::HealthJson { adapter } => {
            // The owned listener queues this request until our server is accepting.
            let response = tokio::time::timeout(Duration::from_secs(60), async {
                Client::new()
                    .get(format!("http://{bind}/_gaze_proxy/healthz"))
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap()
                    .json::<Value>()
                    .await
                    .unwrap()
            })
            .await
            .expect("owned proxy listener did not become ready");
            assert_eq!(response["bind"], bind.to_string());
            assert_eq!(response["adapters"][0]["name"], adapter);
            return;
        }
        Readiness::Poll { interval } => interval,
    };
    let client = Client::new();
    let health_url = format!("http://{bind}/_gaze_proxy/healthz");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(response) = client
            .get(&health_url)
            .timeout(Duration::from_secs(60))
            .send()
            .await
        {
            if response.status().is_success() {
                return;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "proxy did not start at {bind}"
        );
        tokio::time::sleep(interval).await;
    }
}

#[derive(Clone)]
struct JsonUpstreamState {
    forwarded: Arc<Mutex<Vec<Value>>>,
    response: Arc<dyn Fn(Value) -> Value + Send + Sync>,
}

// Only the JSON-response fixtures need this capture handler.
#[allow(dead_code)]
pub async fn spawn_json_upstream(
    response: impl Fn(Value) -> Value + Send + Sync + 'static,
) -> (Url, Arc<Mutex<Vec<Value>>>, JoinHandle<()>) {
    let forwarded = Arc::new(Mutex::new(Vec::new()));
    let state = JsonUpstreamState {
        forwarded: forwarded.clone(),
        response: Arc::new(response),
    };
    let app = Router::new()
        .route("/{*path}", post(capture_json_upstream))
        .with_state(state);
    let (addr, handle) = spawn_upstream(app).await;
    (
        Url::parse(&format!("http://{addr}")).unwrap(),
        forwarded,
        handle,
    )
}

async fn capture_json_upstream(
    State(state): State<JsonUpstreamState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    state.forwarded.lock().await.push(body.clone());
    Json((state.response)(body))
}
