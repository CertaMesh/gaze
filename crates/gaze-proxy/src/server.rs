use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::StreamExt;
use gaze::{
    token_shape, CleanDocument, CommittedSessionSnapshot, DictionaryBundle, LocaleTag, Pipeline,
    RawDocument, Scope, Session, SessionTransaction, SessionTransactionError,
};
use reqwest::{Client, ClientBuilder};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use url::{Host, Url};

use crate::adapter::{ProtocolContract, ProviderAdapter, SessionPolicy, SseEvent};
use crate::adapters::anthropic::{AnthropicAdapter, DEFAULT_ANTHROPIC_VERSION};
use crate::codec::{
    BodyCodec, CodecError, CodecErrorCode, CodecLimits, CodecPhase, InspectionParsedFactsV1,
    ProvedRequestBody, RequestPseudonymizer, RequestTransformContext, ResponseResidualValidator,
    ResponseTransformContext, WireFormat,
};
use crate::error::{DirectProxyError, ProxyError, ProxyErrorCode, ProxyErrorPhase};
use crate::inspection::{ProxyInspectionEndpointCodesV1, ProxyInspectionLogicalV1};
use crate::principal::{
    invoke_resolver, ListenerScope, PeerScope, PrincipalContext, PrincipalResolver,
    ProcessLocalLoopbackResolver,
};
use crate::session_registry::{RegistryError, SessionLease, SessionRegistry};
use crate::ProxyConfig;

const SESSION_HEADER: &str = "x-gaze-session-id";

/// Compiled ping emitted by a direct SSE response only after its upstream head is accepted.
pub const DIRECT_PROXY_PING_FRAME: &[u8] = b"event: ping\ndata: {\"type\":\"ping\"}\n\n";

/// The only frame emitted after a direct SSE response has opened and later fails closed.
pub const DIRECT_PROXY_ERROR_FRAME: &[u8] = b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"proxy_validation_failed\"}}\n\n";

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(180);
const DEFAULT_MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_HEADERS: usize = 64;
const DEFAULT_MAX_HEADER_NAME_BYTES: usize = 128;
const DEFAULT_MAX_HEADER_VALUE_BYTES: usize = 8 * 1024;
const DEFAULT_MAX_HEADER_BYTES: usize = 64 * 1024;

/// Finite direct-client configuration. Additive builders can only lower frozen defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DirectClientConfig {
    connect_timeout: Duration,
    request_timeout: Duration,
    total_timeout: Duration,
    max_request_bytes: usize,
    max_response_bytes: usize,
    max_error_body_bytes: usize,
    max_headers: usize,
    max_header_name_bytes: usize,
    max_header_value_bytes: usize,
    max_header_bytes: usize,
}

impl Default for DirectClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            total_timeout: DEFAULT_TOTAL_TIMEOUT,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_error_body_bytes: DEFAULT_MAX_ERROR_BODY_BYTES,
            max_headers: DEFAULT_MAX_HEADERS,
            max_header_name_bytes: DEFAULT_MAX_HEADER_NAME_BYTES,
            max_header_value_bytes: DEFAULT_MAX_HEADER_VALUE_BYTES,
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
        }
    }
}

// P3 builds this crate-private substrate before the P4 orchestration wires it into `serve`.
#[allow(dead_code)]
impl DirectClientConfig {
    fn from_anthropic(adapter: Option<&AnthropicAdapter>) -> Self {
        let Some(adapter) = adapter else {
            return Self::default();
        };
        let (max_headers, max_header_name_bytes, max_header_value_bytes, max_header_bytes) =
            adapter.header_limits();
        Self {
            connect_timeout: adapter.connect_timeout(),
            request_timeout: adapter.request_timeout(),
            total_timeout: adapter.total_timeout(),
            max_headers,
            max_header_name_bytes,
            max_header_value_bytes,
            max_header_bytes,
            ..Self::default()
        }
    }

    #[must_use]
    pub(crate) const fn connect_timeout(self) -> Duration {
        self.connect_timeout
    }

    #[must_use]
    pub(crate) const fn request_timeout(self) -> Duration {
        self.request_timeout
    }

    #[must_use]
    pub(crate) const fn total_timeout(self) -> Duration {
        self.total_timeout
    }

    #[must_use]
    pub(crate) const fn max_request_bytes(self) -> usize {
        self.max_request_bytes
    }

    #[must_use]
    pub(crate) const fn max_response_bytes(self) -> usize {
        self.max_response_bytes
    }

    #[must_use]
    pub(crate) const fn max_error_body_bytes(self) -> usize {
        self.max_error_body_bytes
    }

    /// Lowers all finite timeouts for a deployment or deterministic test.
    pub(crate) fn try_with_timeouts(
        mut self,
        connect: Duration,
        request: Duration,
        total: Duration,
    ) -> Result<Self, DirectProxyError> {
        if connect.is_zero()
            || request.is_zero()
            || total.is_zero()
            || connect > DEFAULT_CONNECT_TIMEOUT
            || request > DEFAULT_REQUEST_TIMEOUT
            || total > DEFAULT_TOTAL_TIMEOUT
        {
            return Err(
                ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::UpstreamConfiguration)
            );
        }
        self.connect_timeout = connect;
        self.request_timeout = request;
        self.total_timeout = total;
        Ok(self)
    }

    /// Lowers the successful upstream response-body limit.
    pub(crate) fn try_with_max_response_bytes(
        mut self,
        value: usize,
    ) -> Result<Self, DirectProxyError> {
        if value == 0 || value > DEFAULT_MAX_RESPONSE_BYTES {
            return Err(
                ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::UpstreamConfiguration)
            );
        }
        self.max_response_bytes = value;
        Ok(self)
    }

    /// Lowers the discard-only upstream error-body budget.
    pub(crate) fn try_with_max_error_body_bytes(
        mut self,
        value: usize,
    ) -> Result<Self, DirectProxyError> {
        if value == 0 || value > DEFAULT_MAX_ERROR_BODY_BYTES {
            return Err(
                ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::UpstreamConfiguration)
            );
        }
        self.max_error_body_bytes = value;
        Ok(self)
    }

    /// Lowers the exact outbound request-body limit.
    pub(crate) fn try_with_max_request_bytes(
        mut self,
        value: usize,
    ) -> Result<Self, DirectProxyError> {
        if value == 0 || value > DEFAULT_MAX_REQUEST_BYTES {
            return Err(
                ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::UpstreamConfiguration)
            );
        }
        self.max_request_bytes = value;
        Ok(self)
    }
}

struct DirectRuntime {
    client: DirectClient,
    endpoint: Url,
    session_policy: SessionPolicy,
    registry: Option<SessionRegistry>,
    resolver: Arc<dyn PrincipalResolver>,
    listener_scope: ListenerScope,
    allowed_versions: Arc<[String]>,
    allowed_betas: Arc<[String]>,
    codec_limits: CodecLimits,
    client_config: DirectClientConfig,
    ping_interval: Duration,
    inspection_endpoint_codes: ProxyInspectionEndpointCodesV1,
}

enum DirectIngressSession {
    Ephemeral,
    Continuity(SessionLease),
}

struct PreparedDirectIngress {
    outbound_headers: HeaderMap,
    session: DirectIngressSession,
}

impl DirectRuntime {
    fn from_config(config: &ProxyConfig) -> Result<Option<Self>, ProxyError> {
        let direct_adapters: Vec<_> = config
            .adapters
            .iter()
            .filter(|adapter| matches!(adapter.contract().protocol(), ProtocolContract::Codec(_)))
            .collect();
        if direct_adapters.is_empty() {
            return Ok(None);
        }
        if direct_adapters.len() != 1 {
            return Err(direct_readiness_error("direct_adapter_count_invalid"));
        }
        let adapter = direct_adapters[0];
        let contract = adapter.contract();
        let session_policy = contract
            .session_policy()
            .ok_or_else(|| direct_readiness_error("direct_session_policy_missing"))?;
        let codec_limits = contract
            .codec_limits()
            .ok_or_else(|| direct_readiness_error("direct_codec_limits_missing"))?;
        validate_direct_origin(adapter.upstream_base())
            .map_err(|_| direct_readiness_error("direct_upstream_invalid"))?;
        let mut endpoint = adapter.upstream_base().clone();
        endpoint.set_path("/v1/messages");
        let inspection_endpoint_codes =
            ProxyInspectionEndpointCodesV1::from_validated_origin(&endpoint);

        let explicit = config.direct_anthropic.as_deref();
        if explicit.is_some_and(|value| value.upstream_base() != adapter.upstream_base()) {
            return Err(direct_readiness_error("direct_adapter_mismatch"));
        }
        let client_config = DirectClientConfig::from_anthropic(explicit);
        let client = DirectClient::new(client_config)
            .map_err(|_| direct_readiness_error("direct_client_invalid"))?;
        let listener_scope = if config.bind.ip().is_loopback() {
            ListenerScope::Loopback
        } else {
            ListenerScope::NonLoopback
        };
        let resolver: Arc<dyn PrincipalResolver> =
            if let Some(resolver) = explicit.and_then(AnthropicAdapter::principal_resolver) {
                Arc::clone(resolver)
            } else if listener_scope == ListenerScope::Loopback {
                Arc::new(
                    ProcessLocalLoopbackResolver::new()
                        .map_err(|_| direct_readiness_error("direct_principal_unavailable"))?,
                )
            } else {
                return Err(direct_readiness_error("direct_principal_required"));
            };
        let registry = match session_policy {
            SessionPolicy::EphemeralSingleRequest => None,
            SessionPolicy::OpaqueHeaderContinuity(registry_config) => Some(
                SessionRegistry::new(registry_config)
                    .map_err(|_| direct_readiness_error("direct_registry_invalid"))?,
            ),
        };
        let allowed_versions = explicit.map_or_else(
            || Arc::from([DEFAULT_ANTHROPIC_VERSION.to_string()]),
            |value| Arc::from(value.allowed_versions()),
        );
        let allowed_betas = explicit.map_or_else(
            || Arc::<[String]>::from([]),
            |value| Arc::from(value.allowed_betas()),
        );
        let ping_interval =
            explicit.map_or(Duration::from_secs(10), AnthropicAdapter::ping_interval);
        Ok(Some(Self {
            client,
            endpoint,
            session_policy,
            registry,
            resolver,
            listener_scope,
            allowed_versions,
            allowed_betas,
            codec_limits,
            client_config,
            ping_interval,
            inspection_endpoint_codes,
        }))
    }
}

fn direct_readiness_error(detail: &'static str) -> ProxyError {
    ProxyError::DaemonConfig {
        detail: detail.to_string(),
    }
}

/// One exact already-proved direct request. It has no `Debug` implementation by design.
#[allow(dead_code)]
pub(crate) struct DirectRequest {
    url: Url,
    headers: HeaderMap,
    body: Vec<u8>,
    expected_response: WireFormat,
    inspection_parsed_facts: Option<InspectionParsedFactsV1>,
    signed_or_encrypted_surface: bool,
}

#[allow(dead_code)]
impl DirectRequest {
    /// Consumes the exact codec-proved body after validating the safe upstream URL and headers.
    pub(crate) fn from_proved(
        url: Url,
        headers: HeaderMap,
        proved_body: ProvedRequestBody,
        expected_response: WireFormat,
    ) -> Result<Self, DirectProxyError> {
        validate_direct_url(&url)?;
        validate_outbound_headers(&headers)?;
        if !proved_body.provenance().final_buffer_verified() {
            return Err(ProxyErrorCode::InvalidProvenance.error(ProxyErrorPhase::RequestTransform));
        }
        let (body, inspection_parsed_facts, signed_or_encrypted_surface) =
            proved_body.into_inspection_parts();
        Ok(Self {
            url,
            headers,
            body,
            expected_response,
            inspection_parsed_facts,
            signed_or_encrypted_surface,
        })
    }
}

/// A proved request whose real core transaction has not yet been published.
///
/// This type is crate-private and has exactly one consuming transition to
/// [`CommittedDirectRequest`]. Dropping it publishes neither mappings nor prefix-cache state.
struct PreparedDirectRequest<'session> {
    transaction: SessionTransaction<'session>,
    request: DirectRequest,
    lifecycle: DirectTransactionState,
}

impl<'session> PreparedDirectRequest<'session> {
    fn new(transaction: SessionTransaction<'session>, request: DirectRequest) -> Self {
        Self {
            transaction,
            request,
            lifecycle: DirectTransactionState::new(),
        }
    }

    fn commit(mut self) -> Result<CommittedDirectRequest, SessionTransactionError> {
        let snapshot = self.transaction.commit()?;
        self.lifecycle.state = DirectIoState::CommittedNoIo;
        Ok(CommittedDirectRequest {
            snapshot,
            request: self.request,
            lifecycle: self.lifecycle,
        })
    }
}

/// The only request state accepted by [`DirectClient`].
///
/// Fields stay private so neither the committed snapshot nor the no-I/O state can be forged.
struct CommittedDirectRequest {
    snapshot: CommittedSessionSnapshot,
    request: DirectRequest,
    lifecycle: DirectTransactionState,
}

impl fmt::Debug for CommittedDirectRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommittedDirectRequest")
            .field("lifecycle", &self.lifecycle)
            .finish_non_exhaustive()
    }
}

struct CommittedDirectResponse {
    snapshot: CommittedSessionSnapshot,
    response: DirectResponse,
    lifecycle: DirectTransactionState,
}

struct CommittedDirectSse {
    snapshot: CommittedSessionSnapshot,
    response: reqwest::Response,
    head: DirectResponseHead,
    lifecycle: DirectTransactionState,
    remaining_total_timeout: Duration,
}

impl fmt::Debug for CommittedDirectResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommittedDirectResponse")
            .field("status", &self.response.status())
            .field("format", &self.response.format())
            .field("lifecycle", &self.lifecycle)
            .finish_non_exhaustive()
    }
}

/// Validated minimal upstream response head.
pub struct DirectResponseHead {
    status: StatusCode,
    format: WireFormat,
    headers: HeaderMap,
}

impl fmt::Debug for DirectResponseHead {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DirectResponseHead")
            .field("status", &self.status)
            .field("format", &self.format)
            .field("header_count", &self.headers.len())
            .finish()
    }
}

impl DirectResponseHead {
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    #[must_use]
    pub const fn format(&self) -> WireFormat {
        self.format
    }

    #[must_use]
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

/// A buffered direct response whose head and accumulated body have both passed substrate checks.
#[allow(dead_code)]
pub(crate) struct DirectResponse {
    head: DirectResponseHead,
    body: Vec<u8>,
}

impl fmt::Debug for DirectResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DirectResponse")
            .field("status", &self.head.status)
            .field("format", &self.head.format)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[allow(dead_code)]
impl DirectResponse {
    #[must_use]
    pub(crate) const fn status(&self) -> StatusCode {
        self.head.status()
    }

    #[must_use]
    pub(crate) const fn format(&self) -> WireFormat {
        self.head.format()
    }

    #[must_use]
    pub(crate) const fn headers(&self) -> &HeaderMap {
        self.head.headers()
    }

    #[must_use]
    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }

    #[must_use]
    pub(crate) fn into_body(self) -> Vec<u8> {
        self.body
    }

    fn into_parts(self) -> (DirectResponseHead, Vec<u8>) {
        (self.head, self.body)
    }
}

/// Dedicated hardened client for direct codec-backed adapters.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct DirectClient {
    client: Client,
    config: DirectClientConfig,
}

#[allow(dead_code)]
impl DirectClient {
    /// Builds a client with redirects, proxies, retries, referrers, and decompression disabled.
    pub(crate) fn new(config: DirectClientConfig) -> Result<Self, DirectProxyError> {
        let client = hardened_client_builder(config).build().map_err(|_| {
            ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::UpstreamConfiguration)
        })?;
        Ok(Self { client, config })
    }

    /// Sends the exact request body once and buffers the raw response with accumulated limits.
    async fn execute(
        &self,
        mut committed: CommittedDirectRequest,
    ) -> Result<CommittedDirectResponse, DirectProxyError> {
        committed.lifecycle.require_committed_no_io()?;
        if committed.request.body.len() > self.config.max_request_bytes {
            return Err(
                ProxyErrorCode::RequestBodyLimitExceeded.error(ProxyErrorPhase::RequestValidation)
            );
        }
        committed.lifecycle.mark_io_attempted()?;
        match tokio::time::timeout(
            self.config.total_timeout,
            self.execute_inner(committed.request),
        )
        .await
        {
            Ok(Ok(response)) => Ok(CommittedDirectResponse {
                snapshot: committed.snapshot,
                response,
                lifecycle: committed.lifecycle,
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(ProxyErrorCode::TotalTimeout.error(ProxyErrorPhase::UpstreamBody)),
        }
    }

    async fn execute_sse(
        &self,
        mut committed: CommittedDirectRequest,
    ) -> Result<CommittedDirectSse, DirectProxyError> {
        committed.lifecycle.require_committed_no_io()?;
        if committed.request.body.len() > self.config.max_request_bytes {
            return Err(
                ProxyErrorCode::RequestBodyLimitExceeded.error(ProxyErrorPhase::RequestValidation)
            );
        }
        if committed.request.expected_response != WireFormat::Sse {
            return Err(ProxyErrorCode::InvalidStateTransition.error(ProxyErrorPhase::Framework));
        }
        committed.lifecycle.mark_io_attempted()?;
        let started = tokio::time::Instant::now();
        let opened = tokio::time::timeout(
            self.config.total_timeout,
            self.open_sse_response(committed.request),
        )
        .await
        .map_err(|_| ProxyErrorCode::TotalTimeout.error(ProxyErrorPhase::UpstreamHeaders))??;
        let remaining_total_timeout = self.config.total_timeout.saturating_sub(started.elapsed());
        Ok(CommittedDirectSse {
            snapshot: committed.snapshot,
            response: opened.0,
            head: opened.1,
            lifecycle: committed.lifecycle,
            remaining_total_timeout,
        })
    }

    async fn open_sse_response(
        &self,
        request: DirectRequest,
    ) -> Result<(reqwest::Response, DirectResponseHead), DirectProxyError> {
        let response = self
            .client
            .post(request.url)
            .headers(request.headers)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .body(request.body)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(ProxyErrorCode::UpstreamRedirect.error(ProxyErrorPhase::UpstreamHeaders));
        }
        match validate_direct_response_head_with_config(
            status,
            response.headers(),
            WireFormat::Sse,
            self.config,
        ) {
            Ok(head) => Ok((response, head)),
            Err(error) if is_mapped_upstream_status(error.code()) => {
                discard_error_body(response, self.config.max_error_body_bytes).await;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    async fn execute_inner(
        &self,
        request: DirectRequest,
    ) -> Result<DirectResponse, DirectProxyError> {
        let mut outbound = self
            .client
            .post(request.url)
            .headers(request.headers)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .body(request.body);
        outbound = outbound.timeout(self.config.request_timeout);
        let response = outbound.send().await.map_err(map_reqwest_error)?;
        let status = response.status();

        if status.is_redirection() {
            return Err(ProxyErrorCode::UpstreamRedirect.error(ProxyErrorPhase::UpstreamHeaders));
        }

        let head = validate_direct_response_head_with_config(
            status,
            response.headers(),
            request.expected_response,
            self.config,
        );
        match head {
            Ok(head) => {
                let body = collect_response_body(response, self.config.max_response_bytes).await?;
                validate_declared_body(
                    request.expected_response,
                    &body,
                    self.config.max_response_bytes,
                )?;
                Ok(DirectResponse { head, body })
            }
            Err(error) if is_mapped_upstream_status(error.code()) => {
                discard_error_body(response, self.config.max_error_body_bytes).await;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

#[allow(dead_code)]
fn hardened_client_builder(config: DirectClientConfig) -> ClientBuilder {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .referer(false)
        .retry(reqwest::retry::never())
        .no_gzip()
        .no_brotli()
        .no_zstd()
        .no_deflate()
        .connect_timeout(config.connect_timeout)
        .timeout(config.request_timeout)
}

#[allow(dead_code)]
fn map_reqwest_error(error: reqwest::Error) -> DirectProxyError {
    let code = if error.is_timeout() && error.is_connect() {
        ProxyErrorCode::ConnectTimeout
    } else if error.is_timeout() {
        ProxyErrorCode::RequestTimeout
    } else if error.is_connect() {
        ProxyErrorCode::UpstreamUnreachable
    } else {
        ProxyErrorCode::UpstreamProtocol
    };
    code.error(ProxyErrorPhase::UpstreamConnect)
}

#[allow(dead_code)]
fn map_response_body_error(error: reqwest::Error) -> DirectProxyError {
    let code = if error.is_timeout() {
        ProxyErrorCode::RequestTimeout
    } else {
        ProxyErrorCode::UpstreamProtocol
    };
    code.error(ProxyErrorPhase::UpstreamBody)
}

#[allow(dead_code)]
async fn collect_response_body(
    response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, DirectProxyError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_response_body_error)?;
        let accumulated = body.len().checked_add(chunk.len()).ok_or_else(|| {
            ProxyErrorCode::ResponseBodyLimitExceeded.error(ProxyErrorPhase::UpstreamBody)
        })?;
        if accumulated > limit {
            return Err(
                ProxyErrorCode::ResponseBodyLimitExceeded.error(ProxyErrorPhase::UpstreamBody)
            );
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[allow(dead_code)]
async fn discard_error_body(response: reqwest::Response, limit: usize) {
    let mut stream = response.bytes_stream();
    let mut discarded = 0_usize;
    while discarded < limit {
        let Some(next) = stream.next().await else {
            break;
        };
        let Ok(chunk) = next else {
            break;
        };
        let remaining = limit - discarded;
        discarded += chunk.len().min(remaining);
        if chunk.len() > remaining {
            break;
        }
    }
}

#[allow(dead_code)]
fn validate_direct_url(url: &Url) -> Result<(), DirectProxyError> {
    let safe_authority = url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.host().is_some();
    let safe_scheme = match (url.scheme(), url.host()) {
        ("https", Some(_)) => true,
        ("http", Some(Host::Ipv4(address))) => IpAddr::V4(address).is_loopback(),
        ("http", Some(Host::Ipv6(address))) => IpAddr::V6(address).is_loopback(),
        _ => false,
    };
    if safe_authority && safe_scheme {
        Ok(())
    } else {
        Err(ProxyErrorCode::InvalidUpstreamUrl.error(ProxyErrorPhase::UpstreamConfiguration))
    }
}

fn validate_direct_origin(url: &Url) -> Result<(), DirectProxyError> {
    if url.path() != "/" {
        return Err(
            ProxyErrorCode::InvalidUpstreamUrl.error(ProxyErrorPhase::UpstreamConfiguration)
        );
    }
    validate_direct_url(url)
}

#[allow(dead_code)]
fn validate_outbound_headers(headers: &HeaderMap) -> Result<(), DirectProxyError> {
    const ALLOWED: &[&str] = &[
        "content-type",
        "x-api-key",
        "anthropic-version",
        "anthropic-beta",
    ];
    validate_header_space(headers, DirectClientConfig::default())?;
    let mut singleton_names = HashSet::new();
    for (name, value) in headers {
        if !ALLOWED.contains(&name.as_str()) || value.is_empty() {
            return Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation));
        }
        if matches!(
            name.as_str(),
            "x-api-key" | "anthropic-version" | "anthropic-beta"
        ) && !singleton_names.insert(name.as_str())
        {
            return Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation));
        }
    }
    Ok(())
}

/// Maps an upstream status without reading its body or redirect metadata.
pub fn classify_upstream_status(status: StatusCode) -> Result<(), DirectProxyError> {
    let code = if status.is_success() {
        return Ok(());
    } else if status.is_redirection() {
        ProxyErrorCode::UpstreamRedirect
    } else {
        match status.as_u16() {
            400 => ProxyErrorCode::UpstreamBadRequest,
            401 => ProxyErrorCode::UpstreamUnauthorized,
            403 => ProxyErrorCode::UpstreamForbidden,
            404 => ProxyErrorCode::UpstreamNotFound,
            409 => ProxyErrorCode::UpstreamConflict,
            413 => ProxyErrorCode::UpstreamPayloadTooLarge,
            429 => ProxyErrorCode::UpstreamRateLimited,
            502 | 503 | 504 | 529 => ProxyErrorCode::UpstreamUnavailable,
            value if (400..=499).contains(&value) => ProxyErrorCode::UpstreamClientFailure,
            500..=599 => ProxyErrorCode::UpstreamServerFailure,
            _ => ProxyErrorCode::UpstreamServerFailure,
        }
    };
    Err(code.error(ProxyErrorPhase::UpstreamHeaders))
}

#[allow(dead_code)]
fn is_mapped_upstream_status(code: ProxyErrorCode) -> bool {
    matches!(
        code,
        ProxyErrorCode::UpstreamBadRequest
            | ProxyErrorCode::UpstreamUnauthorized
            | ProxyErrorCode::UpstreamForbidden
            | ProxyErrorCode::UpstreamNotFound
            | ProxyErrorCode::UpstreamConflict
            | ProxyErrorCode::UpstreamPayloadTooLarge
            | ProxyErrorCode::UpstreamRateLimited
            | ProxyErrorCode::UpstreamClientFailure
            | ProxyErrorCode::UpstreamUnavailable
            | ProxyErrorCode::UpstreamServerFailure
    )
}

/// Validates a raw upstream head and rebuilds its minimal transformed header allowlist.
///
/// Header limits, framing, and retry metadata are validated before an error status is mapped, so
/// malformed metadata can never be hidden behind a provider status code.
pub fn validate_direct_response_head(
    status: StatusCode,
    headers: &HeaderMap,
    expected: WireFormat,
) -> Result<DirectResponseHead, DirectProxyError> {
    validate_direct_response_head_with_config(
        status,
        headers,
        expected,
        DirectClientConfig::default(),
    )
}

fn validate_direct_response_head_with_config(
    status: StatusCode,
    headers: &HeaderMap,
    expected: WireFormat,
    config: DirectClientConfig,
) -> Result<DirectResponseHead, DirectProxyError> {
    if status.is_redirection() {
        return Err(ProxyErrorCode::UpstreamRedirect.error(ProxyErrorPhase::UpstreamHeaders));
    }
    validate_header_space(headers, config)?;
    validate_response_framing(headers)?;

    if let Err(error) = classify_upstream_status(status) {
        let retry_after_seconds = parse_retry_after(headers)?;
        return Err(error.with_retry_after_seconds(retry_after_seconds));
    }

    let content_type = one_header(headers, reqwest::header::CONTENT_TYPE)?;
    let actual = match content_type {
        None if expected == WireFormat::Empty => WireFormat::Empty,
        Some(value) => parse_wire_content_type(value)?,
        None => {
            return Err(ProxyErrorCode::InvalidUpstreamResponseFormat
                .error(ProxyErrorPhase::ResponseValidation));
        }
    };
    if actual != expected || (expected == WireFormat::Empty && content_type.is_some()) {
        return Err(ProxyErrorCode::InvalidUpstreamResponseFormat
            .error(ProxyErrorPhase::ResponseValidation));
    }
    let mut minimal = HeaderMap::new();
    if expected != WireFormat::Empty {
        minimal.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static(canonical_content_type(expected)),
        );
    }
    Ok(DirectResponseHead {
        status,
        format: expected,
        headers: minimal,
    })
}

/// Validates one complete declared body without format sniffing or fallback.
pub fn validate_declared_body(
    format: WireFormat,
    body: &[u8],
    limit: usize,
) -> Result<(), DirectProxyError> {
    if body.len() > limit {
        return Err(
            ProxyErrorCode::ResponseBodyLimitExceeded.error(ProxyErrorPhase::ResponseValidation)
        );
    }
    match format {
        WireFormat::Empty if body.is_empty() => Ok(()),
        WireFormat::Empty => Err(ProxyErrorCode::InvalidUpstreamResponseFormat
            .error(ProxyErrorPhase::ResponseValidation)),
        WireFormat::Json => validate_unique_json(body),
        WireFormat::Ndjson => {
            let text = std::str::from_utf8(body).map_err(|_| {
                ProxyErrorCode::InvalidUpstreamResponseFormat
                    .error(ProxyErrorPhase::ResponseValidation)
            })?;
            for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                validate_unique_json(line.as_bytes())?;
            }
            Ok(())
        }
        WireFormat::Utf8Text => std::str::from_utf8(body).map(|_| ()).map_err(|_| {
            ProxyErrorCode::InvalidUpstreamResponseFormat.error(ProxyErrorPhase::ResponseValidation)
        }),
        WireFormat::Sse => validate_sse_envelope(body),
    }
}

fn validate_unique_json(body: &[u8]) -> Result<(), DirectProxyError> {
    let parsed: UniqueJson = serde_json::from_slice(body).map_err(|_| {
        ProxyErrorCode::InvalidUpstreamResponseFormat.error(ProxyErrorPhase::ResponseValidation)
    })?;
    if parsed.duplicate {
        Err(ProxyErrorCode::DuplicateObjectKey.error(ProxyErrorPhase::ResponseValidation))
    } else {
        Ok(())
    }
}

fn validate_sse_envelope(body: &[u8]) -> Result<(), DirectProxyError> {
    if body.starts_with(&[0xef, 0xbb, 0xbf]) || body.contains(&0) {
        return Err(ProxyErrorCode::InvalidSseLifecycle.error(ProxyErrorPhase::ResponseValidation));
    }
    std::str::from_utf8(body).map_err(|_| {
        ProxyErrorCode::InvalidUpstreamResponseFormat.error(ProxyErrorPhase::ResponseValidation)
    })?;
    if body.ends_with(b"\n\n") || body.ends_with(b"\r\r") || body.ends_with(b"\r\n\r\n") {
        Ok(())
    } else {
        Err(ProxyErrorCode::InvalidSseLifecycle.error(ProxyErrorPhase::ResponseValidation))
    }
}

fn validate_header_space(
    headers: &HeaderMap,
    config: DirectClientConfig,
) -> Result<(), DirectProxyError> {
    let mut count = 0_usize;
    let mut aggregate = 0_usize;
    for (name, value) in headers {
        count = count.checked_add(1).ok_or_else(header_limit_error)?;
        aggregate = aggregate
            .checked_add(name.as_str().len())
            .and_then(|value_so_far| value_so_far.checked_add(value.as_bytes().len()))
            .ok_or_else(header_limit_error)?;
        if count > config.max_headers
            || name.as_str().len() > config.max_header_name_bytes
            || value.as_bytes().len() > config.max_header_value_bytes
            || aggregate > config.max_header_bytes
        {
            return Err(header_limit_error());
        }
    }
    Ok(())
}

fn header_limit_error() -> DirectProxyError {
    ProxyErrorCode::HeaderLimitExceeded.error(ProxyErrorPhase::UpstreamHeaders)
}

fn one_header(
    headers: &HeaderMap,
    name: HeaderName,
) -> Result<Option<&HeaderValue>, DirectProxyError> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
        return Err(ProxyErrorCode::InvalidUpstreamHeader.error(ProxyErrorPhase::UpstreamHeaders));
    }
    Ok(first)
}

fn validate_response_framing(headers: &HeaderMap) -> Result<(), DirectProxyError> {
    let content_length = one_header(headers, reqwest::header::CONTENT_LENGTH)?;
    let transfer_encoding = one_header(headers, reqwest::header::TRANSFER_ENCODING)?;
    if content_length.is_some() && transfer_encoding.is_some() {
        return Err(ProxyErrorCode::InvalidFraming.error(ProxyErrorPhase::UpstreamHeaders));
    }
    if let Some(value) = content_length {
        let raw = value.as_bytes();
        if raw.is_empty() || !raw.iter().all(u8::is_ascii_digit) {
            return Err(ProxyErrorCode::InvalidFraming.error(ProxyErrorPhase::UpstreamHeaders));
        }
    }
    if let Some(value) = transfer_encoding {
        if !value.as_bytes().eq_ignore_ascii_case(b"chunked") {
            return Err(ProxyErrorCode::InvalidFraming.error(ProxyErrorPhase::UpstreamHeaders));
        }
    }
    if let Some(value) = one_header(headers, reqwest::header::CONTENT_ENCODING)? {
        if !trim_ascii(value.as_bytes()).eq_ignore_ascii_case(b"identity") {
            return Err(
                ProxyErrorCode::UnsupportedContentEncoding.error(ProxyErrorPhase::UpstreamHeaders)
            );
        }
    }
    Ok(())
}

fn parse_retry_after(headers: &HeaderMap) -> Result<Option<u32>, DirectProxyError> {
    let Some(value) = one_header(headers, reqwest::header::RETRY_AFTER)? else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(ProxyErrorCode::InvalidUpstreamHeader.error(ProxyErrorPhase::UpstreamHeaders));
    }
    let seconds = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.parse::<u32>().ok())
        .ok_or_else(|| {
            ProxyErrorCode::InvalidUpstreamHeader.error(ProxyErrorPhase::UpstreamHeaders)
        })?;
    Ok(Some(seconds))
}

fn parse_wire_content_type(value: &HeaderValue) -> Result<WireFormat, DirectProxyError> {
    let raw = value.as_bytes();
    if !raw.is_ascii() {
        return Err(ProxyErrorCode::InvalidUpstreamResponseFormat
            .error(ProxyErrorPhase::ResponseValidation));
    }
    let mut parts = raw.split(|byte| *byte == b';');
    let media_type = trim_ascii(parts.next().unwrap_or_default());
    let format = if media_type.eq_ignore_ascii_case(b"application/json") {
        WireFormat::Json
    } else if media_type.eq_ignore_ascii_case(b"application/x-ndjson") {
        WireFormat::Ndjson
    } else if media_type.eq_ignore_ascii_case(b"text/plain") {
        WireFormat::Utf8Text
    } else if media_type.eq_ignore_ascii_case(b"text/event-stream") {
        WireFormat::Sse
    } else {
        return Err(ProxyErrorCode::InvalidUpstreamResponseFormat
            .error(ProxyErrorPhase::ResponseValidation));
    };
    for parameter in parts {
        validate_mime_parameter(trim_ascii(parameter))?;
    }
    Ok(format)
}

fn validate_mime_parameter(parameter: &[u8]) -> Result<(), DirectProxyError> {
    let Some(separator) = parameter.iter().position(|byte| *byte == b'=') else {
        return Err(ProxyErrorCode::InvalidUpstreamResponseFormat
            .error(ProxyErrorPhase::ResponseValidation));
    };
    let name = trim_ascii(&parameter[..separator]);
    let value = trim_ascii(&parameter[separator + 1..]);
    let valid_name = !name.is_empty() && name.iter().all(|byte| is_mime_token(*byte));
    let valid_value = if value.len() >= 2 && value[0] == b'"' && value[value.len() - 1] == b'"' {
        value[1..value.len() - 1]
            .iter()
            .all(|byte| matches!(*byte, b'\t' | 0x20..=0x7e) && *byte != b'"' && *byte != b'\\')
    } else {
        !value.is_empty() && value.iter().all(|byte| is_mime_token(*byte))
    };
    if valid_name && valid_value {
        Ok(())
    } else {
        Err(ProxyErrorCode::InvalidUpstreamResponseFormat
            .error(ProxyErrorPhase::ResponseValidation))
    }
}

fn is_mime_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while matches!(value.first(), Some(b' ' | b'\t')) {
        value = &value[1..];
    }
    while matches!(value.last(), Some(b' ' | b'\t')) {
        value = &value[..value.len() - 1];
    }
    value
}

fn canonical_content_type(format: WireFormat) -> &'static str {
    match format {
        WireFormat::Empty => "application/octet-stream",
        WireFormat::Json => "application/json",
        WireFormat::Ndjson => "application/x-ndjson",
        WireFormat::Utf8Text => "text/plain; charset=utf-8",
        WireFormat::Sse => "text/event-stream",
    }
}

struct UniqueJson {
    value: Value,
    duplicate: bool,
}

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

struct UniqueJsonVisitor;

impl<'de> Visitor<'de> for UniqueJsonVisitor {
    type Value = UniqueJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("one bounded JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(unique(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(unique(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(unique(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(unique)
            .ok_or_else(|| E::custom("non-finite number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(unique(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(unique(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(unique(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(unique(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        let mut duplicate = false;
        while let Some(value) = sequence.next_element::<UniqueJson>()? {
            duplicate |= value.duplicate;
            values.push(value.value);
        }
        Ok(UniqueJson {
            value: Value::Array(values),
            duplicate,
        })
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        let mut seen = HashSet::new();
        let mut duplicate = false;
        while let Some(key) = object.next_key::<String>()? {
            duplicate |= !seen.insert(key.clone());
            let value = object.next_value::<UniqueJson>()?;
            duplicate |= value.duplicate;
            values.insert(key, value.value);
        }
        Ok(UniqueJson {
            value: Value::Object(values),
            duplicate,
        })
    }
}

fn unique(value: Value) -> UniqueJson {
    UniqueJson {
        value,
        duplicate: false,
    }
}

/// Parses the request `stream` control as a closed boolean without sniffing or fallback.
pub fn parse_request_stream_format(body: &[u8]) -> Result<WireFormat, DirectProxyError> {
    let parsed: UniqueJson = serde_json::from_slice(body).map_err(|_| {
        ProxyErrorCode::InvalidRequestFormat.error(ProxyErrorPhase::RequestValidation)
    })?;
    if parsed.duplicate {
        return Err(ProxyErrorCode::DuplicateObjectKey.error(ProxyErrorPhase::RequestValidation));
    }
    let Value::Object(object) = parsed.value else {
        return Err(ProxyErrorCode::InvalidRequestFormat.error(ProxyErrorPhase::RequestValidation));
    };
    match object.get("stream") {
        None | Some(Value::Bool(false)) => Ok(WireFormat::Json),
        Some(Value::Bool(true)) => Ok(WireFormat::Sse),
        _ => Err(ProxyErrorCode::InvalidRequestFormat.error(ProxyErrorPhase::RequestValidation)),
    }
}

/// Production request protection over the real staged core transaction.
///
/// It delegates to the single shared Pipeline algorithm introduced by P4a and performs only
/// validation against the staged token namespace. No throwaway session, replay, or whole-value
/// token shortcut exists here.
struct PipelineRequestPseudonymizer<'pipeline, 'transaction, 'session> {
    pipeline: &'pipeline Pipeline,
    transaction: &'transaction mut SessionTransaction<'session>,
    dictionaries: DictionaryBundle,
}

impl<'pipeline, 'transaction, 'session>
    PipelineRequestPseudonymizer<'pipeline, 'transaction, 'session>
{
    fn new(
        pipeline: &'pipeline Pipeline,
        transaction: &'transaction mut SessionTransaction<'session>,
    ) -> Self {
        Self {
            pipeline,
            transaction,
            dictionaries: DictionaryBundle::default(),
        }
    }

    fn protect_unstaged_segment(&mut self, input: &str) -> Result<String, CodecErrorCode> {
        let clean = self
            .pipeline
            .pseudonymize_transaction_with_detect_context(
                self.transaction,
                RawDocument::Text(input.to_owned()),
                &[LocaleTag::Global],
                &self.dictionaries,
            )
            .map_err(|_| CodecErrorCode::ProtectionFailedClosed)?;
        match clean {
            CleanDocument::Text(text) => Ok(text),
            _ => Err(CodecErrorCode::ProtectionFailedClosed),
        }
    }
}

impl RequestPseudonymizer for PipelineRequestPseudonymizer<'_, '_, '_> {
    fn protect(&mut self, input: &str) -> Result<String, CodecErrorCode> {
        self.validate_token_shapes(input)?;
        let mut tokens = self.transaction.tokens();
        if tokens.is_empty() {
            return self.protect_unstaged_segment(input);
        }
        tokens.sort_unstable_by_key(|token| std::cmp::Reverse(token.len()));

        let mut output = String::with_capacity(input.len());
        let mut cursor = 0;
        while cursor < input.len() {
            let next = tokens
                .iter()
                .filter_map(|token| {
                    input[cursor..]
                        .find(token)
                        .map(|offset| (cursor + offset, token.as_str()))
                })
                .min_by(|left, right| {
                    left.0
                        .cmp(&right.0)
                        .then_with(|| right.1.len().cmp(&left.1.len()))
                });
            let Some((start, token)) = next else {
                output.push_str(&self.protect_unstaged_segment(&input[cursor..])?);
                break;
            };
            if start > cursor {
                output.push_str(&self.protect_unstaged_segment(&input[cursor..start])?);
            }
            output.push_str(token);
            cursor = start + token.len();
        }
        Ok(output)
    }

    fn validate_token_shapes(&mut self, input: &str) -> Result<(), CodecErrorCode> {
        self.transaction
            .validate_token_shapes(input)
            .map_err(|_| CodecErrorCode::RestoreFailedClosed)
    }
}

struct PipelineResponseResidualValidator<'pipeline> {
    pipeline: &'pipeline Pipeline,
    validation_session: Session,
}

impl<'pipeline> PipelineResponseResidualValidator<'pipeline> {
    fn new(pipeline: &'pipeline Pipeline) -> Result<Self, DirectProxyError> {
        let validation_session = Session::new(Scope::Ephemeral).map_err(|_| {
            ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::ResponseValidation)
        })?;
        Ok(Self {
            pipeline,
            validation_session,
        })
    }
}

impl ResponseResidualValidator for PipelineResponseResidualValidator<'_> {
    fn validate(
        &mut self,
        value: &str,
        authorized_output_ranges: &[std::ops::Range<usize>],
    ) -> Result<(), CodecErrorCode> {
        let (_, spans, _) = self
            .pipeline
            .clean_with_safety_net(
                &self.validation_session,
                RawDocument::Text(value.to_owned()),
                &[LocaleTag::Global],
            )
            .map_err(|_| CodecErrorCode::ProviderOriginPii)?;
        for span in spans {
            if !authorized_output_ranges.iter().any(|authorized| {
                authorized.start <= span.raw_span.start && authorized.end >= span.raw_span.end
            }) {
                return Err(CodecErrorCode::ProviderOriginPii);
            }
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_and_commit_direct_request(
    pipeline: &Pipeline,
    session: &Session,
    codec: &dyn BodyCodec,
    original_body: &[u8],
    endpoint: &Url,
    headers: &HeaderMap,
    expected_response: WireFormat,
    limits: CodecLimits,
    provider_request_projection_selected: bool,
    max_request_bytes: usize,
) -> Result<CommittedDirectRequest, DirectProxyError> {
    prepare_and_commit_direct_request_with_hook(
        pipeline,
        session,
        codec,
        original_body,
        endpoint,
        headers,
        expected_response,
        limits,
        provider_request_projection_selected,
        max_request_bytes,
        |_, _| Ok(()),
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_and_commit_direct_request_with_hook(
    pipeline: &Pipeline,
    session: &Session,
    codec: &dyn BodyCodec,
    original_body: &[u8],
    endpoint: &Url,
    headers: &HeaderMap,
    expected_response: WireFormat,
    limits: CodecLimits,
    provider_request_projection_selected: bool,
    max_request_bytes: usize,
    mut before_commit: impl FnMut(u8, &Session) -> Result<(), DirectProxyError>,
) -> Result<CommittedDirectRequest, DirectProxyError> {
    for attempt in 0..=1 {
        let mut transaction = session.begin_transaction();
        let proved_body = {
            let mut pseudonymizer = PipelineRequestPseudonymizer::new(pipeline, &mut transaction);
            let mut context =
                RequestTransformContext::new(&mut pseudonymizer, WireFormat::Json, limits);
            if provider_request_projection_selected {
                context.request_inspection_projection();
            }
            codec
                .protect_request(original_body, &mut context)
                .map_err(map_codec_error)?
        };
        let request = DirectRequest::from_proved(
            endpoint.clone(),
            headers.clone(),
            proved_body,
            expected_response,
        )?;
        if request.body.len() > max_request_bytes {
            return Err(
                ProxyErrorCode::RequestBodyLimitExceeded.error(ProxyErrorPhase::RequestValidation)
            );
        }
        let prepared = PreparedDirectRequest::new(transaction, request);
        before_commit(attempt, session)?;
        match prepared.commit() {
            Ok(committed) => return Ok(committed),
            Err(SessionTransactionError::GenerationConflict) if attempt == 0 => {}
            Err(SessionTransactionError::GenerationConflict) => {
                return Err(
                    ProxyErrorCode::SessionGenerationConflict.error(ProxyErrorPhase::Session)
                );
            }
            Err(_) => {
                return Err(
                    ProxyErrorCode::SessionGenerationConflict.error(ProxyErrorPhase::Session)
                );
            }
        }
    }
    Err(ProxyErrorCode::SessionGenerationConflict.error(ProxyErrorPhase::Session))
}

/// Direct transaction state around the only possible upstream send.
#[derive(Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum DirectIoState {
    Prepared,
    CommittedNoIo,
    IoAttempted,
    ResponseComplete,
}

/// Enforces one generation retry and the Prepared-to-response I/O ordering.
#[derive(Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) struct DirectTransactionState {
    state: DirectIoState,
    generation_retries: u8,
}

#[allow(dead_code)]
impl DirectTransactionState {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            state: DirectIoState::Prepared,
            generation_retries: 0,
        }
    }

    #[must_use]
    pub(crate) const fn state(&self) -> &DirectIoState {
        &self.state
    }

    #[must_use]
    pub(crate) const fn generation_retries(&self) -> u8 {
        self.generation_retries
    }

    #[must_use]
    pub(crate) const fn retains_committed_mapping(&self) -> bool {
        !matches!(self.state, DirectIoState::Prepared)
    }

    pub(crate) fn retry_generation_conflict(&mut self) -> Result<(), DirectProxyError> {
        if self.state != DirectIoState::Prepared || self.generation_retries != 0 {
            return Err(ProxyErrorCode::SessionGenerationConflict.error(ProxyErrorPhase::Session));
        }
        self.generation_retries = 1;
        Ok(())
    }

    pub(crate) fn mark_committed(&mut self) -> Result<(), DirectProxyError> {
        self.transition(DirectIoState::Prepared, DirectIoState::CommittedNoIo)
    }

    pub(crate) fn mark_io_attempted(&mut self) -> Result<(), DirectProxyError> {
        self.transition(DirectIoState::CommittedNoIo, DirectIoState::IoAttempted)
    }

    pub(crate) fn mark_response_complete(&mut self) -> Result<(), DirectProxyError> {
        self.transition(DirectIoState::IoAttempted, DirectIoState::ResponseComplete)
    }

    fn transition(
        &mut self,
        expected: DirectIoState,
        next: DirectIoState,
    ) -> Result<(), DirectProxyError> {
        if self.state != expected {
            return Err(ProxyErrorCode::InvalidStateTransition.error(ProxyErrorPhase::Framework));
        }
        self.state = next;
        Ok(())
    }

    fn require_committed_no_io(&self) -> Result<(), DirectProxyError> {
        if self.state == DirectIoState::CommittedNoIo {
            Ok(())
        } else {
            Err(ProxyErrorCode::InvalidStateTransition.error(ProxyErrorPhase::Framework))
        }
    }
}

impl Default for DirectTransactionState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct AppState {
    config: Arc<ProxyConfig>,
    pipeline: Arc<Pipeline>,
    client: Client,
    direct: Option<Arc<DirectRuntime>>,
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
    let direct = DirectRuntime::from_config(&config)?.map(Arc::new);
    let state = AppState {
        config: Arc::new(config),
        pipeline,
        client: Client::new(),
        direct,
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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let direct_route_candidate =
        state.direct.is_some() && matches!(parts.uri.path(), "/v1/messages" | "/v1/messages/");
    let direct_ingress = if direct_route_candidate {
        match prepare_direct_ingress(&state, peer, &parts.method, &parts.uri, &parts.headers) {
            Ok(ingress) => Some(ingress),
            Err(error) => return direct_error_response(error),
        }
    } else {
        None
    };
    let is_direct = direct_ingress.is_some();
    let body_limit_bytes = if is_direct {
        state
            .direct
            .as_ref()
            .map_or(state.config.body_limit_bytes, |runtime| {
                state
                    .config
                    .body_limit_bytes
                    .min(runtime.client_config.max_request_bytes as u64)
            })
    } else {
        state.config.body_limit_bytes
    };
    let body = match collect_inbound_body(body, &parts.headers, body_limit_bytes).await {
        Ok(body) => body,
        Err(_) if is_direct => {
            return direct_error_response(
                ProxyErrorCode::RequestBodyLimitExceeded.error(ProxyErrorPhase::RequestValidation),
            )
        }
        Err(error) => return proxy_error_response(error),
    };
    match proxy_inner(
        state,
        parts.method,
        parts.uri,
        parts.headers,
        body,
        direct_ingress,
    )
    .await
    {
        Ok(response) => response,
        Err(err) => proxy_error_response(err),
    }
}

async fn collect_inbound_body(
    body: Body,
    headers: &HeaderMap,
    limit_bytes: u64,
) -> Result<Bytes, ProxyError> {
    if let Some(length) = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| std::str::from_utf8(value.as_bytes()).ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if length > limit_bytes {
            return Err(ProxyError::BodyTooLarge { limit_bytes });
        }
    }

    let mut stream = body.into_data_stream();
    let mut collected = Vec::new();
    let mut accumulated = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ProxyError::DaemonConfig {
            detail: "request_body_rejected".to_string(),
        })?;
        accumulated = accumulated
            .checked_add(chunk.len() as u64)
            .ok_or(ProxyError::BodyTooLarge { limit_bytes })?;
        if accumulated > limit_bytes {
            return Err(ProxyError::BodyTooLarge { limit_bytes });
        }
        collected.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(collected))
}

async fn proxy_inner(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    direct_ingress: Option<PreparedDirectIngress>,
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
    let contract = adapter.contract();
    if let ProtocolContract::Codec(codec) = contract.protocol() {
        return Ok(
            match direct_proxy_inner(
                &state,
                direct_ingress.ok_or_else(|| ProxyError::DaemonConfig {
                    detail: "direct_ingress_missing".to_string(),
                })?,
                &body,
                codec,
            )
            .await
            {
                Ok(response) => response,
                Err(error) => direct_error_response(error),
            },
        );
    }
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

fn prepare_direct_ingress(
    state: &AppState,
    peer: SocketAddr,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
) -> Result<PreparedDirectIngress, DirectProxyError> {
    let runtime = state
        .direct
        .as_ref()
        .ok_or_else(|| ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::Framework))?;
    if method != Method::POST || uri.path() != "/v1/messages" || uri.query().is_some() {
        return Err(ProxyErrorCode::RouteRejected.error(ProxyErrorPhase::Ingress));
    }
    let outbound_headers = validate_direct_request_headers(headers, runtime)?;
    let session = match runtime.session_policy {
        SessionPolicy::EphemeralSingleRequest => {
            if headers.get_all(SESSION_HEADER).iter().next().is_some() {
                return Err(ProxyErrorCode::UnexpectedSessionHeader
                    .error(ProxyErrorPhase::RequestValidation));
            }
            DirectIngressSession::Ephemeral
        }
        SessionPolicy::OpaqueHeaderContinuity(_) => {
            let identifier = canonical_session_identifier(headers)?;
            let peer_scope = if peer.ip().is_loopback() {
                PeerScope::Loopback
            } else {
                PeerScope::NonLoopback
            };
            let context = PrincipalContext::new(runtime.listener_scope, peer_scope, None, None)
                .map_err(|_| ProxyErrorCode::PrincipalRejected.error(ProxyErrorPhase::Session))?;
            let principal = invoke_resolver(runtime.resolver.as_ref(), context)
                .map_err(|_| ProxyErrorCode::PrincipalRejected.error(ProxyErrorPhase::Session))?;
            let registry = runtime.registry.as_ref().ok_or_else(|| {
                ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::Session)
            })?;
            let lease = registry
                .acquire(principal, identifier, None)
                .map_err(map_registry_error)?;
            DirectIngressSession::Continuity(lease)
        }
    };
    Ok(PreparedDirectIngress {
        outbound_headers,
        session,
    })
}

async fn direct_proxy_inner(
    state: &AppState,
    ingress: PreparedDirectIngress,
    body: &[u8],
    codec: &dyn BodyCodec,
) -> Result<Response, DirectProxyError> {
    let runtime = state
        .direct
        .as_ref()
        .ok_or_else(|| ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::Framework))?;
    if body.len() > runtime.client_config.max_request_bytes {
        return Err(
            ProxyErrorCode::RequestBodyLimitExceeded.error(ProxyErrorPhase::RequestValidation)
        );
    }
    let expected_response = parse_request_stream_format(body)?;
    let mut inspection = state
        .config
        .inspection
        .as_ref()
        .and_then(|producer| producer.begin_logical(runtime.inspection_endpoint_codes));

    let mut committed = match ingress.session {
        DirectIngressSession::Ephemeral => {
            let session = Session::new(Scope::Ephemeral)
                .map_err(|_| ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::Session))?;
            prepare_and_commit_direct_request(
                &state.pipeline,
                &session,
                codec,
                body,
                &runtime.endpoint,
                &ingress.outbound_headers,
                expected_response,
                runtime.codec_limits,
                inspection
                    .as_ref()
                    .is_some_and(ProxyInspectionLogicalV1::provider_request_projection_selected),
                runtime.client_config.max_request_bytes,
            )?
        }
        DirectIngressSession::Continuity(lease) => {
            let request_guard = lease.lock_request().await;
            let committed = prepare_and_commit_direct_request(
                &state.pipeline,
                lease.session(),
                codec,
                body,
                &runtime.endpoint,
                &ingress.outbound_headers,
                expected_response,
                runtime.codec_limits,
                inspection
                    .as_ref()
                    .is_some_and(ProxyInspectionLogicalV1::provider_request_projection_selected),
                runtime.client_config.max_request_bytes,
            );
            drop(request_guard);
            committed?
        }
    };

    if let Some(inspection) = inspection.as_mut() {
        let inspection_parsed_facts = committed.request.inspection_parsed_facts.take();
        inspection.emit_request_stages(
            body,
            &committed.request.body,
            inspection_parsed_facts,
            committed.request.signed_or_encrypted_surface,
        );
    }

    if expected_response == WireFormat::Sse {
        let opened = runtime.client.execute_sse(committed).await?;
        return stage_and_replay_direct_sse(
            Arc::clone(&state.pipeline),
            opened,
            runtime.codec_limits,
            runtime.client_config.max_response_bytes,
            runtime.ping_interval,
            inspection,
        );
    }
    let executed = runtime.client.execute(committed).await?;
    restore_direct_response(
        &state.pipeline,
        codec,
        executed,
        runtime.codec_limits,
        inspection.as_mut(),
    )
}

fn stage_and_replay_direct_sse(
    pipeline: Arc<Pipeline>,
    opened: CommittedDirectSse,
    limits: CodecLimits,
    max_response_bytes: usize,
    ping_interval: Duration,
    inspection: Option<ProxyInspectionLogicalV1>,
) -> Result<Response, DirectProxyError> {
    let CommittedDirectSse {
        snapshot,
        response: upstream_response,
        head,
        lifecycle,
        remaining_total_timeout,
    } = opened;
    let status = head.status();
    let headers = head.headers().clone();
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(8);
    tokio::spawn(async move {
        let mut inspection = inspection;
        let proof = async move {
            let body = collect_response_body(upstream_response, max_response_bytes).await?;
            validate_declared_body(WireFormat::Sse, &body, max_response_bytes)?;
            let mut residual = PipelineResponseResidualValidator::new(&pipeline)?;
            let mut context =
                ResponseTransformContext::new(&snapshot, &mut residual, WireFormat::Sse, limits);
            if inspection
                .as_ref()
                .is_some_and(ProxyInspectionLogicalV1::owner_restored_response_projection_selected)
            {
                context.request_inspection_projection();
            }
            let mut proved = crate::codecs::anthropic::AnthropicMessagesCodec
                .restore_response(&body, &mut context)
                .map_err(map_codec_error)?;
            if let Some(inspection) = inspection.as_mut() {
                let format = proved.format();
                let signed_or_encrypted_surface = proved.signed_or_encrypted_surface();
                let inspection_parsed_facts = proved.take_inspection_parsed_facts();
                inspection.emit_response_stages(
                    &body,
                    proved.bytes(),
                    format,
                    inspection_parsed_facts,
                    signed_or_encrypted_surface,
                );
            }
            let mut lifecycle = lifecycle;
            lifecycle.mark_response_complete()?;
            Ok::<Vec<u8>, DirectProxyError>(proved.into_bytes())
        };
        let timed_proof = tokio::time::timeout(remaining_total_timeout, proof);
        tokio::pin!(timed_proof);
        let mut ping = tokio::time::interval(ping_interval);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping.tick().await;
        loop {
            tokio::select! {
                () = sender.closed() => {
                    break;
                }
                result = &mut timed_proof => {
                    let output = match result {
                        Ok(Ok(bytes)) => Bytes::from(bytes),
                        Ok(Err(_)) | Err(_) => Bytes::from_static(DIRECT_PROXY_ERROR_FRAME),
                    };
                    let _ = sender.send(Ok(output)).await;
                    break;
                }
                _ = ping.tick() => {
                    if sender
                        .send(Ok(Bytes::from_static(DIRECT_PROXY_PING_FRAME)))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    let mut response = Response::builder().status(status);
    for (name, value) in &headers {
        response = response.header(name, value);
    }
    response
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(receiver),
        ))
        .map_err(|_| ProxyErrorCode::UpstreamProtocol.error(ProxyErrorPhase::Framework))
}

fn validate_direct_request_headers(
    headers: &HeaderMap,
    runtime: &DirectRuntime,
) -> Result<HeaderMap, DirectProxyError> {
    validate_header_space(headers, runtime.client_config)?;
    for singleton in [
        "content-type",
        "content-length",
        "transfer-encoding",
        "content-encoding",
        "connection",
        "x-api-key",
        "anthropic-version",
        "anthropic-beta",
        SESSION_HEADER,
    ] {
        reject_duplicate_request_header(headers, singleton)?;
    }
    if headers.get(axum::http::header::CONTENT_ENCODING).is_some() {
        return Err(
            ProxyErrorCode::UnsupportedContentEncoding.error(ProxyErrorPhase::RequestValidation)
        );
    }
    if let Some(connection) = one_request_header(headers, "connection", false)? {
        let connection = connection.to_str().map_err(|_| {
            ProxyErrorCode::InvalidFraming.error(ProxyErrorPhase::RequestValidation)
        })?;
        if !connection.eq_ignore_ascii_case("keep-alive")
            && !connection.eq_ignore_ascii_case("close")
        {
            return Err(ProxyErrorCode::InvalidFraming.error(ProxyErrorPhase::RequestValidation));
        }
    }
    if headers.get(axum::http::header::TRANSFER_ENCODING).is_some()
        && headers.get(axum::http::header::CONTENT_LENGTH).is_some()
    {
        return Err(ProxyErrorCode::InvalidFraming.error(ProxyErrorPhase::RequestValidation));
    }
    let content_type = one_request_header(headers, "content-type", true)?
        .ok_or_else(|| ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation))?;
    if parse_wire_content_type(content_type)? != WireFormat::Json {
        return Err(ProxyErrorCode::InvalidRequestFormat.error(ProxyErrorPhase::RequestValidation));
    }
    let api_key = one_request_header(headers, "x-api-key", true)?
        .ok_or_else(|| ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation))?;
    if !api_key
        .as_bytes()
        .iter()
        .all(|byte| (0x21..=0x7e).contains(byte) && *byte != b',')
    {
        return Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation));
    }
    let version = one_request_header(headers, "anthropic-version", true)?
        .ok_or_else(|| ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation))?;
    let version_text = std::str::from_utf8(version.as_bytes())
        .map_err(|_| ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation))?;
    if !runtime
        .allowed_versions
        .iter()
        .any(|allowed| allowed == version_text)
    {
        return Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation));
    }
    let beta = one_request_header(headers, "anthropic-beta", false)?;
    if let Some(beta) = beta {
        let beta_text = std::str::from_utf8(beta.as_bytes()).map_err(|_| {
            ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation)
        })?;
        if !runtime
            .allowed_betas
            .iter()
            .any(|allowed| allowed == beta_text)
        {
            return Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation));
        }
    }

    let mut outbound = HeaderMap::new();
    outbound.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    outbound.insert(HeaderName::from_static("x-api-key"), api_key.clone());
    outbound.insert(
        HeaderName::from_static("anthropic-version"),
        version.clone(),
    );
    if let Some(beta) = beta {
        outbound.insert(HeaderName::from_static("anthropic-beta"), beta.clone());
    }
    Ok(outbound)
}

fn one_request_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
    required: bool,
) -> Result<Option<&'a HeaderValue>, DirectProxyError> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() || first.is_some_and(|value| value.is_empty()) {
        return Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation));
    }
    match (first, required) {
        (Some(value), _) => Ok(Some(value)),
        (None, false) => Ok(None),
        (None, true) => {
            Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation))
        }
    }
}

fn reject_duplicate_request_header(
    headers: &HeaderMap,
    name: &'static str,
) -> Result<(), DirectProxyError> {
    let mut values = headers.get_all(name).iter();
    let _ = values.next();
    if values.next().is_some() {
        Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation))
    } else {
        Ok(())
    }
}

fn canonical_session_identifier(headers: &HeaderMap) -> Result<&str, DirectProxyError> {
    let value = one_request_header(headers, SESSION_HEADER, false)?
        .ok_or_else(|| {
            ProxyErrorCode::SessionIdentityRequired.error(ProxyErrorPhase::RequestValidation)
        })?
        .to_str()
        .map_err(|_| ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation))?;
    let identifier = uuid::Uuid::parse_str(value)
        .map_err(|_| ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation))?;
    if identifier.get_version() != Some(uuid::Version::Random)
        || identifier.get_variant() != uuid::Variant::RFC4122
        || identifier.hyphenated().to_string() != value
    {
        return Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::RequestValidation));
    }
    Ok(value)
}

fn restore_direct_response(
    pipeline: &Pipeline,
    codec: &dyn BodyCodec,
    mut executed: CommittedDirectResponse,
    limits: CodecLimits,
    inspection: Option<&mut ProxyInspectionLogicalV1>,
) -> Result<Response, DirectProxyError> {
    let (head, body) = executed.response.into_parts();
    let mut residual = PipelineResponseResidualValidator::new(pipeline)?;
    let mut context =
        ResponseTransformContext::new(&executed.snapshot, &mut residual, head.format(), limits);
    if inspection
        .as_deref()
        .is_some_and(ProxyInspectionLogicalV1::owner_restored_response_projection_selected)
    {
        context.request_inspection_projection();
    }
    let mut proved = codec
        .restore_response(&body, &mut context)
        .map_err(map_codec_error)?;
    if let Some(inspection) = inspection {
        let format = proved.format();
        let signed_or_encrypted_surface = proved.signed_or_encrypted_surface();
        let inspection_parsed_facts = proved.take_inspection_parsed_facts();
        inspection.emit_response_stages(
            &body,
            proved.bytes(),
            format,
            inspection_parsed_facts,
            signed_or_encrypted_surface,
        );
    }
    executed.lifecycle.mark_response_complete()?;
    let mut response = Response::builder().status(head.status());
    for (name, value) in head.headers() {
        response = response.header(name, value);
    }
    response
        .body(Body::from(proved.into_bytes()))
        .map_err(|_| ProxyErrorCode::UpstreamProtocol.error(ProxyErrorPhase::Framework))
}

fn map_registry_error(error: RegistryError) -> DirectProxyError {
    let code = match error {
        RegistryError::InvalidSessionIdentifier => ProxyErrorCode::HeaderRejected,
        RegistryError::SessionExpired => ProxyErrorCode::SessionExpired,
        RegistryError::SessionGenerationConflict => ProxyErrorCode::SessionGenerationConflict,
        RegistryError::ActiveCapacityReached => ProxyErrorCode::RegistryCapacity,
        RegistryError::EntropyUnavailable
        | RegistryError::SessionCreationFailed
        | RegistryError::EpochExhausted
        | RegistryError::InternalFailure => ProxyErrorCode::ProxyConfiguration,
    };
    code.error(ProxyErrorPhase::Session)
}

fn map_codec_error(error: CodecError) -> DirectProxyError {
    let code = match error.code() {
        CodecErrorCode::DuplicateObjectKey => ProxyErrorCode::DuplicateObjectKey,
        CodecErrorCode::InternalCoverageFailure => ProxyErrorCode::InternalCoverageFailure,
        CodecErrorCode::ControlWouldMutate => ProxyErrorCode::ControlWouldMutate,
        CodecErrorCode::OpaqueMediaUninspected => ProxyErrorCode::OpaqueMediaUninspected,
        CodecErrorCode::SignedMutationRequired => ProxyErrorCode::SignedMutationRequired,
        CodecErrorCode::SignedSurfaceMalformed => ProxyErrorCode::SignedSurfaceMalformed,
        CodecErrorCode::InvalidSseLifecycle | CodecErrorCode::IncompleteSseStream => {
            ProxyErrorCode::InvalidSseLifecycle
        }
        CodecErrorCode::InvalidContentBlockIndex | CodecErrorCode::InvalidContentBlockDelta => {
            ProxyErrorCode::InvalidContentBlockIndex
        }
        CodecErrorCode::DepthLimitExceeded
        | CodecErrorCode::NodeLimitExceeded
        | CodecErrorCode::StringLimitExceeded
        | CodecErrorCode::NumberLimitExceeded
        | CodecErrorCode::BodyLimitExceeded
        | CodecErrorCode::GrowthLimitExceeded
        | CodecErrorCode::SseLineLimitExceeded
        | CodecErrorCode::SseFrameLimitExceeded
        | CodecErrorCode::SseEventLimitExceeded
        | CodecErrorCode::SseIndexLimitExceeded
        | CodecErrorCode::SseAccumulatorLimitExceeded => ProxyErrorCode::ResponseBodyLimitExceeded,
        CodecErrorCode::ProtectionFailedClosed
        | CodecErrorCode::RestoreFailedClosed
        | CodecErrorCode::ProviderOriginPii
        | CodecErrorCode::ProviderOriginNumeric
        | CodecErrorCode::TransformedKeyCollision
        | CodecErrorCode::InvalidUtf8
        | CodecErrorCode::InvalidJson
        | CodecErrorCode::InvalidWireFormat
        | CodecErrorCode::UnsupportedRequestSurface
        | CodecErrorCode::UnsupportedResponseSurface
        | CodecErrorCode::InvalidSseField
        | CodecErrorCode::InvalidSseFrame
        | CodecErrorCode::InvalidSseEventPair
        | CodecErrorCode::UnsafeFutureEvent
        | CodecErrorCode::ProviderErrorEvent => ProxyErrorCode::InvalidToken,
    };
    let phase = match error.phase() {
        CodecPhase::RequestParse => ProxyErrorPhase::RequestValidation,
        CodecPhase::RequestProtection | CodecPhase::RequestProof => {
            ProxyErrorPhase::RequestTransform
        }
        CodecPhase::ResponseParse => ProxyErrorPhase::ResponseValidation,
        CodecPhase::ResponseRestore | CodecPhase::ResponseProof => {
            ProxyErrorPhase::ResponseTransform
        }
        CodecPhase::SseDecode | CodecPhase::SseLifecycle => ProxyErrorPhase::ResponseValidation,
        CodecPhase::SseReplay => ProxyErrorPhase::ResponseReplay,
    };
    code.error(phase)
}

fn direct_error_response(error: DirectProxyError) -> Response {
    let body = serde_json::json!({
        "type": "error",
        "error": {
            "type": "api_error",
            "message": "proxy_validation_failed",
            "code": format!("{:?}", error.code()),
        }
    });
    let mut response = Response::builder()
        .status(error.http_status())
        .header(axum::http::header::CONTENT_TYPE, "application/json");
    if let Some(seconds) = error.retry_after_seconds() {
        response = response.header(axum::http::header::RETRY_AFTER, seconds.to_string());
    }
    response
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
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
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use axum::body::to_bytes;
    use axum::routing::post;
    use gaze::{Action, ClassRule, DefaultRule, PiiClass};
    use gaze_recognizers::RegexDetector;

    use crate::codec::OutputProvenance;

    const SYNTHETIC_EMAIL: &str = "alice@example.invalid";
    const SYNTHETIC_PHONE: &str = "+1-555-0199";

    struct SyntheticPrincipalResolver;

    impl PrincipalResolver for SyntheticPrincipalResolver {
        fn resolve(
            &self,
            _context: PrincipalContext<'_>,
        ) -> Result<crate::AuthenticatedPrincipal, crate::principal::PrincipalResolveError>
        {
            crate::AuthenticatedPrincipal::try_from_canonical_bytes(b"synthetic-principal".to_vec())
        }
    }

    #[derive(Clone, Default)]
    struct Capture {
        hits: Arc<AtomicUsize>,
        headers: Arc<Mutex<Option<HeaderMap>>>,
        body: Arc<Mutex<Vec<u8>>>,
    }

    async fn capture_ok(
        State(capture): State<Capture>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        capture.hits.fetch_add(1, Ordering::SeqCst);
        *capture.headers.lock().unwrap() = Some(headers);
        *capture.body.lock().unwrap() = body.to_vec();
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("set-cookie", "synthetic=drop")
            .header("x-request-id", "drop-me")
            .body(Body::from(br#"{"ok":true}"#.as_slice()))
            .unwrap()
    }

    async fn spawn_test_server(app: Router) -> (Url, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            Url::parse(&format!("http://{address}/v1/messages")).unwrap(),
            handle,
        )
    }

    fn proved(bytes: &[u8]) -> ProvedRequestBody {
        let mut provenance = OutputProvenance::default();
        provenance.mark_final_buffer_verified();
        ProvedRequestBody::new(bytes.to_vec(), WireFormat::Json, provenance)
    }

    fn direct_request(
        url: Url,
        expected: WireFormat,
        bytes: &[u8],
    ) -> Result<DirectRequest, DirectProxyError> {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", HeaderValue::from_static("synthetic-key"));
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        DirectRequest::from_proved(url, headers, proved(bytes), expected)
    }

    fn committed_direct_request(
        url: Url,
        expected: WireFormat,
        bytes: &[u8],
    ) -> CommittedDirectRequest {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let transaction = session.begin_transaction();
        PreparedDirectRequest::new(transaction, direct_request(url, expected, bytes).unwrap())
            .commit()
            .unwrap()
    }

    fn email_pipeline() -> Pipeline {
        Pipeline::builder()
            .detector(RegexDetector::emails().unwrap())
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .unwrap()
    }

    #[test]
    fn non_loopback_readiness_requires_an_explicit_principal_resolver() {
        let bind = "0.0.0.0:8787".parse().unwrap();
        let upstream = Url::parse("https://api.anthropic.com").unwrap();
        let missing = ProxyConfig::anthropic_direct(bind, AnthropicAdapter::new(upstream.clone()));
        assert!(DirectRuntime::from_config(&missing).is_err());

        let configured = ProxyConfig::anthropic_direct(
            bind,
            AnthropicAdapter::builder(upstream)
                .principal_resolver(Arc::new(SyntheticPrincipalResolver))
                .build()
                .unwrap(),
        );
        assert!(DirectRuntime::from_config(&configured).unwrap().is_some());
    }

    #[test]
    fn invalid_direct_upstream_fails_readiness_before_any_request() {
        let config = ProxyConfig::anthropic_direct(
            "127.0.0.1:8787".parse().unwrap(),
            AnthropicAdapter::new(Url::parse("https://synthetic:credential@127.0.0.1").unwrap()),
        );
        assert!(DirectRuntime::from_config(&config).is_err());
    }

    #[tokio::test]
    async fn direct_send_requires_commit_is_single_use_and_captures_exact_proof() {
        let capture = Capture::default();
        let (url, server) = spawn_test_server(
            Router::new()
                .route("/v1/messages", post(capture_ok))
                .with_state(capture.clone()),
        )
        .await;
        let exact = br#"{"messages":[{"role":"user","content":"<Email_1>"}]}"#;
        let client = DirectClient::new(DirectClientConfig::default()).unwrap();

        let session = Session::new(Scope::Ephemeral).unwrap();
        let prepared = PreparedDirectRequest::new(
            session.begin_transaction(),
            direct_request(url.clone(), WireFormat::Json, exact).unwrap(),
        );
        drop(prepared);
        assert!(session.tokens().is_empty());
        assert_eq!(capture.hits.load(Ordering::SeqCst), 0);

        let mut lifecycle = DirectTransactionState::new();
        lifecycle.retry_generation_conflict().unwrap();
        assert_eq!(lifecycle.generation_retries(), 1);
        assert_eq!(
            lifecycle.retry_generation_conflict().unwrap_err().code(),
            ProxyErrorCode::SessionGenerationConflict
        );
        let response = client
            .execute(committed_direct_request(url, WireFormat::Json, exact))
            .await
            .unwrap();
        assert_eq!(response.lifecycle.state(), &DirectIoState::IoAttempted);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 1);
        assert_eq!(&*capture.body.lock().unwrap(), exact);
        assert_eq!(response.response.body(), br#"{"ok":true}"#);
        assert_eq!(response.response.headers().len(), 1);
        assert!(response.response.headers().get("set-cookie").is_none());
        server.abort();
    }

    #[test]
    fn signed_shadow_probe_reuses_only_the_previously_staged_mapping() {
        let pipeline = Pipeline::builder()
            .detector(RegexDetector::emails().unwrap())
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .enable_prefix_cache()
            .build()
            .unwrap();
        let codec = crate::codecs::anthropic::AnthropicMessagesCodec;
        let session = Session::new(Scope::Ephemeral).unwrap();
        let expected_token = format!("<{}:Email_1>", session.session_hex());
        let body = format!(
            r#"{{"model":"claude-test","max_tokens":32,"system":"{SYNTHETIC_EMAIL}","messages":[{{"role":"assistant","content":[{{"type":"thinking","thinking":"{expected_token}","signature":"opaque-signature"}}]}}]}}"#
        );
        let request = direct_request(
            Url::parse("https://api.anthropic.com/v1/messages").unwrap(),
            WireFormat::Json,
            body.as_bytes(),
        )
        .unwrap();

        let committed = prepare_and_commit_direct_request(
            &pipeline,
            &session,
            &codec,
            body.as_bytes(),
            &request.url,
            &request.headers,
            WireFormat::Json,
            CodecLimits::default(),
            false,
            DEFAULT_MAX_REQUEST_BYTES,
        )
        .unwrap();

        assert_eq!(session.tokens(), vec![expected_token.clone()]);
        let protected = std::str::from_utf8(&committed.request.body).unwrap();
        assert!(!protected.contains(SYNTHETIC_EMAIL));
        assert_eq!(protected.matches(&expected_token).count(), 2);
    }

    #[test]
    fn final_request_reproof_is_idempotent_for_ordinary_and_format_preserving_tokens() {
        let endpoint = Url::parse("https://api.anthropic.com/v1/messages").unwrap();
        let codec = crate::codecs::anthropic::AnthropicMessagesCodec;

        for action in [Action::Tokenize, Action::FormatPreserve] {
            let pipeline = Pipeline::builder()
                .detector(RegexDetector::emails().unwrap())
                .rule(ClassRule::new(PiiClass::Email, action))
                .rule(DefaultRule::new(Action::Preserve))
                .build()
                .unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let original = format!(
                r#"{{"model":"claude-test","max_tokens":32,"messages":[{{"role":"user","content":"{SYNTHETIC_EMAIL}"}}]}}"#
            );
            let headers = direct_request(endpoint.clone(), WireFormat::Json, original.as_bytes())
                .unwrap()
                .headers;
            let first = prepare_and_commit_direct_request(
                &pipeline,
                &session,
                &codec,
                original.as_bytes(),
                &endpoint,
                &headers,
                WireFormat::Json,
                CodecLimits::default(),
                false,
                DEFAULT_MAX_REQUEST_BYTES,
            )
            .unwrap_or_else(|error| panic!("{action:?} first proof failed: {error:?}"));
            let proved_once = first.request.body.clone();
            assert_eq!(session.tokens().len(), 1);

            let second = prepare_and_commit_direct_request(
                &pipeline,
                &session,
                &codec,
                &proved_once,
                &endpoint,
                &headers,
                WireFormat::Json,
                CodecLimits::default(),
                false,
                DEFAULT_MAX_REQUEST_BYTES,
            )
            .unwrap();
            assert_eq!(second.request.body, proved_once);
            assert_eq!(session.tokens().len(), 1);
        }
    }

    #[tokio::test]
    async fn late_signed_failure_discards_all_staged_state_and_does_zero_io() {
        let capture = Capture::default();
        let (url, server) = spawn_test_server(
            Router::new()
                .route("/v1/messages", post(capture_ok))
                .with_state(capture.clone()),
        )
        .await;
        let pipeline = Pipeline::builder()
            .detector(RegexDetector::emails().unwrap())
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .enable_prefix_cache()
            .build()
            .unwrap();
        let codec = crate::codecs::anthropic::AnthropicMessagesCodec;
        let session = Session::new(Scope::Ephemeral).unwrap();
        let body = format!(
            r#"{{"model":"claude-test","max_tokens":32,"system":"{SYNTHETIC_EMAIL}","messages":[{{"role":"assistant","content":[{{"type":"thinking","thinking":"{SYNTHETIC_EMAIL}","signature":"opaque-signature"}}]}}]}}"#
        );
        let request = direct_request(url, WireFormat::Json, body.as_bytes()).unwrap();
        let error = prepare_and_commit_direct_request(
            &pipeline,
            &session,
            &codec,
            body.as_bytes(),
            &request.url,
            &request.headers,
            WireFormat::Json,
            CodecLimits::default(),
            false,
            DEFAULT_MAX_REQUEST_BYTES,
        )
        .unwrap_err();

        assert_eq!(error.code(), ProxyErrorCode::SignedMutationRequired);
        assert!(session.tokens().is_empty());
        assert_eq!(capture.hits.load(Ordering::SeqCst), 0);

        let valid = format!(
            r#"{{"model":"claude-test","max_tokens":32,"system":"{SYNTHETIC_EMAIL} reports","messages":[{{"role":"user","content":"synthetic"}}]}}"#
        );
        prepare_and_commit_direct_request(
            &pipeline,
            &session,
            &codec,
            valid.as_bytes(),
            &request.url,
            &request.headers,
            WireFormat::Json,
            CodecLimits::default(),
            false,
            DEFAULT_MAX_REQUEST_BYTES,
        )
        .unwrap();
        assert_eq!(session.tokens().len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn generation_conflict_reproofs_once_and_sends_only_the_winning_body() {
        let capture = Capture::default();
        let (url, server) = spawn_test_server(
            Router::new()
                .route("/v1/messages", post(capture_ok))
                .with_state(capture.clone()),
        )
        .await;
        let pipeline = email_pipeline();
        let codec = crate::codecs::anthropic::AnthropicMessagesCodec;
        let session = Session::new(Scope::Ephemeral).unwrap();
        let body = format!(
            r#"{{"model":"claude-test","max_tokens":32,"messages":[{{"role":"user","content":"{SYNTHETIC_EMAIL}"}}]}}"#
        );
        let request = direct_request(url.clone(), WireFormat::Json, body.as_bytes()).unwrap();
        let mut hook_calls = 0;
        let committed = prepare_and_commit_direct_request_with_hook(
            &pipeline,
            &session,
            &codec,
            body.as_bytes(),
            &request.url,
            &request.headers,
            WireFormat::Json,
            CodecLimits::default(),
            false,
            DEFAULT_MAX_REQUEST_BYTES,
            |attempt, live| {
                hook_calls += 1;
                if attempt == 0 {
                    live.tokenize(&PiiClass::Name, "Dr. Schmidt").unwrap();
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(hook_calls, 2);
        assert_eq!(session.tokens().len(), 2);

        DirectClient::new(DirectClientConfig::default())
            .unwrap()
            .execute(committed)
            .await
            .unwrap();
        assert_eq!(capture.hits.load(Ordering::SeqCst), 1);
        let captured = String::from_utf8(capture.body.lock().unwrap().clone()).unwrap();
        assert!(!captured.contains(SYNTHETIC_EMAIL));
        assert_eq!(captured.matches(":Email_1>").count(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn second_generation_conflict_fails_closed_without_upstream_io() {
        let capture = Capture::default();
        let (url, server) = spawn_test_server(
            Router::new()
                .route("/v1/messages", post(capture_ok))
                .with_state(capture.clone()),
        )
        .await;
        let pipeline = email_pipeline();
        let codec = crate::codecs::anthropic::AnthropicMessagesCodec;
        let session = Session::new(Scope::Ephemeral).unwrap();
        let body = format!(
            r#"{{"model":"claude-test","max_tokens":32,"messages":[{{"role":"user","content":"{SYNTHETIC_EMAIL}"}}]}}"#
        );
        let request = direct_request(url, WireFormat::Json, body.as_bytes()).unwrap();
        let error = prepare_and_commit_direct_request_with_hook(
            &pipeline,
            &session,
            &codec,
            body.as_bytes(),
            &request.url,
            &request.headers,
            WireFormat::Json,
            CodecLimits::default(),
            false,
            DEFAULT_MAX_REQUEST_BYTES,
            |attempt, live| {
                live.tokenize(&PiiClass::Name, &format!("Dr. Schmidt {attempt}"))
                    .unwrap();
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::SessionGenerationConflict);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 0);
        assert!(session
            .tokens()
            .iter()
            .all(|token| !token.contains(":Email_")));
        server.abort();
    }

    #[tokio::test]
    async fn cancellation_publishes_only_after_commit_and_never_rolls_it_back() {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut staged = session.begin_transaction();
        staged.tokenize(&PiiClass::Email, SYNTHETIC_EMAIL).unwrap();
        let prepared = PreparedDirectRequest::new(
            staged,
            direct_request(
                Url::parse("https://api.anthropic.com/v1/messages").unwrap(),
                WireFormat::Json,
                br#"{}"#,
            )
            .unwrap(),
        );
        drop(prepared);
        assert!(session.tokens().is_empty());

        let (url, server) = spawn_test_server(Router::new().route(
            "/v1/messages",
            post(|| async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                ([("content-type", "application/json")], "{}")
            }),
        ))
        .await;
        let mut staged = session.begin_transaction();
        staged.tokenize(&PiiClass::Email, SYNTHETIC_EMAIL).unwrap();
        let committed = PreparedDirectRequest::new(
            staged,
            direct_request(url, WireFormat::Json, br#"{}"#).unwrap(),
        )
        .commit()
        .unwrap();
        assert_eq!(session.tokens().len(), 1);
        let client = DirectClient::new(DirectClientConfig::default()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), client.execute(committed))
                .await
                .is_err()
        );
        assert_eq!(session.tokens().len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn response_restore_uses_the_exact_committed_snapshot_after_live_mutation() {
        let pipeline = email_pipeline();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut transaction = session.begin_transaction();
        let committed_token = transaction
            .tokenize(&PiiClass::Email, SYNTHETIC_EMAIL)
            .unwrap();
        let snapshot = transaction.commit().unwrap();
        session.tokenize(&PiiClass::Name, "Dr. Schmidt").unwrap();
        assert_eq!(session.tokens().len(), 2);

        let body = format!(
            r#"{{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{{"type":"text","text":"{committed_token}"}}],"stop_reason":"end_turn","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let executed = CommittedDirectResponse {
            snapshot,
            response: DirectResponse {
                head: DirectResponseHead {
                    status: StatusCode::OK,
                    format: WireFormat::Json,
                    headers,
                },
                body: body.into_bytes(),
            },
            lifecycle: DirectTransactionState {
                state: DirectIoState::IoAttempted,
                generation_retries: 0,
            },
        };
        let response = restore_direct_response(
            &pipeline,
            &crate::codecs::anthropic::AnthropicMessagesCodec,
            executed,
            CodecLimits::default(),
            None,
        )
        .unwrap();
        let restored = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert!(std::str::from_utf8(&restored)
            .unwrap()
            .contains(SYNTHETIC_EMAIL));
        assert!(!std::str::from_utf8(&restored)
            .unwrap()
            .contains("Dr. Schmidt"));
    }

    #[tokio::test]
    async fn transformed_request_overlimit_rejects_before_commit_and_zero_io() {
        const SESSION_ID: &str = "00000000-0000-4000-8000-000000000001";

        let capture = Capture::default();
        let (url, server) = spawn_test_server(
            Router::new()
                .route("/v1/messages", post(capture_ok))
                .with_state(capture.clone()),
        )
        .await;
        let phone_class = PiiClass::Custom("phone".to_string());
        let pipeline = Pipeline::builder()
            .detector(RegexDetector::new(r"\+1-555-0199", phone_class.clone()).unwrap())
            .rule(ClassRule::new(phone_class, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .enable_prefix_cache()
            .build()
            .unwrap();
        let codec = crate::codecs::anthropic::AnthropicMessagesCodec;
        let body = format!(
            r#"{{"model":"claude-test","max_tokens":32,"messages":[{{"role":"user","content":"{SYNTHETIC_PHONE}"}}]}}"#
        );
        let request_limit = body.len() + 1;
        let probe_session = Session::new(Scope::Ephemeral).unwrap();
        let probe_request = direct_request(url.clone(), WireFormat::Json, body.as_bytes()).unwrap();
        let transformed = prepare_and_commit_direct_request(
            &pipeline,
            &probe_session,
            &codec,
            body.as_bytes(),
            &probe_request.url,
            &probe_request.headers,
            WireFormat::Json,
            CodecLimits::default(),
            false,
            DEFAULT_MAX_REQUEST_BYTES,
        )
        .unwrap();
        assert!(body.len() < request_limit);
        assert!(transformed.request.body.len() > request_limit);

        let registry_config = crate::SessionRegistryConfig::default();
        let registry = SessionRegistry::with_secret(registry_config, [0x5a; 32]);
        let observed_lease = registry
            .acquire(
                crate::AuthenticatedPrincipal::try_from_canonical_bytes(
                    b"synthetic-principal".to_vec(),
                )
                .unwrap(),
                SESSION_ID,
                None,
            )
            .unwrap();
        let request_lease = registry
            .acquire(
                crate::AuthenticatedPrincipal::try_from_canonical_bytes(
                    b"synthetic-principal".to_vec(),
                )
                .unwrap(),
                SESSION_ID,
                None,
            )
            .unwrap();
        let client_config = DirectClientConfig::default()
            .try_with_max_request_bytes(request_limit)
            .unwrap();
        let runtime = DirectRuntime {
            client: DirectClient::new(client_config).unwrap(),
            endpoint: url.clone(),
            session_policy: SessionPolicy::OpaqueHeaderContinuity(registry_config),
            registry: Some(registry),
            resolver: Arc::new(SyntheticPrincipalResolver),
            listener_scope: ListenerScope::Loopback,
            allowed_versions: Arc::from([DEFAULT_ANTHROPIC_VERSION.to_string()]),
            allowed_betas: Arc::from([]),
            codec_limits: CodecLimits::default(),
            client_config,
            ping_interval: Duration::from_secs(10),
            inspection_endpoint_codes: ProxyInspectionEndpointCodesV1::from_validated_origin(&url),
        };
        let state = AppState {
            config: Arc::new(ProxyConfig::new("127.0.0.1:0".parse().unwrap(), Vec::new())),
            pipeline: Arc::new(pipeline),
            client: Client::new(),
            direct: Some(Arc::new(runtime)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            started_at: Instant::now(),
        };
        let ingress = PreparedDirectIngress {
            outbound_headers: probe_request.headers.clone(),
            session: DirectIngressSession::Continuity(request_lease),
        };

        let error = direct_proxy_inner(&state, ingress, body.as_bytes(), &codec)
            .await
            .unwrap_err();

        assert_eq!(error.code(), ProxyErrorCode::RequestBodyLimitExceeded);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 0);
        assert!(observed_lease.session().tokens().is_empty());
        assert!(observed_lease.session().snapshot_entries().is_empty());

        let extended_body = format!(
            r#"{{"model":"claude-test","max_tokens":32,"messages":[{{"role":"user","content":"{SYNTHETIC_PHONE} reports"}}]}}"#
        );
        let recovered = prepare_and_commit_direct_request(
            &state.pipeline,
            observed_lease.session(),
            &codec,
            extended_body.as_bytes(),
            &url,
            &probe_request.headers,
            WireFormat::Json,
            CodecLimits::default(),
            false,
            DEFAULT_MAX_REQUEST_BYTES,
        )
        .unwrap();
        let first_token = format!(
            "<{}:Custom:phone_1>",
            observed_lease.session().session_hex()
        );
        assert_eq!(observed_lease.session().tokens(), vec![first_token.clone()]);
        assert!(std::str::from_utf8(&recovered.request.body)
            .unwrap()
            .contains(&first_token));
        assert_eq!(capture.hits.load(Ordering::SeqCst), 0);
        server.abort();
    }

    #[tokio::test]
    async fn overlimit_proved_request_and_duplicate_credentials_do_zero_io() {
        let capture = Capture::default();
        let (url, server) = spawn_test_server(
            Router::new()
                .route("/v1/messages", post(capture_ok))
                .with_state(capture.clone()),
        )
        .await;
        let config = DirectClientConfig::default()
            .try_with_max_request_bytes(4)
            .unwrap();
        let client = DirectClient::new(config).unwrap();
        let error = client
            .execute(committed_direct_request(
                url.clone(),
                WireFormat::Json,
                br#"{"over":true}"#,
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::RequestBodyLimitExceeded);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 0);

        for singleton in ["x-api-key", "anthropic-version", "anthropic-beta"] {
            let mut duplicate = HeaderMap::new();
            duplicate.insert("content-type", HeaderValue::from_static("application/json"));
            duplicate.append(singleton, HeaderValue::from_static("synthetic-value-1"));
            duplicate.append(singleton, HeaderValue::from_static("synthetic-value-2"));
            let error = DirectRequest::from_proved(
                url.clone(),
                duplicate,
                proved(br#"{}"#),
                WireFormat::Json,
            )
            .err()
            .unwrap();
            assert_eq!(error.code(), ProxyErrorCode::HeaderRejected);
        }

        let mut unverified_headers = HeaderMap::new();
        unverified_headers.insert("content-type", HeaderValue::from_static("application/json"));
        let unverified = ProvedRequestBody::new(
            br#"{}"#.to_vec(),
            WireFormat::Json,
            OutputProvenance::default(),
        );
        let error =
            DirectRequest::from_proved(url, unverified_headers, unverified, WireFormat::Json)
                .err()
                .unwrap();
        assert_eq!(error.code(), ProxyErrorCode::InvalidProvenance);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 0);
        server.abort();
    }

    #[tokio::test]
    async fn redirects_retries_and_response_overflow_remain_disabled() {
        let target_hits = Arc::new(AtomicUsize::new(0));
        let target_hits_for_handler = Arc::clone(&target_hits);
        let (target_url, target) = spawn_test_server(Router::new().route(
            "/v1/messages",
            post(move || {
                let hits = Arc::clone(&target_hits_for_handler);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            }),
        ))
        .await;
        let origin_hits = Arc::new(AtomicUsize::new(0));
        let origin_hits_for_handler = Arc::clone(&origin_hits);
        let location = target_url.to_string();
        let (origin_url, origin) = spawn_test_server(Router::new().route(
            "/v1/messages",
            post(move || {
                let hits = Arc::clone(&origin_hits_for_handler);
                let location = location.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::TEMPORARY_REDIRECT)
                        .header("location", location)
                        .body(Body::from("synthetic redirect body"))
                        .unwrap()
                }
            }),
        ))
        .await;
        let client = DirectClient::new(DirectClientConfig::default()).unwrap();
        let error = client
            .execute(committed_direct_request(
                origin_url,
                WireFormat::Json,
                br#"{}"#,
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::UpstreamRedirect);
        assert_eq!(origin_hits.load(Ordering::SeqCst), 1);
        assert_eq!(target_hits.load(Ordering::SeqCst), 0);
        origin.abort();
        target.abort();

        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_handler = Arc::clone(&attempts);
        let (url, server) = spawn_test_server(Router::new().route(
            "/v1/messages",
            post(move || {
                let attempts = Arc::clone(&attempts_for_handler);
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    StatusCode::SERVICE_UNAVAILABLE
                }
            }),
        ))
        .await;
        let error = client
            .execute(committed_direct_request(url, WireFormat::Json, br#"{}"#))
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::UpstreamUnavailable);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        server.abort();

        let chunks = futures_util::stream::iter([
            Ok::<_, std::convert::Infallible>(Bytes::from_static(b"12345")),
            Ok(Bytes::from_static(b"67890")),
        ]);
        let body = Arc::new(Mutex::new(Some(Body::from_stream(chunks))));
        let body_for_handler = Arc::clone(&body);
        let (url, server) = spawn_test_server(Router::new().route(
            "/v1/messages",
            post(move || {
                let body = body_for_handler.lock().unwrap().take().unwrap();
                async move {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(body)
                        .unwrap()
                }
            }),
        ))
        .await;
        let config = DirectClientConfig::default()
            .try_with_max_response_bytes(8)
            .unwrap();
        let client = DirectClient::new(config).unwrap();
        let error = client
            .execute(committed_direct_request(url, WireFormat::Json, br#"{}"#))
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::ResponseBodyLimitExceeded);
        server.abort();
    }

    #[tokio::test]
    async fn timeout_and_body_stream_error_retain_io_attempted() {
        let (slow_url, slow_server) = spawn_test_server(Router::new().route(
            "/v1/messages",
            post(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                ([("content-type", "application/json")], "{}")
            }),
        ))
        .await;
        let config = DirectClientConfig::default()
            .try_with_timeouts(
                Duration::from_millis(20),
                Duration::from_millis(80),
                Duration::from_millis(30),
            )
            .unwrap();
        let client = DirectClient::new(config).unwrap();
        let error = client
            .execute(committed_direct_request(
                slow_url,
                WireFormat::Json,
                br#"{}"#,
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::TotalTimeout);
        slow_server.abort();

        let chunks =
            futures_util::stream::once(async { Ok::<_, std::io::Error>(Bytes::from_static(b"{")) })
                .chain(futures_util::stream::once(async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Err(std::io::Error::other("synthetic stream failure"))
                }));
        let body = Body::from_stream(chunks);
        let body = Arc::new(Mutex::new(Some(body)));
        let body_for_handler = Arc::clone(&body);
        let (url, server) = spawn_test_server(Router::new().route(
            "/v1/messages",
            post(move || {
                let body = body_for_handler.lock().unwrap().take().unwrap();
                async move {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(body)
                        .unwrap()
                }
            }),
        ))
        .await;
        let client = DirectClient::new(DirectClientConfig::default()).unwrap();
        let error = client
            .execute(committed_direct_request(url, WireFormat::Json, br#"{}"#))
            .await
            .unwrap_err();
        assert_eq!(error.phase(), ProxyErrorPhase::UpstreamBody);
        server.abort();
    }

    #[test]
    fn proxy_environment_is_ignored_in_isolated_helper() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "server::tests::proxy_environment_helper",
                "--nocapture",
            ])
            .env("GAZE_PROXY_ENV_HELPER", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated proxy helper failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn proxy_environment_helper() {
        if std::env::var_os("GAZE_PROXY_ENV_HELPER").is_none() {
            return;
        }
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let capture = Capture::default();
            let (url, server) = spawn_test_server(
                Router::new()
                    .route("/v1/messages", post(capture_ok))
                    .with_state(capture.clone()),
            )
            .await;
            let trap = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let proxy = format!("http://{}", trap.local_addr().unwrap());
            for name in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
            ] {
                std::env::set_var(name, &proxy);
            }
            std::env::remove_var("NO_PROXY");
            std::env::remove_var("no_proxy");

            let client = DirectClient::new(DirectClientConfig::default()).unwrap();
            client
                .execute(committed_direct_request(url, WireFormat::Json, br#"{}"#))
                .await
                .unwrap();
            assert_eq!(capture.hits.load(Ordering::SeqCst), 1);
            assert!(
                tokio::time::timeout(Duration::from_millis(100), trap.accept())
                    .await
                    .is_err()
            );
            server.abort();
        });
    }

    #[test]
    fn direct_frames_match_anthropic_codec_bytes() {
        assert_eq!(
            DIRECT_PROXY_PING_FRAME,
            crate::codecs::anthropic::ANTHROPIC_PROXY_PING_FRAME
        );
        assert_eq!(
            DIRECT_PROXY_ERROR_FRAME,
            crate::codecs::anthropic::ANTHROPIC_PROXY_ERROR_FRAME
        );
    }

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
            direct: None,
            started_at: Instant::now(),
        };
        assert_eq!(health_snapshot(&state).adapters[0].name, "openai");
    }
}
