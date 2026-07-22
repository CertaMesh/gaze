use std::fmt;
use std::time::Duration;

use http::Method;
use serde_json::Value;
use url::Url;

use crate::codec::{BodyCodec, CodecLimits, WireFormat};

/// Transport protocol selected by a reviewed adapter contract.
#[derive(Clone, Copy)]
pub enum ProtocolContract<'a> {
    /// Preserve the pre-codec JSON/SSE surface traversal path.
    LegacySurfaces,
    /// Transform complete bodies through the borrowed codec.
    Codec(&'a dyn BodyCodec),
}

impl ProtocolContract<'_> {
    /// Returns the selected codec, if this is a codec-backed contract.
    #[must_use]
    pub const fn codec(&self) -> Option<&dyn BodyCodec> {
        match self {
            Self::LegacySurfaces => None,
            Self::Codec(codec) => Some(*codec),
        }
    }

    /// Reports whether this contract preserves the legacy traversal path.
    #[must_use]
    pub const fn is_legacy(&self) -> bool {
        matches!(self, Self::LegacySurfaces)
    }
}

impl fmt::Debug for ProtocolContract<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LegacySurfaces => formatter.write_str("LegacySurfaces"),
            Self::Codec(_) => formatter.debug_tuple("Codec").field(&"BodyCodec").finish(),
        }
    }
}

/// Session lifecycle required by an adapter protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPolicy {
    /// A request owns an isolated session that is discarded after its response.
    EphemeralSingleRequest,
    /// An opaque header selects a bounded continuity registry.
    OpaqueHeaderContinuity(SessionRegistryConfig),
}

/// Closed validation failure for [`SessionRegistryConfig`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRegistryConfigError {
    InvalidMaxActiveSessions,
    InvalidSessionTtl,
    InvalidMaxTombstones,
    InvalidMaxTombstoneBytes,
    InvalidTombstoneTtl,
    NotALowering,
}

impl fmt::Display for SessionRegistryConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidMaxActiveSessions => "invalid maximum active session count",
            Self::InvalidSessionTtl => "invalid session TTL",
            Self::InvalidMaxTombstones => "invalid maximum tombstone count",
            Self::InvalidMaxTombstoneBytes => "invalid maximum tombstone byte count",
            Self::InvalidTombstoneTtl => "invalid tombstone TTL",
            Self::NotALowering => "registry limit may only be lowered",
        })
    }
}

impl std::error::Error for SessionRegistryConfigError {}

/// Finite, validated bounds for an opaque-session continuity registry.
///
/// Fields are deliberately private so callers cannot bypass validation.
///
/// ```compile_fail
/// use gaze_proxy::SessionRegistryConfig;
///
/// let _ = SessionRegistryConfig { max_active_sessions: 1 };
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionRegistryConfig {
    max_active_sessions: usize,
    session_ttl: Duration,
    max_tombstones: usize,
    max_tombstone_bytes: usize,
    tombstone_ttl: Duration,
}

impl Default for SessionRegistryConfig {
    fn default() -> Self {
        Self {
            max_active_sessions: 4_096,
            session_ttl: Duration::from_secs(30 * 60),
            max_tombstones: 4_096,
            max_tombstone_bytes: 1024 * 1024,
            tombstone_ttl: Duration::from_secs(30 * 60),
        }
    }
}

impl SessionRegistryConfig {
    pub const HARD_MAX_ACTIVE_SESSIONS: usize = 65_536;
    pub const HARD_MAX_SESSION_TTL: Duration = Duration::from_secs(24 * 60 * 60);
    pub const HARD_MAX_TOMBSTONES: usize = 65_536;
    pub const HARD_MAX_TOMBSTONE_BYTES: usize = 64 * 1024 * 1024;
    pub const HARD_MAX_TOMBSTONE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

    /// Constructs a registry configuration after validating every finite bound.
    pub fn try_new(
        max_active_sessions: usize,
        session_ttl: Duration,
        max_tombstones: usize,
        max_tombstone_bytes: usize,
        tombstone_ttl: Duration,
    ) -> Result<Self, SessionRegistryConfigError> {
        if max_active_sessions == 0 || max_active_sessions > Self::HARD_MAX_ACTIVE_SESSIONS {
            return Err(SessionRegistryConfigError::InvalidMaxActiveSessions);
        }
        if session_ttl.is_zero() || session_ttl > Self::HARD_MAX_SESSION_TTL {
            return Err(SessionRegistryConfigError::InvalidSessionTtl);
        }
        if max_tombstones == 0 || max_tombstones > Self::HARD_MAX_TOMBSTONES {
            return Err(SessionRegistryConfigError::InvalidMaxTombstones);
        }
        if max_tombstone_bytes == 0 || max_tombstone_bytes > Self::HARD_MAX_TOMBSTONE_BYTES {
            return Err(SessionRegistryConfigError::InvalidMaxTombstoneBytes);
        }
        if tombstone_ttl.is_zero() || tombstone_ttl > Self::HARD_MAX_TOMBSTONE_TTL {
            return Err(SessionRegistryConfigError::InvalidTombstoneTtl);
        }
        Ok(Self {
            max_active_sessions,
            session_ttl,
            max_tombstones,
            max_tombstone_bytes,
            tombstone_ttl,
        })
    }

    #[must_use]
    pub const fn max_active_sessions(&self) -> usize {
        self.max_active_sessions
    }

    #[must_use]
    pub const fn session_ttl(&self) -> Duration {
        self.session_ttl
    }

    #[must_use]
    pub const fn max_tombstones(&self) -> usize {
        self.max_tombstones
    }

    #[must_use]
    pub const fn max_tombstone_bytes(&self) -> usize {
        self.max_tombstone_bytes
    }

    #[must_use]
    pub const fn tombstone_ttl(&self) -> Duration {
        self.tombstone_ttl
    }

    pub fn try_with_max_active_sessions(
        mut self,
        value: usize,
    ) -> Result<Self, SessionRegistryConfigError> {
        if value == 0 || value > Self::HARD_MAX_ACTIVE_SESSIONS {
            return Err(SessionRegistryConfigError::InvalidMaxActiveSessions);
        }
        if value > self.max_active_sessions {
            return Err(SessionRegistryConfigError::NotALowering);
        }
        self.max_active_sessions = value;
        Ok(self)
    }

    pub fn try_with_session_ttl(
        mut self,
        value: Duration,
    ) -> Result<Self, SessionRegistryConfigError> {
        if value.is_zero() || value > Self::HARD_MAX_SESSION_TTL {
            return Err(SessionRegistryConfigError::InvalidSessionTtl);
        }
        if value > self.session_ttl {
            return Err(SessionRegistryConfigError::NotALowering);
        }
        self.session_ttl = value;
        Ok(self)
    }

    pub fn try_with_max_tombstones(
        mut self,
        value: usize,
    ) -> Result<Self, SessionRegistryConfigError> {
        if value == 0 || value > Self::HARD_MAX_TOMBSTONES {
            return Err(SessionRegistryConfigError::InvalidMaxTombstones);
        }
        if value > self.max_tombstones {
            return Err(SessionRegistryConfigError::NotALowering);
        }
        self.max_tombstones = value;
        Ok(self)
    }

    pub fn try_with_max_tombstone_bytes(
        mut self,
        value: usize,
    ) -> Result<Self, SessionRegistryConfigError> {
        if value == 0 || value > Self::HARD_MAX_TOMBSTONE_BYTES {
            return Err(SessionRegistryConfigError::InvalidMaxTombstoneBytes);
        }
        if value > self.max_tombstone_bytes {
            return Err(SessionRegistryConfigError::NotALowering);
        }
        self.max_tombstone_bytes = value;
        Ok(self)
    }

    pub fn try_with_tombstone_ttl(
        mut self,
        value: Duration,
    ) -> Result<Self, SessionRegistryConfigError> {
        if value.is_zero() || value > Self::HARD_MAX_TOMBSTONE_TTL {
            return Err(SessionRegistryConfigError::InvalidTombstoneTtl);
        }
        if value > self.tombstone_ttl {
            return Err(SessionRegistryConfigError::NotALowering);
        }
        self.tombstone_ttl = value;
        Ok(self)
    }
}

/// Route matching policy declared by an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutePolicy {
    LegacyAdapterMatch,
    ExactMethodAndPath,
}

/// Forwarded-header policy declared by an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeaderPolicy {
    LegacyForwarded,
    StrictAllowlist,
}

/// Redirect policy declared by an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedirectPolicy {
    LegacyClient,
    Deny,
}

/// PII coverage proof policy declared by an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoveragePolicy {
    LegacySurfaces,
    CodecProved,
}

/// Request, response, and streaming wire formats accepted by an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatPolicy {
    request: WireFormat,
    response: WireFormat,
    streaming_response: WireFormat,
}

impl FormatPolicy {
    /// Characterizes the pre-codec JSON request/response and SSE stream behavior.
    #[must_use]
    pub const fn legacy() -> Self {
        Self {
            request: WireFormat::Json,
            response: WireFormat::Json,
            streaming_response: WireFormat::Sse,
        }
    }

    #[must_use]
    pub const fn request(&self) -> WireFormat {
        self.request
    }

    #[must_use]
    pub const fn response(&self) -> WireFormat {
        self.response
    }

    #[must_use]
    pub const fn streaming_response(&self) -> WireFormat {
        self.streaming_response
    }
}

/// Reviewed protocol, session, routing, and coverage declarations for an adapter.
///
/// Fields are private so downstream code cannot assemble an unreviewed contract.
/// Existing adapters obtain [`Self::legacy`] through the default trait method.
///
/// ```compile_fail
/// use gaze_proxy::{AdapterContract, ProtocolContract};
///
/// let _ = AdapterContract { protocol: ProtocolContract::LegacySurfaces };
/// ```
#[derive(Clone, Copy, Debug)]
pub struct AdapterContract<'a> {
    protocol: ProtocolContract<'a>,
    session_policy: Option<SessionPolicy>,
    route_policy: RoutePolicy,
    header_policy: HeaderPolicy,
    redirect_policy: RedirectPolicy,
    coverage_policy: CoveragePolicy,
    formats: FormatPolicy,
    codec_limits: Option<CodecLimits>,
}

impl AdapterContract<'static> {
    /// Characterizes the existing adapter behavior without changing it.
    #[must_use]
    pub const fn legacy() -> Self {
        Self {
            protocol: ProtocolContract::LegacySurfaces,
            session_policy: None,
            route_policy: RoutePolicy::LegacyAdapterMatch,
            header_policy: HeaderPolicy::LegacyForwarded,
            redirect_policy: RedirectPolicy::LegacyClient,
            coverage_policy: CoveragePolicy::LegacySurfaces,
            formats: FormatPolicy::legacy(),
            codec_limits: None,
        }
    }
}

impl<'a> AdapterContract<'a> {
    /// Constructs the only non-legacy policy bundle accepted by later direct adapters.
    #[allow(
        dead_code,
        reason = "consumed by the direct adapter in the next serialized checkpoint"
    )]
    pub(crate) const fn codec(
        codec: &'a dyn BodyCodec,
        session_policy: SessionPolicy,
        codec_limits: CodecLimits,
    ) -> Self {
        Self {
            protocol: ProtocolContract::Codec(codec),
            session_policy: Some(session_policy),
            route_policy: RoutePolicy::ExactMethodAndPath,
            header_policy: HeaderPolicy::StrictAllowlist,
            redirect_policy: RedirectPolicy::Deny,
            coverage_policy: CoveragePolicy::CodecProved,
            formats: FormatPolicy::legacy(),
            codec_limits: Some(codec_limits),
        }
    }

    #[must_use]
    pub const fn protocol(&self) -> ProtocolContract<'a> {
        self.protocol
    }

    /// Legacy adapters preserve the server's pre-contract auto-create behavior.
    #[must_use]
    pub const fn uses_legacy_session_auto_create(&self) -> bool {
        self.session_policy.is_none()
    }

    #[must_use]
    pub const fn session_policy(&self) -> Option<SessionPolicy> {
        self.session_policy
    }

    #[must_use]
    pub const fn route_policy(&self) -> RoutePolicy {
        self.route_policy
    }

    #[must_use]
    pub const fn header_policy(&self) -> HeaderPolicy {
        self.header_policy
    }

    #[must_use]
    pub const fn redirect_policy(&self) -> RedirectPolicy {
        self.redirect_policy
    }

    #[must_use]
    pub const fn coverage_policy(&self) -> CoveragePolicy {
        self.coverage_policy
    }

    #[must_use]
    pub const fn formats(&self) -> FormatPolicy {
        self.formats
    }

    #[must_use]
    pub const fn codec_limits(&self) -> Option<CodecLimits> {
        self.codec_limits
    }
}

#[async_trait::async_trait]
pub trait ProviderAdapter: Send + Sync + 'static {
    /// Declares protocol and safety behavior while preserving old implementations.
    fn contract(&self) -> AdapterContract<'_> {
        AdapterContract::legacy()
    }

    fn name(&self) -> &'static str;
    fn matches_path(&self, method: &Method, path: &str) -> bool;
    fn upstream_base(&self) -> &Url;
    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>>;
    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>>;
    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>>;
}

pub struct PiiSurface<'a> {
    pub field_path: String,
    pub text: &'a mut String,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: Value,
}

impl SseEvent {
    pub fn new(event: Option<String>, data: Value) -> Self {
        Self { event, data }
    }
}

pub(crate) fn push_string<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    field_path: impl Into<String>,
    value: &'a mut Value,
) {
    if let Value::String(text) = value {
        surfaces.push(PiiSurface {
            field_path: field_path.into(),
            text,
        });
    }
}

pub(crate) fn walk_all_strings<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    prefix: String,
    value: &'a mut Value,
) {
    match value {
        Value::String(text) => surfaces.push(PiiSurface {
            field_path: prefix,
            text,
        }),
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                walk_all_strings(surfaces, format!("{prefix}[{index}]"), item);
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                walk_all_strings(surfaces, path, child);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

pub(crate) fn push_text_blocks<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    prefix: &str,
    value: &'a mut Value,
) {
    match value {
        Value::String(text) => surfaces.push(PiiSurface {
            field_path: prefix.to_string(),
            text,
        }),
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                match item {
                    Value::String(text) => surfaces.push(PiiSurface {
                        field_path: format!("{prefix}[{index}]"),
                        text,
                    }),
                    Value::Object(map) => {
                        if matches!(map.get("type"), Some(Value::String(kind)) if kind == "text") {
                            if let Some(Value::String(text)) = map.get_mut("text") {
                                surfaces.push(PiiSurface {
                                    field_path: format!("{prefix}[{index}].text"),
                                    text,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
