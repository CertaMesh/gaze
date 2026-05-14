#![cfg_attr(docsrs, feature(doc_cfg))]

//! Multi-provider pass-through proxy for Gaze pseudonymization.
//!
//! The proxy does not transcode provider wire formats. Provider adapters only
//! identify native PII-bearing JSON surfaces; the server applies the supplied
//! [`gaze::Pipeline`] and session manifest.

pub mod adapter;
pub mod adapters;
#[cfg(feature = "proxy-daemon")]
pub mod daemon;
pub mod error;
pub mod server;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use gaze::Pipeline;

pub use adapter::{PiiSurface, ProviderAdapter, SseEvent};
pub use error::ProxyError;
pub use server::{serve, HealthSnapshot};

#[derive(Clone)]
#[non_exhaustive]
pub struct ProxyConfig {
    pub bind: SocketAddr,
    pub adapters: Vec<Arc<dyn ProviderAdapter>>,
    pub session_ttl: Duration,
    pub body_limit_bytes: u64,
}

impl ProxyConfig {
    pub fn new(bind: SocketAddr, adapters: Vec<Arc<dyn ProviderAdapter>>) -> Self {
        Self {
            bind,
            adapters,
            session_ttl: Duration::from_secs(30 * 60),
            body_limit_bytes: 2 * 1024 * 1024,
        }
    }
}

pub async fn serve_foreground(
    config: ProxyConfig,
    pipeline: Arc<Pipeline>,
) -> Result<(), ProxyError> {
    serve(config, pipeline).await
}
