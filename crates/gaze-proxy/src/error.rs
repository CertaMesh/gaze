use std::path::PathBuf;

use http::Method;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProxyError {
    #[error("upstream unreachable: {url}")]
    UpstreamUnreachable {
        url: Url,
        #[source]
        source: reqwest::Error,
    },
    #[error("adapter not found for {method} {path}")]
    AdapterNotFound { path: String, method: Method },
    #[error("body too large: limit {limit_bytes} bytes")]
    BodyTooLarge { limit_bytes: u64 },
    #[error("invalid json body")]
    InvalidJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("sse partial frame: {reason}")]
    SsePartialFrame { reason: String },
    #[error("pipeline failed")]
    Pipeline {
        #[source]
        source: gaze::Error,
    },
    #[error("http server failed")]
    Server {
        #[source]
        source: std::io::Error,
    },
    #[error("daemon already running: pid {pid} ({pidfile})", pidfile = pidfile.display())]
    DaemonAlreadyRunning { pid: u32, pidfile: PathBuf },
    #[error("daemon is not running")]
    DaemonNotRunning,
    #[error("daemon pidfile stale: {pidfile}", pidfile = pidfile.display())]
    DaemonPidfileStale { pidfile: PathBuf },
    #[error("daemon io failed: {path}", path = path.display())]
    DaemonIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("daemon config failed: {detail}")]
    DaemonConfig { detail: String },
}
