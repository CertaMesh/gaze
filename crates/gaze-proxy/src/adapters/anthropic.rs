use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use http::Method;
use serde_json::Value;
use url::{Host, Url};

use crate::adapter::{
    push_string, push_text_blocks, walk_all_strings, AdapterContract, PiiSurface, ProviderAdapter,
    SessionPolicy, SessionRegistryConfig, SseEvent,
};
use crate::codec::CodecLimits;
use crate::codecs::anthropic::AnthropicMessagesCodec;
use crate::error::{DirectProxyError, ProxyErrorCode, ProxyErrorPhase};
use crate::principal::PrincipalResolver;

pub const DEFAULT_ANTHROPIC_UPSTREAM: &str = "https://api.anthropic.com";
pub const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(180);
const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(10);
const MAX_HEADER_COUNT: usize = 64;
const MAX_HEADER_NAME_BYTES: usize = 128;
const MAX_HEADER_VALUE_BYTES: usize = 8 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;

#[derive(Clone)]
#[non_exhaustive]
pub struct AnthropicAdapter {
    upstream: Url,
    codec: AnthropicMessagesCodec,
    session_policy: SessionPolicy,
    allowed_versions: Arc<[String]>,
    allowed_betas: Arc<[String]>,
    codec_limits: CodecLimits,
    connect_timeout: Duration,
    request_timeout: Duration,
    total_timeout: Duration,
    ping_interval: Duration,
    max_headers: usize,
    max_header_name_bytes: usize,
    max_header_value_bytes: usize,
    max_header_bytes: usize,
    principal_resolver: Option<Arc<dyn PrincipalResolver>>,
}

impl fmt::Debug for AnthropicAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnthropicAdapter")
            .field("session_policy", &self.session_policy)
            .field("allowed_version_count", &self.allowed_versions.len())
            .field("allowed_beta_count", &self.allowed_betas.len())
            .field("codec_limits", &self.codec_limits)
            .field("has_principal_resolver", &self.principal_resolver.is_some())
            .finish()
    }
}

impl AnthropicAdapter {
    pub fn new(upstream: Url) -> Self {
        Self {
            upstream,
            codec: AnthropicMessagesCodec,
            session_policy: SessionPolicy::EphemeralSingleRequest,
            allowed_versions: Arc::from([DEFAULT_ANTHROPIC_VERSION.to_string()]),
            allowed_betas: Arc::<[String]>::from([]),
            codec_limits: CodecLimits::default(),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            total_timeout: DEFAULT_TOTAL_TIMEOUT,
            ping_interval: DEFAULT_PING_INTERVAL,
            max_headers: MAX_HEADER_COUNT,
            max_header_name_bytes: MAX_HEADER_NAME_BYTES,
            max_header_value_bytes: MAX_HEADER_VALUE_BYTES,
            max_header_bytes: MAX_HEADER_BYTES,
            principal_resolver: None,
        }
    }

    #[must_use]
    pub fn builder(upstream: Url) -> AnthropicAdapterBuilder {
        AnthropicAdapterBuilder::new(upstream)
    }

    #[must_use]
    pub fn allowed_versions(&self) -> &[String] {
        &self.allowed_versions
    }

    #[must_use]
    pub fn allowed_betas(&self) -> &[String] {
        &self.allowed_betas
    }

    pub(crate) fn validate_upstream_origin(&self) -> Result<(), DirectProxyError> {
        validate_upstream_origin(&self.upstream)
    }

    pub(crate) const fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub(crate) const fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub(crate) const fn total_timeout(&self) -> Duration {
        self.total_timeout
    }

    pub(crate) const fn ping_interval(&self) -> Duration {
        self.ping_interval
    }

    pub(crate) const fn header_limits(&self) -> (usize, usize, usize, usize) {
        (
            self.max_headers,
            self.max_header_name_bytes,
            self.max_header_value_bytes,
            self.max_header_bytes,
        )
    }

    pub(crate) fn principal_resolver(&self) -> Option<&Arc<dyn PrincipalResolver>> {
        self.principal_resolver.as_ref()
    }
}

pub struct AnthropicAdapterBuilder {
    adapter: AnthropicAdapter,
}

impl fmt::Debug for AnthropicAdapterBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnthropicAdapterBuilder")
            .field("adapter", &self.adapter)
            .finish()
    }
}

impl AnthropicAdapterBuilder {
    fn new(upstream: Url) -> Self {
        let mut adapter = AnthropicAdapter::new(upstream);
        adapter.session_policy =
            SessionPolicy::OpaqueHeaderContinuity(SessionRegistryConfig::default());
        Self { adapter }
    }

    #[must_use]
    pub fn session_registry_config(mut self, config: SessionRegistryConfig) -> Self {
        self.adapter.session_policy = SessionPolicy::OpaqueHeaderContinuity(config);
        self
    }

    #[must_use]
    pub fn codec_limits(mut self, limits: CodecLimits) -> Self {
        self.adapter.codec_limits = limits;
        self
    }

    pub fn connect_timeout(mut self, timeout: Duration) -> Result<Self, DirectProxyError> {
        validate_timeout(timeout, DEFAULT_CONNECT_TIMEOUT)?;
        self.adapter.connect_timeout = timeout;
        Ok(self)
    }

    pub fn request_timeout(mut self, timeout: Duration) -> Result<Self, DirectProxyError> {
        validate_timeout(timeout, DEFAULT_REQUEST_TIMEOUT)?;
        self.adapter.request_timeout = timeout;
        Ok(self)
    }

    pub fn total_timeout(mut self, timeout: Duration) -> Result<Self, DirectProxyError> {
        validate_timeout(timeout, DEFAULT_TOTAL_TIMEOUT)?;
        self.adapter.total_timeout = timeout;
        Ok(self)
    }

    pub fn ping_interval(mut self, interval: Duration) -> Result<Self, DirectProxyError> {
        if !(Duration::from_secs(1)..=Duration::from_secs(30)).contains(&interval) {
            return Err(
                ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::UpstreamConfiguration)
            );
        }
        self.adapter.ping_interval = interval;
        Ok(self)
    }

    pub fn max_headers(mut self, value: usize) -> Result<Self, DirectProxyError> {
        validate_lowered_limit(value, MAX_HEADER_COUNT)?;
        self.adapter.max_headers = value;
        Ok(self)
    }

    pub fn max_header_name_bytes(mut self, value: usize) -> Result<Self, DirectProxyError> {
        validate_lowered_limit(value, MAX_HEADER_NAME_BYTES)?;
        self.adapter.max_header_name_bytes = value;
        Ok(self)
    }

    pub fn max_header_value_bytes(mut self, value: usize) -> Result<Self, DirectProxyError> {
        validate_lowered_limit(value, MAX_HEADER_VALUE_BYTES)?;
        self.adapter.max_header_value_bytes = value;
        Ok(self)
    }

    pub fn max_header_bytes(mut self, value: usize) -> Result<Self, DirectProxyError> {
        validate_lowered_limit(value, MAX_HEADER_BYTES)?;
        self.adapter.max_header_bytes = value;
        Ok(self)
    }

    pub fn allow_version(mut self, version: impl Into<String>) -> Result<Self, DirectProxyError> {
        let version = version.into();
        validate_allowlist_value(&version)?;
        let mut values = self.adapter.allowed_versions.to_vec();
        if !values.contains(&version) {
            values.push(version);
        }
        self.adapter.allowed_versions = values.into();
        Ok(self)
    }

    pub fn allow_beta(mut self, beta: impl Into<String>) -> Result<Self, DirectProxyError> {
        let beta = beta.into();
        validate_allowlist_value(&beta)?;
        let mut values = self.adapter.allowed_betas.to_vec();
        if !values.contains(&beta) {
            values.push(beta);
        }
        self.adapter.allowed_betas = values.into();
        Ok(self)
    }

    #[must_use]
    pub fn principal_resolver(mut self, resolver: Arc<dyn PrincipalResolver>) -> Self {
        self.adapter.principal_resolver = Some(resolver);
        self
    }

    pub fn build(self) -> Result<AnthropicAdapter, DirectProxyError> {
        self.adapter.validate_upstream_origin()?;
        validate_codec_limits(self.adapter.codec_limits)?;
        if self.adapter.connect_timeout > self.adapter.request_timeout
            || self.adapter.request_timeout > self.adapter.total_timeout
        {
            return Err(configuration_error());
        }
        Ok(self.adapter)
    }
}

fn validate_timeout(timeout: Duration, maximum: Duration) -> Result<(), DirectProxyError> {
    if timeout.is_zero() || timeout > maximum {
        Err(configuration_error())
    } else {
        Ok(())
    }
}

fn validate_lowered_limit(value: usize, maximum: usize) -> Result<(), DirectProxyError> {
    if value == 0 || value > maximum {
        Err(configuration_error())
    } else {
        Ok(())
    }
}

fn validate_codec_limits(limits: CodecLimits) -> Result<(), DirectProxyError> {
    let ceilings = CodecLimits::default();
    for (value, maximum) in [
        (limits.max_body_bytes(), ceilings.max_body_bytes()),
        (limits.max_json_depth(), ceilings.max_json_depth()),
        (limits.max_json_nodes(), ceilings.max_json_nodes()),
        (limits.max_string_bytes(), ceilings.max_string_bytes()),
        (limits.max_number_bytes(), ceilings.max_number_bytes()),
        (limits.max_sse_line_bytes(), ceilings.max_sse_line_bytes()),
        (limits.max_sse_frame_bytes(), ceilings.max_sse_frame_bytes()),
        (limits.max_sse_events(), ceilings.max_sse_events()),
        (limits.max_sse_indices(), ceilings.max_sse_indices()),
        (
            limits.max_sse_accumulator_bytes(),
            ceilings.max_sse_accumulator_bytes(),
        ),
    ] {
        validate_lowered_limit(value, maximum)?;
    }

    let body = limits.max_body_bytes();
    if limits.max_sse_line_bytes() > limits.max_sse_frame_bytes()
        || limits.max_sse_line_bytes() > body
        || limits.max_sse_frame_bytes() > body
        || limits.max_string_bytes() > body
        || limits.max_number_bytes() > body
        || limits.max_sse_accumulator_bytes() > body
    {
        return Err(configuration_error());
    }

    Ok(())
}

fn validate_allowlist_value(value: &str) -> Result<(), DirectProxyError> {
    if value.is_empty()
        || value.len() > MAX_HEADER_VALUE_BYTES
        || value
            .bytes()
            .any(|byte| !matches!(byte, 0x21..=0x7e) || byte == b',')
    {
        Err(ProxyErrorCode::HeaderRejected.error(ProxyErrorPhase::UpstreamConfiguration))
    } else {
        Ok(())
    }
}

fn validate_upstream_origin(upstream: &Url) -> Result<(), DirectProxyError> {
    let safe_authority = upstream.username().is_empty()
        && upstream.password().is_none()
        && upstream.query().is_none()
        && upstream.fragment().is_none()
        && upstream.path() == "/"
        && upstream.host().is_some();
    let safe_scheme = match (upstream.scheme(), upstream.host()) {
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

fn configuration_error() -> DirectProxyError {
    ProxyErrorCode::ProxyConfiguration.error(ProxyErrorPhase::UpstreamConfiguration)
}

#[async_trait::async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn contract(&self) -> AdapterContract<'_> {
        AdapterContract::codec(&self.codec, self.session_policy, self.codec_limits)
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn matches_path(&self, method: &Method, path: &str) -> bool {
        method == Method::POST && path == "/v1/messages"
    }

    fn upstream_base(&self) -> &Url {
        &self.upstream
    }

    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = body {
            for (key, value) in root {
                match key.as_str() {
                    "system" => push_text_blocks(&mut surfaces, "system", value),
                    "messages" => {
                        if let Value::Array(messages) = value {
                            for (index, message) in messages.iter_mut().enumerate() {
                                if let Value::Object(message) = message {
                                    for (message_key, message_value) in message {
                                        if message_key == "content" {
                                            collect_content_blocks(
                                                &mut surfaces,
                                                &format!("messages[{index}].content"),
                                                message_value,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        surfaces
    }

    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = body {
            if let Some(content) = root.get_mut("content") {
                collect_content_blocks(&mut surfaces, "content", content);
            }
        }
        surfaces
    }

    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = &mut event.data {
            if matches!(root.get("type"), Some(Value::String(kind)) if kind == "content_block_delta")
            {
                if let Some(Value::Object(delta)) = root.get_mut("delta") {
                    for (key, value) in delta {
                        if let Value::String(text) = value {
                            match key.as_str() {
                                "text" => surfaces.push(PiiSurface {
                                    field_path: "delta.text".to_string(),
                                    text,
                                }),
                                "partial_json" => surfaces.push(PiiSurface {
                                    field_path: "delta.partial_json".to_string(),
                                    text,
                                }),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        surfaces
    }
}

fn collect_content_blocks<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    prefix: &str,
    content: &'a mut Value,
) {
    match content {
        Value::String(_) => push_string(surfaces, prefix, content),
        Value::Array(blocks) => {
            for (index, block) in blocks.iter_mut().enumerate() {
                let Value::Object(block) = block else {
                    continue;
                };
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(Value::String(text)) = block.get_mut("text") {
                            surfaces.push(PiiSurface {
                                field_path: format!("{prefix}[{index}].text"),
                                text,
                            });
                        }
                    }
                    Some("tool_use") => {
                        if let Some(input) = block.get_mut("input") {
                            walk_all_strings(surfaces, format!("{prefix}[{index}].input"), input);
                        }
                    }
                    Some("tool_result") => {
                        if let Some(result) = block.get_mut("content") {
                            collect_content_blocks(
                                surfaces,
                                &format!("{prefix}[{index}].content"),
                                result,
                            );
                        }
                    }
                    Some("image") | None | Some(_) => {}
                }
            }
        }
        _ => {}
    }
}
