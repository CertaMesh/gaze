use std::fmt;
use std::path::PathBuf;

use http::StatusCode;

use http::Method;
use thiserror::Error;
use url::Url;

use crate::codec::OpaqueCarrierLocation;

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
    /// A legacy-path request carried PII in a position no adapter surface covered.
    ///
    /// Raised by the outbound request residual re-scan, which fails closed rather than
    /// silently rewriting provider-owned fields. The variant carries the JSON field path
    /// ONLY -- never the detected value, nor any substring of it -- so neither `Display`,
    /// `Debug`, nor any log line derived from them can egress protected bytes.
    #[error("unsurfaced pii at {field_path}")]
    UnsurfacedPii { field_path: String },
    /// A request reached the legacy path under a contract that claims codec-proved coverage.
    ///
    /// The contract and the router disagree, so no coverage mechanism is known to have run.
    /// Proxying under an unproven coverage claim is exactly the failure this policy exists to
    /// prevent, so the request is refused.
    #[error("adapter coverage contract is unproven on the legacy path")]
    UnprovenCoverage,
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

/// Closed failure codes used by the direct proxy profile.
///
/// Unlike [`ProxyError`], this surface never stores provider, request, URL, parser, transport, or
/// operating-system error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyErrorCode {
    RouteRejected,
    HeaderRejected,
    InvalidUpstreamHeader,
    SessionIdentityRequired,
    UnexpectedSessionHeader,
    SessionExpired,
    SessionGenerationConflict,
    SessionCommitFailure,
    PrincipalRequired,
    PrincipalRejected,
    RegistryCapacity,
    InvalidUpstreamUrl,
    ProxyConfiguration,
    UnsupportedContentEncoding,
    InvalidFraming,
    RequestBodyLimitExceeded,
    ResponseBodyLimitExceeded,
    HeaderLimitExceeded,
    InvalidRequestFormat,
    InvalidUpstreamResponseFormat,
    DuplicateObjectKey,
    InternalCoverageFailure,
    ControlWouldMutate,
    OpaqueMediaUninspected,
    InvalidProvenance,
    InvalidToken,
    SignedMutationRequired,
    SignedSurfaceMalformed,
    InvalidSseLifecycle,
    InvalidContentBlockIndex,
    ConnectTimeout,
    RequestTimeout,
    TotalTimeout,
    UpstreamRedirect,
    UpstreamBadRequest,
    UpstreamUnauthorized,
    UpstreamForbidden,
    UpstreamNotFound,
    UpstreamConflict,
    UpstreamPayloadTooLarge,
    UpstreamRateLimited,
    UpstreamClientFailure,
    UpstreamUnavailable,
    UpstreamServerFailure,
    UpstreamUnreachable,
    UpstreamProtocol,
    InspectionInternal,
    InvalidStateTransition,
}

impl ProxyErrorCode {
    /// Constructs a sanitized direct-profile error at a closed phase.
    #[must_use]
    pub const fn error(self, phase: ProxyErrorPhase) -> DirectProxyError {
        DirectProxyError::new(self, phase)
    }

    /// Returns the single reviewed downstream status for this code.
    #[must_use]
    pub const fn http_status(self) -> StatusCode {
        match self {
            Self::RouteRejected => StatusCode::NOT_FOUND,
            Self::HeaderRejected
            | Self::SessionIdentityRequired
            | Self::UnexpectedSessionHeader
            | Self::InvalidRequestFormat
            | Self::DuplicateObjectKey => StatusCode::BAD_REQUEST,
            Self::PrincipalRequired | Self::UpstreamUnauthorized => StatusCode::UNAUTHORIZED,
            Self::PrincipalRejected | Self::UpstreamForbidden => StatusCode::FORBIDDEN,
            Self::SessionExpired => StatusCode::GONE,
            Self::SessionGenerationConflict | Self::UpstreamConflict => StatusCode::CONFLICT,
            Self::RegistryCapacity
            | Self::InvalidUpstreamUrl
            | Self::ProxyConfiguration
            | Self::UpstreamUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::RequestBodyLimitExceeded | Self::UpstreamPayloadTooLarge => {
                StatusCode::PAYLOAD_TOO_LARGE
            }
            Self::ControlWouldMutate | Self::OpaqueMediaUninspected => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::ConnectTimeout | Self::RequestTimeout | Self::TotalTimeout => {
                StatusCode::GATEWAY_TIMEOUT
            }
            Self::UpstreamBadRequest => StatusCode::BAD_REQUEST,
            Self::UpstreamNotFound => StatusCode::NOT_FOUND,
            Self::UpstreamRateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::SessionCommitFailure
            | Self::InspectionInternal
            | Self::InvalidStateTransition => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InvalidUpstreamHeader
            | Self::UnsupportedContentEncoding
            | Self::InvalidFraming
            | Self::ResponseBodyLimitExceeded
            | Self::HeaderLimitExceeded
            | Self::InvalidUpstreamResponseFormat
            | Self::InternalCoverageFailure
            | Self::InvalidProvenance
            | Self::InvalidToken
            | Self::SignedMutationRequired
            | Self::SignedSurfaceMalformed
            | Self::InvalidSseLifecycle
            | Self::InvalidContentBlockIndex
            | Self::UpstreamRedirect
            | Self::UpstreamClientFailure
            | Self::UpstreamServerFailure
            | Self::UpstreamUnreachable
            | Self::UpstreamProtocol => StatusCode::BAD_GATEWAY,
        }
    }
}

/// Closed processing phase carried by a direct-profile failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyErrorPhase {
    Ingress,
    RequestValidation,
    Session,
    RequestTransform,
    UpstreamConfiguration,
    UpstreamConnect,
    UpstreamHeaders,
    UpstreamBody,
    ResponseValidation,
    ResponseTransform,
    ResponseReplay,
    Inspection,
    Framework,
}

/// Structural reason a request header was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeaderRejectionReason {
    NameNotAllowlisted,
    EmptyValue,
    DuplicateSingleton,
}

impl HeaderRejectionReason {
    /// Returns the stable downstream identifier for this rejection cause.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NameNotAllowlisted => "name_not_allowlisted",
            Self::EmptyValue => "empty_value",
            Self::DuplicateSingleton => "duplicate_singleton",
        }
    }
}

const MAX_STRUCTURAL_HEADER_NAME_BYTES: usize = 128;
const PACKED_STRUCTURAL_HEADER_NAME_BYTES: usize = MAX_STRUCTURAL_HEADER_NAME_BYTES * 6 / 8;

// `HeaderName::as_str()` uses the 51 lowercase HTTP token characters. Six-bit packing retains
// every reviewed 128-byte header name exactly while keeping the closed error small and `Copy`.
#[derive(Clone, Copy, Eq, PartialEq)]
struct HeaderRejectionIdentifier {
    packed_name: [u8; PACKED_STRUCTURAL_HEADER_NAME_BYTES],
    name_length: u8,
    reason: HeaderRejectionReason,
}

impl HeaderRejectionIdentifier {
    fn new(name: &str, reason: HeaderRejectionReason) -> Option<Self> {
        let name_length = u8::try_from(name.len()).ok()?;
        if name.len() > MAX_STRUCTURAL_HEADER_NAME_BYTES {
            return None;
        }
        let mut packed_name = [0; PACKED_STRUCTURAL_HEADER_NAME_BYTES];
        for (index, byte) in name.bytes().enumerate() {
            let code = encode_header_name_byte(byte)?;
            let bit_offset = index * 6;
            let byte_offset = bit_offset / 8;
            let shift = bit_offset % 8;
            packed_name[byte_offset] |= code << shift;
            if shift > 2 {
                packed_name[byte_offset + 1] |= code >> (8 - shift);
            }
        }
        Some(Self {
            packed_name,
            name_length,
            reason,
        })
    }

    fn name(self) -> String {
        let mut name = String::with_capacity(usize::from(self.name_length));
        for index in 0..usize::from(self.name_length) {
            let bit_offset = index * 6;
            let byte_offset = bit_offset / 8;
            let shift = bit_offset % 8;
            let mut code = self.packed_name[byte_offset] >> shift;
            if shift > 2 {
                code |= self.packed_name[byte_offset + 1] << (8 - shift);
            }
            name.push(char::from(
                decode_header_name_byte(code & 0x3f)
                    .expect("packed header names contain only validated token bytes"),
            ));
        }
        name
    }
}

fn encode_header_name_byte(byte: u8) -> Option<u8> {
    match byte {
        b'a'..=b'z' => Some(byte - b'a'),
        b'0'..=b'9' => Some(26 + byte - b'0'),
        b'!' => Some(36),
        b'#' => Some(37),
        b'$' => Some(38),
        b'%' => Some(39),
        b'&' => Some(40),
        b'\'' => Some(41),
        b'*' => Some(42),
        b'+' => Some(43),
        b'-' => Some(44),
        b'.' => Some(45),
        b'^' => Some(46),
        b'_' => Some(47),
        b'`' => Some(48),
        b'|' => Some(49),
        b'~' => Some(50),
        _ => None,
    }
}

fn decode_header_name_byte(code: u8) -> Option<u8> {
    match code {
        0..=25 => Some(b'a' + code),
        26..=35 => Some(b'0' + code - 26),
        36 => Some(b'!'),
        37 => Some(b'#'),
        38 => Some(b'$'),
        39 => Some(b'%'),
        40 => Some(b'&'),
        41 => Some(b'\''),
        42 => Some(b'*'),
        43 => Some(b'+'),
        44 => Some(b'-'),
        45 => Some(b'.'),
        46 => Some(b'^'),
        47 => Some(b'_'),
        48 => Some(b'`'),
        49 => Some(b'|'),
        50 => Some(b'~'),
        _ => None,
    }
}

impl fmt::Debug for HeaderRejectionIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Header")
            .field("name", &self.name())
            .field("reason", &self.reason)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectProxyErrorIdentifier {
    Header(HeaderRejectionIdentifier),
    OpaqueCarrier(OpaqueCarrierLocation),
}

/// Sanitized direct-profile failure.
///
/// `Display`, `Debug`, and the source chain expose only its closed code, phase, and reviewed
/// structural identifiers. Header values and carrier bytes are never retained.
///
/// This follows the field-path-only precedent established for [`ProxyError::UnsurfacedPii`] in
/// #400: report the structural location that makes a rejection actionable, never the value.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DirectProxyError {
    code: ProxyErrorCode,
    phase: ProxyErrorPhase,
    retry_after_seconds: Option<u32>,
    identifier: Option<DirectProxyErrorIdentifier>,
}

impl DirectProxyError {
    #[must_use]
    pub const fn new(code: ProxyErrorCode, phase: ProxyErrorPhase) -> Self {
        Self {
            code,
            phase,
            retry_after_seconds: None,
            identifier: None,
        }
    }

    #[must_use]
    pub(crate) const fn with_retry_after_seconds(mut self, seconds: Option<u32>) -> Self {
        self.retry_after_seconds = seconds;
        self
    }

    pub(crate) fn with_header_rejection(
        mut self,
        name: &str,
        reason: HeaderRejectionReason,
    ) -> Self {
        self.identifier =
            HeaderRejectionIdentifier::new(name, reason).map(DirectProxyErrorIdentifier::Header);
        self
    }

    pub(crate) fn with_opaque_carrier(mut self, opaque_carrier: OpaqueCarrierLocation) -> Self {
        self.identifier = Some(DirectProxyErrorIdentifier::OpaqueCarrier(opaque_carrier));
        self
    }

    #[must_use]
    pub const fn code(&self) -> ProxyErrorCode {
        self.code
    }

    #[must_use]
    pub const fn phase(&self) -> ProxyErrorPhase {
        self.phase
    }

    #[must_use]
    pub const fn http_status(&self) -> StatusCode {
        self.code.http_status()
    }

    #[must_use]
    pub const fn retry_after_seconds(&self) -> Option<u32> {
        self.retry_after_seconds
    }

    /// Returns the rejected header name without retaining or exposing its value.
    #[must_use]
    pub fn header_name(&self) -> Option<String> {
        match &self.identifier {
            Some(DirectProxyErrorIdentifier::Header(identifier)) => Some(identifier.name()),
            _ => None,
        }
    }

    /// Returns the structural header-rejection cause.
    #[must_use]
    pub const fn header_rejection_reason(&self) -> Option<HeaderRejectionReason> {
        match &self.identifier {
            Some(DirectProxyErrorIdentifier::Header(identifier)) => Some(identifier.reason),
            _ => None,
        }
    }

    /// Returns only the opaque carrier's scheme and structural location.
    #[must_use]
    pub const fn opaque_carrier(&self) -> Option<OpaqueCarrierLocation> {
        match &self.identifier {
            Some(DirectProxyErrorIdentifier::OpaqueCarrier(location)) => Some(*location),
            _ => None,
        }
    }

    #[must_use]
    pub const fn sse_error_frame(&self) -> &'static [u8] {
        crate::server::DIRECT_PROXY_ERROR_FRAME
    }
}

impl fmt::Debug for DirectProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("DirectProxyError");
        debug.field("code", &self.code).field("phase", &self.phase);
        if let Some(identifier) = &self.identifier {
            debug.field("identifier", identifier);
        }
        debug.finish()
    }
}

impl fmt::Display for DirectProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "proxy_{:?}_{:?}", self.phase, self.code)
    }
}

impl std::error::Error for DirectProxyError {}
