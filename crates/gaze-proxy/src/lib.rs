#![cfg_attr(docsrs, feature(doc_cfg))]

//! Multi-provider pass-through proxy for Gaze pseudonymization.
//!
//! The proxy does not transcode provider wire formats. Provider adapters only
//! identify native PII-bearing JSON surfaces; the server applies the supplied
//! [`gaze::Pipeline`] and session manifest.

pub mod adapter;
pub mod adapters;
#[allow(
    dead_code,
    reason = "constructors are consumed by the next server integration checkpoint"
)]
pub mod codec;
#[allow(
    dead_code,
    reason = "codec probes are consumed by the next server integration checkpoint"
)]
pub mod codecs;
#[cfg(feature = "proxy-daemon")]
pub mod daemon;
pub mod error;
pub mod inspection;
pub mod principal;
pub mod server;
#[allow(
    dead_code,
    reason = "finite registry is integrated by the serialized P3/P4 checkpoints"
)]
pub(crate) mod session_registry;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use adapters::AnthropicAdapter;
use gaze::Pipeline;

pub use adapter::{
    AdapterContract, CoveragePolicy, FormatPolicy, HeaderPolicy, PiiSurface, ProtocolContract,
    ProviderAdapter, RedirectPolicy, RoutePolicy, SessionPolicy, SessionRegistryConfig,
    SessionRegistryConfigError, SseEvent,
};
pub use codec::{
    BodyCodec, CodecError, CodecErrorCode, CodecLimits, CodecPhase, OutputProvenance,
    ProvedRequestBody, ProvedResponseBody, RequestPseudonymizer, RequestTransformContext,
    ResponseResidualValidator, ResponseTransformContext, WireFormat,
};
pub use codecs::anthropic::{
    AnthropicMessagesCodec, ANTHROPIC_PROXY_ERROR_FRAME, ANTHROPIC_PROXY_PING_FRAME,
};
pub use error::{DirectProxyError, ProxyError, ProxyErrorCode, ProxyErrorPhase};
pub use inspection::{install_proxy_inspection_v1, ProxyInspectionProducerV1};
pub use principal::{
    AuthenticatedPrincipal, ListenerScope, LocalAuthCredential, PeerScope, PrincipalContext,
    PrincipalResolveError, PrincipalResolver, ProcessLocalLoopbackResolver,
    TlsClientCertificateHash, MAX_AUTHENTICATED_PRINCIPAL_BYTES, MAX_LOCAL_AUTH_CREDENTIAL_BYTES,
};
pub use server::{
    classify_upstream_status, parse_request_stream_format, serve, validate_declared_body,
    validate_direct_response_head, DirectResponseHead, HealthSnapshot, DIRECT_PROXY_ERROR_FRAME,
    DIRECT_PROXY_PING_FRAME,
};

#[derive(Clone)]
#[non_exhaustive]
pub struct ProxyConfig {
    pub bind: SocketAddr,
    pub adapters: Vec<Arc<dyn ProviderAdapter>>,
    pub session_ttl: Duration,
    pub body_limit_bytes: u64,
    pub(crate) direct_anthropic: Option<Arc<AnthropicAdapter>>,
    pub(crate) inspection: Option<Arc<ProxyInspectionProducerV1>>,
}

impl ProxyConfig {
    pub fn new(bind: SocketAddr, adapters: Vec<Arc<dyn ProviderAdapter>>) -> Self {
        Self {
            bind,
            adapters,
            session_ttl: Duration::from_secs(30 * 60),
            body_limit_bytes: 2 * 1024 * 1024,
            direct_anthropic: None,
            inspection: None,
        }
    }

    /// Constructs the strict direct Anthropic profile without a generic trait-object handoff.
    ///
    /// This preserves builder-only settings such as continuity, allowlists, timeouts, and an
    /// optional trusted principal resolver. [`Self::new`] remains source-compatible for legacy
    /// multi-provider configurations and uses the direct profile's frozen defaults.
    #[must_use]
    pub fn anthropic_direct(bind: SocketAddr, adapter: AnthropicAdapter) -> Self {
        let adapter = Arc::new(adapter);
        let dynamic: Arc<dyn ProviderAdapter> = adapter.clone();
        Self {
            bind,
            adapters: vec![dynamic],
            session_ttl: Duration::from_secs(30 * 60),
            body_limit_bytes: 2 * 1024 * 1024,
            direct_anthropic: Some(adapter),
            inspection: None,
        }
    }

    /// Installs the immutable provider-neutral inspection producer for this launch.
    ///
    /// The producer can be created only by [`install_proxy_inspection_v1`], which atomically
    /// binds it to the adopter-supplied pending consumer half. Omitting this method constructs no
    /// inspection registration or runtime.
    #[must_use]
    pub fn with_inspection(mut self, inspection: ProxyInspectionProducerV1) -> Self {
        self.inspection = Some(Arc::new(inspection));
        self
    }
}

pub async fn serve_foreground(
    config: ProxyConfig,
    pipeline: Arc<Pipeline>,
) -> Result<(), ProxyError> {
    serve(config, pipeline).await
}
