use std::fmt;
use std::path::PathBuf;

use http::StatusCode;

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

/// Sanitized direct-profile failure.
///
/// `Display`, `Debug`, and the source chain deliberately expose only its closed code and phase.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DirectProxyError {
    code: ProxyErrorCode,
    phase: ProxyErrorPhase,
    retry_after_seconds: Option<u32>,
}

impl DirectProxyError {
    #[must_use]
    pub const fn new(code: ProxyErrorCode, phase: ProxyErrorPhase) -> Self {
        Self {
            code,
            phase,
            retry_after_seconds: None,
        }
    }

    #[must_use]
    pub(crate) const fn with_retry_after_seconds(mut self, seconds: Option<u32>) -> Self {
        self.retry_after_seconds = seconds;
        self
    }

    #[must_use]
    pub const fn code(self) -> ProxyErrorCode {
        self.code
    }

    #[must_use]
    pub const fn phase(self) -> ProxyErrorPhase {
        self.phase
    }

    #[must_use]
    pub const fn http_status(self) -> StatusCode {
        self.code.http_status()
    }

    #[must_use]
    pub const fn retry_after_seconds(self) -> Option<u32> {
        self.retry_after_seconds
    }

    #[must_use]
    pub const fn sse_error_frame(self) -> &'static [u8] {
        crate::server::DIRECT_PROXY_ERROR_FRAME
    }
}

impl fmt::Debug for DirectProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DirectProxyError")
            .field("code", &self.code)
            .field("phase", &self.phase)
            .finish()
    }
}

impl fmt::Display for DirectProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "proxy_{:?}_{:?}", self.phase, self.code)
    }
}

impl std::error::Error for DirectProxyError {}
