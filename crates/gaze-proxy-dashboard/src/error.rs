use std::fmt;

/// Closed dashboard error codes. No variant contains source text or sensitive context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DashboardErrorCode {
    /// Dashboard capture was not enabled at immutable startup.
    DisabledByConfiguration,
    /// A configured limit was outside the hard security ceiling.
    InvalidLimit,
    /// Owner capture was selected without its independent acknowledgement.
    MissingOwnerAcknowledgement,
    /// A bind target was not a literal IPv4 loopback address with port zero.
    InvalidLoopbackBind,
    /// A local inherited handle failed provenance validation.
    InvalidInheritedHandle,
    /// No-dump readiness was unavailable or could not be verified.
    NoDumpUnavailable,
    /// Canonical pairing delivery or acknowledgement failed.
    PairingFailed,
    /// The pending consumer could not be constructed.
    ConsumerUnavailable,
    /// Activation failed after inspection installation.
    ActivationFailed,
    /// Bounded ingress was full.
    IngressFull,
    /// Bounded ingress was closed for purge or disable.
    IngressClosed,
    /// A typed IPC frame was malformed or exceeded its cap.
    IpcRejected,
    /// A one-request HTTP/1.1 connection failed the raw gate.
    HttpRejected,
    /// Authentication, page-session, or CSRF validation failed.
    Unauthorized,
    /// A purge could not complete under its matching registration guard.
    PurgeFailed,
    /// A fatal child/runtime fault permanently disabled the dashboard.
    FatalDisabled,
}

impl DashboardErrorCode {
    /// Stable sanitized wire/log spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DisabledByConfiguration => "dashboard_disabled_by_configuration",
            Self::InvalidLimit => "dashboard_invalid_limit",
            Self::MissingOwnerAcknowledgement => "dashboard_missing_owner_acknowledgement",
            Self::InvalidLoopbackBind => "dashboard_invalid_loopback_bind",
            Self::InvalidInheritedHandle => "dashboard_invalid_inherited_handle",
            Self::NoDumpUnavailable => "dashboard_no_dump_unavailable",
            Self::PairingFailed => "dashboard_pairing_failed",
            Self::ConsumerUnavailable => "dashboard_consumer_unavailable",
            Self::ActivationFailed => "dashboard_activation_failed",
            Self::IngressFull => "dashboard_ingress_full",
            Self::IngressClosed => "dashboard_ingress_closed",
            Self::IpcRejected => "dashboard_ipc_rejected",
            Self::HttpRejected => "dashboard_http_rejected",
            Self::Unauthorized => "dashboard_unauthorized",
            Self::PurgeFailed => "dashboard_purge_failed",
            Self::FatalDisabled => "dashboard_fatal_disabled",
        }
    }
}

/// Sanitized dashboard failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DashboardError {
    code: DashboardErrorCode,
}

impl DashboardError {
    /// Creates a closed error from its stable code.
    #[must_use]
    pub const fn new(code: DashboardErrorCode) -> Self {
        Self { code }
    }

    /// Returns the closed code.
    #[must_use]
    pub const fn code(self) -> DashboardErrorCode {
        self.code
    }
}

impl fmt::Display for DashboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for DashboardError {}

/// Sanitized host-visible status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardStatus {
    /// Child preparation is in progress and no inspection sink exists.
    Preparing,
    /// Pairing delivery was acknowledged; activation has not committed.
    Paired,
    /// The inspection consumer is active.
    Active,
    /// The runtime is purging under a registration-bound guard.
    Purging,
    /// The dashboard was permanently disabled and cannot resurrect.
    Disabled(DashboardErrorCode),
    /// Shutdown completed after child reap.
    Stopped,
}
