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
/// Every adapter names its contract in [`ProviderAdapter::contract`]; there is no default.
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

/// A provider the proxy forwards to.
///
/// Every adapter must declare its [`AdapterContract`]; the trait has no default, so an adapter
/// that says nothing about its coverage does not compile:
///
/// ```compile_fail,E0046
/// use gaze_proxy::{PiiSurface, RequestSurfaces, ProviderAdapter, SseEvent};
/// use http::Method;
/// use serde_json::Value;
/// use url::Url;
///
/// struct Silent(Url);
///
/// #[async_trait::async_trait]
/// impl ProviderAdapter for Silent {
///     fn name(&self) -> &'static str { "silent" }
///     fn matches_path(&self, _: &Method, _: &str) -> bool { false }
///     fn upstream_base(&self) -> &Url { &self.0 }
///     fn request_pii_surfaces<'a>(&self, _: &'a mut Value) -> RequestSurfaces<'a> { RequestSurfaces::default() }
///     fn response_pii_surfaces<'a>(&self, _: &'a mut Value) -> Vec<PiiSurface<'a>> { Vec::new() }
///     fn sse_event_pii_surfaces<'a>(&self, _: &'a mut SseEvent) -> Vec<PiiSurface<'a>> { Vec::new() }
/// }
/// ```
///
/// Declaring [`AdapterContract::legacy`] opts into the string-surface path, where the proxy
/// re-scans the whole outbound request body and refuses it if anything is left unprotected:
///
/// ```
/// use gaze_proxy::{AdapterContract, PiiSurface, RequestSurfaces, ProviderAdapter, SseEvent};
/// use http::Method;
/// use serde_json::Value;
/// use url::Url;
///
/// struct Declared(Url);
///
/// #[async_trait::async_trait]
/// impl ProviderAdapter for Declared {
///     fn contract(&self) -> AdapterContract<'_> { AdapterContract::legacy() }
///     fn name(&self) -> &'static str { "declared" }
///     fn matches_path(&self, _: &Method, _: &str) -> bool { false }
///     fn upstream_base(&self) -> &Url { &self.0 }
///     fn request_pii_surfaces<'a>(&self, _: &'a mut Value) -> RequestSurfaces<'a> { RequestSurfaces::default() }
///     fn response_pii_surfaces<'a>(&self, _: &'a mut Value) -> Vec<PiiSurface<'a>> { Vec::new() }
///     fn sse_event_pii_surfaces<'a>(&self, _: &'a mut SseEvent) -> Vec<PiiSurface<'a>> { Vec::new() }
/// }
/// ```
#[async_trait::async_trait]
pub trait ProviderAdapter: Send + Sync + 'static {
    /// Declares protocol, session, routing and coverage behavior. Required: a default here
    /// would make every adapter that forgets it silently take the legacy path.
    fn contract(&self) -> AdapterContract<'_>;

    fn name(&self) -> &'static str;
    fn matches_path(&self, method: &Method, path: &str) -> bool;
    fn upstream_base(&self) -> &Url;
    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> RequestSurfaces<'a>;
    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>>;
    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>>;

    /// The surfaces of one SSE event, each paired with the logical stream it continues.
    ///
    /// Providers stream a value such as a message's text or a tool call's `arguments` as a
    /// series of fragments, roughly one model token per event, so a pseudonym token is usually
    /// split across events. The legacy proxy concatenates the fragments of each identified
    /// [`SseStreamSurface::stream`] across the whole response, restores the result once, and
    /// replays it in that stream's first fragment. An ambiguous key disables joining for every
    /// compatible stream. A surface without a stream is complete in its event and restores alone.
    ///
    /// The default treats every surface of [`Self::sse_event_pii_surfaces`] as complete, which
    /// never restores a split token: the client sees the pseudonym, never a wrong raw value.
    fn sse_event_stream_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<SseStreamSurface<'a>> {
        self.sse_event_pii_surfaces(event)
            .into_iter()
            .map(SseStreamSurface::complete)
            .collect()
    }

    /// Reports whether `request` asked the model for JSON output (a JSON mode or a JSON
    /// schema), which makes every [`SurfaceSyntax::ModelOutput`] response surface a JSON
    /// document. The default, `false`, restores model output as free text.
    fn requests_json_output(&self, _request: &Value) -> bool {
        false
    }
}

/// The syntax of a surface's text, which decides how restore writes a raw value into it.
///
/// Only restore reads it: request protection scans a surface's text as it stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceSyntax {
    /// Free text. Restore writes the raw value byte for byte.
    Text,
    /// A serialized JSON document, such as tool-call `arguments`. A token can only stand inside
    /// one of its string literals, so restore JSON-escapes the raw value; a verbatim `"`, `\`,
    /// or control character would break the document or silently change the parsed value. A
    /// value that was itself captured inside a JSON string literal is already escaped and is
    /// written as it is.
    Json,
    /// Model-authored response text: [`Self::Json`] when the request asked for JSON output
    /// ([`ProviderAdapter::requests_json_output`]), [`Self::Text`] otherwise.
    ModelOutput,
}

#[non_exhaustive]
pub struct PiiSurface<'a> {
    pub field_path: CoveragePath,
    /// Validated scalar location. Debug/Display of the path hide user-authored keys.
    pub field: Option<gaze_types::FieldProvenance>,
    pub text: &'a mut String,
    pub syntax: SurfaceSyntax,
}

impl<'a> PiiSurface<'a> {
    /// Construct a surface without structured evidence (for custom adapters).
    pub fn new(
        field_path: impl Into<CoveragePath>,
        text: &'a mut String,
        syntax: SurfaceSyntax,
    ) -> Self {
        Self {
            field_path: field_path.into(),
            field: None,
            text,
            syntax,
        }
    }
}

/// Request string rewrite targets. Numeric leaves remain untouched; numeric
/// admission is still proved by the residual scan. No unused number inventory is retained.
#[derive(Default)]
#[non_exhaustive]
pub struct RequestSurfaces<'a> {
    pub strings: Vec<PiiSurface<'a>>,
}

impl<'a> From<Vec<PiiSurface<'a>>> for RequestSurfaces<'a> {
    fn from(strings: Vec<PiiSurface<'a>>) -> Self {
        Self { strings }
    }
}

impl<'a> IntoIterator for RequestSurfaces<'a> {
    type Item = PiiSurface<'a>;
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.strings.into_iter()
    }
}

impl<'a> RequestSurfaces<'a> {
    pub fn iter(&self) -> std::slice::Iter<'_, PiiSurface<'a>> {
        self.strings.iter()
    }
    pub fn len(&self) -> usize {
        self.strings.len()
    }
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
    pub(crate) fn push(&mut self, surface: PiiSurface<'a>) {
        self.strings.push(surface);
    }
}

/// A legacy coverage spelling with shared ancestor storage. Debug hides keys.
/// Equality and hashing do not materialize the spelling.
#[derive(Clone)]
#[non_exhaustive]
pub struct CoveragePath {
    chunks: gaze_types::FieldPath,
    rendered: std::sync::OnceLock<String>,
    prefix_hash: std::collections::hash_map::DefaultHasher,
    byte_len: usize,
}

impl Default for CoveragePath {
    fn default() -> Self {
        static HASHER: std::sync::OnceLock<std::collections::hash_map::RandomState> =
            std::sync::OnceLock::new();
        Self {
            chunks: gaze_types::FieldPath::default(),
            rendered: std::sync::OnceLock::new(),
            prefix_hash: std::hash::BuildHasher::build_hasher(
                HASHER.get_or_init(std::collections::hash_map::RandomState::new),
            ),
            byte_len: 0,
        }
    }
}

impl CoveragePath {
    fn append(&self, chunk: String) -> Self {
        let mut prefix_hash = self.prefix_hash.clone();
        // Extend the ancestor's keyed hash state, visiting only new bytes. Writing
        // bytes individually keeps flat and segmented spellings equivalent.
        for byte in chunk.bytes() {
            std::hash::Hasher::write_u8(&mut prefix_hash, byte);
        }
        let byte_len = self.byte_len + chunk.len();
        Self {
            chunks: self.chunks.child(chunk),
            rendered: std::sync::OnceLock::new(),
            prefix_hash,
            byte_len,
        }
    }
    fn fingerprint(&self) -> u64 {
        std::hash::Hasher::finish(&self.prefix_hash)
    }
    /// Materialize the legacy spelling only when a trusted caller needs it.
    /// Calling this on every descendant copies shared ancestor bytes per call.
    pub fn as_str(&self) -> &str {
        self.rendered
            .get_or_init(|| self.chunks.segments().concat())
    }
    fn is_empty(&self) -> bool {
        self.byte_len == 0
    }
    pub(crate) fn member(&self, key: &str) -> Self {
        self.append(if self.is_empty() {
            key.to_owned()
        } else {
            format!(".{key}")
        })
    }
    pub(crate) fn index(&self, index: usize) -> Self {
        self.append(format!("[{index}]"))
    }
}

impl From<String> for CoveragePath {
    fn from(value: String) -> Self {
        Self::default().append(value)
    }
}
impl From<&str> for CoveragePath {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl PartialEq for CoveragePath {
    fn eq(&self, other: &Self) -> bool {
        if self.byte_len != other.byte_len || self.fingerprint() != other.fingerprint() {
            return false;
        }
        let left = self.chunks.segments();
        let right = other.chunks.segments();
        if left
            .iter()
            .map(|s| s.len())
            .eq(right.iter().map(|s| s.len()))
        {
            return left == right;
        }
        left.iter()
            .flat_map(|chunk| chunk.bytes())
            .eq(right.iter().flat_map(|chunk| chunk.bytes()))
    }
}
impl Eq for CoveragePath {}
impl std::hash::Hash for CoveragePath {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.fingerprint());
        state.write_usize(self.byte_len);
    }
}
impl std::fmt::Debug for CoveragePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoveragePath").finish_non_exhaustive()
    }
}

/// Keeps coverage spelling separate from unescaped JSON-pointer segments.
#[derive(Clone)]
pub(crate) struct SurfacePath {
    coverage: CoveragePath,
    segments: gaze_types::FieldPath,
}

impl SurfacePath {
    pub(crate) fn root(key: &str) -> Self {
        Self {
            coverage: key.into(),
            segments: gaze_types::FieldPath::new([key.to_owned()]),
        }
    }
    pub(crate) fn member(&self, key: &str) -> Self {
        Self {
            coverage: self.coverage.member(key),
            segments: self.segments.child(key.to_owned()),
        }
    }
    pub(crate) fn index(&self, index: usize) -> Self {
        Self {
            coverage: self.coverage.index(index),
            segments: self.segments.child(index.to_string()),
        }
    }
    fn provenance(&self, kind: gaze_types::JsonScalarType) -> gaze_types::FieldProvenance {
        gaze_types::FieldProvenance::new(
            self.segments.clone(),
            kind,
            gaze_types::FieldEvidence::ValidatedParse,
        )
    }
    pub(crate) fn surface<'a>(self, text: &'a mut String, syntax: SurfaceSyntax) -> PiiSurface<'a> {
        let field = Some(self.provenance(gaze_types::JsonScalarType::String));
        PiiSurface {
            field_path: self.coverage,
            field,
            text,
            syntax,
        }
    }
}

/// A logical SSE value, including any missing identity fields that make joining ambiguous.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SseStreamKey {
    kind: String,
    indices: Vec<Option<u64>>,
}

impl SseStreamKey {
    /// Identifies a surface kind and its provider indices, ordered from outermost to innermost.
    /// Missing or invalid required indices must be `None`, never an array position or a guess.
    /// Each kind must use the same number and order of indices throughout a response.
    pub fn new(kind: impl Into<String>, indices: impl IntoIterator<Item = Option<u64>>) -> Self {
        Self {
            kind: kind.into(),
            indices: indices.into_iter().collect(),
        }
    }

    pub(crate) fn is_ambiguous(&self) -> bool {
        self.indices.iter().any(Option::is_none)
    }

    /// Unknown indices may refer to any stream whose known indices agree.
    pub(crate) fn could_continue(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.indices.len() == other.indices.len()
            && self
                .indices
                .iter()
                .zip(&other.indices)
                .all(|(a, b)| match (a, b) {
                    (Some(a), Some(b)) => a == b,
                    _ => true,
                })
    }
}

/// A response surface of one SSE event and the logical stream it is a fragment of.
///
/// See [`ProviderAdapter::sse_event_stream_surfaces`].
pub struct SseStreamSurface<'a> {
    /// Names the logical value this fragment continues, unique within one response. Two
    /// fragments with the same key are concatenated in event order before restore, so a key
    /// must never be shared by two values: parallel tool calls or alternative choices each need
    /// their own. Derive it from the provider's own indices, never from an array position that
    /// restarts in every event. All fragments of one stream must share one [`SurfaceSyntax`].
    ///
    /// A key with a missing index disables joining for every compatible stream in the
    /// response. Those fragments restore individually, preserving their original wire order.
    /// `None` marks a surface that is complete in its event, not an unidentified fragment.
    pub stream: Option<SseStreamKey>,
    pub surface: PiiSurface<'a>,
}

impl<'a> SseStreamSurface<'a> {
    /// A surface that is complete in its own event.
    pub fn complete(surface: PiiSurface<'a>) -> Self {
        Self {
            stream: None,
            surface,
        }
    }

    /// A fragment of the logical stream `stream`.
    pub fn fragment(stream: SseStreamKey, surface: PiiSurface<'a>) -> Self {
        Self {
            stream: Some(stream),
            surface,
        }
    }
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
    surfaces: &mut RequestSurfaces<'a>,
    path: SurfacePath,
    value: &'a mut Value,
    syntax: SurfaceSyntax,
) {
    if let Value::String(text) = value {
        surfaces.push(path.surface(text, syntax));
    }
}

/// Walk decoded string leaves. Numbers are not rewrite targets or retained metadata.
pub(crate) fn walk_all_scalars<'a>(
    surfaces: &mut RequestSurfaces<'a>,
    path: SurfacePath,
    value: &'a mut Value,
) {
    match value {
        Value::String(text) => surfaces.push(path.surface(text, SurfaceSyntax::Text)),

        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                walk_all_scalars(surfaces, path.index(index), item);
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                walk_all_scalars(surfaces, path.member(key), child);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

pub(crate) fn push_text_blocks<'a>(
    surfaces: &mut RequestSurfaces<'a>,
    path: &SurfacePath,
    value: &'a mut Value,
    syntax: SurfaceSyntax,
) {
    match value {
        Value::String(text) => surfaces.push(path.clone().surface(text, syntax)),
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                let path = path.index(index);
                match item {
                    Value::String(text) => surfaces.push(path.surface(text, syntax)),
                    Value::Object(map) => {
                        if matches!(map.get("type"), Some(Value::String(kind))
                            if kind == "text" || kind == "input_text" || kind == "output_text")
                        {
                            if let Some(Value::String(text)) = map.get_mut("text") {
                                surfaces.push(path.member("text").surface(text, syntax));
                            }
                        } else if matches!(map.get("type"), Some(Value::String(kind)) if kind == "message")
                        {
                            if let Some(content) = map.get_mut("content") {
                                push_text_blocks(
                                    surfaces,
                                    &path.member("content"),
                                    content,
                                    syntax,
                                );
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

#[cfg(test)]
mod path_storage_tests {
    use super::*;
    use crate::adapters::OpenAiAdapter;

    #[test]
    fn field_provenance_wide_legacy_paths_retain_linear_bytes() {
        let adapter = OpenAiAdapter::new(url::Url::parse("http://127.0.0.1:1").unwrap());
        for leaf in [serde_json::json!(0), serde_json::json!("x")] {
            let key = "k".repeat(1024 * 1024);
            let mut body = serde_json::json!({"metadata": {key: vec![leaf.clone(); 2000]}});
            let input_bytes = serde_json::to_vec(&body).unwrap().len();
            let surfaces = adapter.request_pii_surfaces(&mut body);
            assert_eq!(surfaces.len(), if leaf.is_number() { 0 } else { 2000 });
            let retained =
                gaze::test_support::retained_path_bytes(surfaces.iter().flat_map(|surface| {
                    [
                        &surface.field.as_ref().unwrap().path,
                        &surface.field_path.chunks,
                    ]
                }));
            // Two spellings, one allocation per unique key plus small per-leaf indices.
            assert!(
                retained <= 3 * input_bytes,
                "retained path bytes: {retained}"
            );
            assert!(surfaces
                .iter()
                .all(|surface| surface.field_path.rendered.get().is_none()));
        }
    }

    #[test]
    fn coverage_equality_and_hash_ignore_chunk_boundaries() {
        use std::collections::HashSet;
        let joined = CoveragePath::from("metadata").member("rows").index(2);
        let flat = CoveragePath::from("metadata.rows[2]");
        assert_eq!(joined, flat);
        assert!(HashSet::from([joined]).contains(&flat));
        assert_eq!(flat.as_str(), "metadata.rows[2]");
        assert!(!format!("{flat:?}").contains("metadata"));
    }
}
