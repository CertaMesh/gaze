use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::StreamExt;
use gaze::{token_shape, CleanDocument, Pipeline, RawDocument, Scope, Session};
use reqwest::{Client, ClientBuilder};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use url::{Host, Url};

use crate::adapter::{ProviderAdapter, SseEvent};
use crate::codec::{ProvedRequestBody, WireFormat};
use crate::error::{DirectProxyError, ProxyError, ProxyErrorCode, ProxyErrorPhase};
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

/// One exact already-proved direct request. It has no `Debug` implementation by design.
#[allow(dead_code)]
pub(crate) struct DirectRequest {
    url: Url,
    headers: HeaderMap,
    body: Vec<u8>,
    expected_response: WireFormat,
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
        Ok(Self {
            url,
            headers,
            body: proved_body.into_bytes(),
            expected_response,
        })
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
    pub(crate) async fn execute(
        &self,
        lifecycle: &mut DirectTransactionState,
        request: DirectRequest,
    ) -> Result<DirectResponse, DirectProxyError> {
        lifecycle.require_committed_no_io()?;
        if request.body.len() > self.config.max_request_bytes {
            return Err(
                ProxyErrorCode::RequestBodyLimitExceeded.error(ProxyErrorPhase::RequestValidation)
            );
        }
        lifecycle.mark_io_attempted()?;
        match tokio::time::timeout(self.config.total_timeout, self.execute_inner(request)).await {
            Ok(result) => result,
            Err(_) => Err(ProxyErrorCode::TotalTimeout.error(ProxyErrorPhase::UpstreamBody)),
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

async fn proxy(State(state): State<AppState>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    let body = match collect_inbound_body(body, &parts.headers, state.config.body_limit_bytes).await
    {
        Ok(body) => body,
        Err(error) => return proxy_error_response(error),
    };
    match proxy_inner(state, parts.method, parts.uri, parts.headers, body).await {
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
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use axum::routing::post;

    use crate::codec::OutputProvenance;

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
        let mut lifecycle = DirectTransactionState::new();

        let error = client
            .execute(
                &mut lifecycle,
                direct_request(url.clone(), WireFormat::Json, exact).unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::InvalidStateTransition);
        assert_eq!(lifecycle.state(), &DirectIoState::Prepared);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 0);

        lifecycle.retry_generation_conflict().unwrap();
        assert_eq!(lifecycle.generation_retries(), 1);
        assert_eq!(
            lifecycle.retry_generation_conflict().unwrap_err().code(),
            ProxyErrorCode::SessionGenerationConflict
        );
        lifecycle.mark_committed().unwrap();
        let response = client
            .execute(
                &mut lifecycle,
                direct_request(url.clone(), WireFormat::Json, exact).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(lifecycle.state(), &DirectIoState::IoAttempted);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 1);
        assert_eq!(&*capture.body.lock().unwrap(), exact);
        assert_eq!(response.body(), br#"{"ok":true}"#);
        assert_eq!(response.headers().len(), 1);
        assert!(response.headers().get("set-cookie").is_none());

        let error = client
            .execute(
                &mut lifecycle,
                direct_request(url.clone(), WireFormat::Json, exact).unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::InvalidStateTransition);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 1);

        lifecycle.mark_response_complete().unwrap();
        assert_eq!(lifecycle.state(), &DirectIoState::ResponseComplete);
        let error = client
            .execute(
                &mut lifecycle,
                direct_request(url, WireFormat::Json, exact).unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::InvalidStateTransition);
        assert_eq!(capture.hits.load(Ordering::SeqCst), 1);
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
        let mut lifecycle = DirectTransactionState::new();
        lifecycle.mark_committed().unwrap();
        let error = client
            .execute(
                &mut lifecycle,
                direct_request(url.clone(), WireFormat::Json, br#"{"over":true}"#).unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::RequestBodyLimitExceeded);
        assert_eq!(lifecycle.state(), &DirectIoState::CommittedNoIo);
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
        let mut lifecycle = DirectTransactionState::new();
        lifecycle.mark_committed().unwrap();
        let error = client
            .execute(
                &mut lifecycle,
                direct_request(origin_url, WireFormat::Json, br#"{}"#).unwrap(),
            )
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
        let mut lifecycle = DirectTransactionState::new();
        lifecycle.mark_committed().unwrap();
        let error = client
            .execute(
                &mut lifecycle,
                direct_request(url, WireFormat::Json, br#"{}"#).unwrap(),
            )
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
        let mut lifecycle = DirectTransactionState::new();
        lifecycle.mark_committed().unwrap();
        let error = client
            .execute(
                &mut lifecycle,
                direct_request(url, WireFormat::Json, br#"{}"#).unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::ResponseBodyLimitExceeded);
        assert_eq!(lifecycle.state(), &DirectIoState::IoAttempted);
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
        let mut lifecycle = DirectTransactionState::new();
        lifecycle.mark_committed().unwrap();
        let error = client
            .execute(
                &mut lifecycle,
                direct_request(slow_url, WireFormat::Json, br#"{}"#).unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::TotalTimeout);
        assert_eq!(lifecycle.state(), &DirectIoState::IoAttempted);
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
        let mut lifecycle = DirectTransactionState::new();
        lifecycle.mark_committed().unwrap();
        let error = client
            .execute(
                &mut lifecycle,
                direct_request(url, WireFormat::Json, br#"{}"#).unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.phase(), ProxyErrorPhase::UpstreamBody);
        assert_eq!(lifecycle.state(), &DirectIoState::IoAttempted);
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
            let mut lifecycle = DirectTransactionState::new();
            lifecycle.mark_committed().unwrap();
            client
                .execute(
                    &mut lifecycle,
                    direct_request(url, WireFormat::Json, br#"{}"#).unwrap(),
                )
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
            started_at: Instant::now(),
        };
        assert_eq!(health_snapshot(&state).adapters[0].name, "openai");
    }
}
