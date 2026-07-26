use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use gaze_types::inspection::{CaptureDomainsV1, DashboardCaptureDescriptorV1};

use crate::{DashboardError, DashboardErrorCode};

/// Hard retained logical-event ceiling.
pub const HARD_MAX_EVENTS: usize = 1_024;
/// Hard dashboard-owned byte ceiling.
pub const HARD_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Hard retention ceiling.
pub const HARD_MAX_TTL: Duration = Duration::from_secs(60 * 60);
/// Hard parent-ingress ceiling.
pub const HARD_MAX_INGRESS: usize = 4_096;
/// Hard encoded IPC-frame ceiling.
pub const HARD_MAX_IPC_FRAME: usize = 4 * 1024 * 1024;

/// Independent acknowledgement for OwnerRaw capture.
pub struct OwnerRawRiskAcknowledgement(());

impl OwnerRawRiskAcknowledgement {
    /// Explicitly acknowledges that OwnerRaw bytes may contain PII.
    #[must_use]
    pub const fn acknowledge_pii_risk() -> Self {
        Self(())
    }
}

/// Independent acknowledgement for OwnerRestored capture.
pub struct OwnerRestoredRiskAcknowledgement(());

impl OwnerRestoredRiskAcknowledgement {
    /// Explicitly acknowledges that OwnerRestored bytes are re-identified.
    #[must_use]
    pub const fn acknowledge_reidentification_risk() -> Self {
        Self(())
    }
}

/// Immutable launch-time payload acceptance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DashboardPayloadAcceptance {
    owner_raw: bool,
    owner_restored: bool,
}

impl DashboardPayloadAcceptance {
    /// ProviderVisible is the only dashboard-on baseline.
    #[must_use]
    pub const fn provider_visible() -> Self {
        Self {
            owner_raw: false,
            owner_restored: false,
        }
    }

    /// Adds independently acknowledged OwnerRaw capture.
    #[must_use]
    pub fn with_owner_raw(self, _ack: OwnerRawRiskAcknowledgement) -> Self {
        Self {
            owner_raw: true,
            ..self
        }
    }

    /// Adds independently acknowledged OwnerRestored capture.
    #[must_use]
    pub fn with_owner_restored(self, _ack: OwnerRestoredRiskAcknowledgement) -> Self {
        Self {
            owner_restored: true,
            ..self
        }
    }

    /// Returns the exact immutable inspection descriptor.
    #[must_use]
    pub const fn descriptor(self) -> DashboardCaptureDescriptorV1 {
        let domains = match (self.owner_raw, self.owner_restored) {
            (false, false) => CaptureDomainsV1::ProviderVisible,
            (true, false) => CaptureDomainsV1::OwnerRawAndProviderVisible,
            (false, true) => CaptureDomainsV1::ProviderVisibleAndOwnerRestored,
            (true, true) => CaptureDomainsV1::All,
        };
        DashboardCaptureDescriptorV1::new(domains)
    }
}

/// Bounded memory/TTL limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionLimits {
    max_events: usize,
    max_bytes: usize,
    ttl: Duration,
}

impl RetentionLimits {
    /// Conservative defaults: 64 logical events, 4 MiB, five minutes.
    #[must_use]
    pub const fn conservative() -> Self {
        Self {
            max_events: 64,
            max_bytes: 4 * 1024 * 1024,
            ttl: Duration::from_secs(5 * 60),
        }
    }

    /// Creates retention limits under hard ceilings.
    pub fn new(max_events: usize, max_bytes: usize, ttl: Duration) -> Result<Self, DashboardError> {
        if max_events == 0
            || max_events > HARD_MAX_EVENTS
            || max_bytes == 0
            || max_bytes > HARD_MAX_BYTES
            || ttl.is_zero()
            || ttl > HARD_MAX_TTL
        {
            return Err(DashboardError::new(DashboardErrorCode::InvalidLimit));
        }
        Ok(Self {
            max_events,
            max_bytes,
            ttl,
        })
    }

    pub(crate) const fn max_events(self) -> usize {
        self.max_events
    }

    pub(crate) const fn max_bytes(self) -> usize {
        self.max_bytes
    }

    pub(crate) const fn ttl(self) -> Duration {
        self.ttl
    }
}

/// Bounded authenticated-client and response limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientLimits {
    page_sessions: usize,
    followers: usize,
    active_payload_responses: usize,
}

impl ClientLimits {
    /// Conservative client limits.
    #[must_use]
    pub const fn conservative() -> Self {
        Self {
            page_sessions: 4,
            followers: 4,
            active_payload_responses: 2,
        }
    }

    pub(crate) const fn page_sessions(self) -> usize {
        self.page_sessions
    }

    pub(crate) const fn followers(self) -> usize {
        self.followers
    }

    pub(crate) const fn active_http_connections(self) -> usize {
        self.page_sessions
    }

    pub(crate) const fn active_payload_responses(self) -> usize {
        self.active_payload_responses
    }
}

/// Bounded parent ingress and IPC limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcLimits {
    ingress_items: usize,
    frame_bytes: usize,
}

impl IpcLimits {
    /// Conservative ingress/frame limits.
    #[must_use]
    pub const fn conservative() -> Self {
        Self {
            ingress_items: 64,
            frame_bytes: 1024 * 1024,
        }
    }

    /// Creates limits under fixed hard ceilings.
    pub fn new(ingress_items: usize, frame_bytes: usize) -> Result<Self, DashboardError> {
        if ingress_items == 0
            || ingress_items > HARD_MAX_INGRESS
            || frame_bytes == 0
            || frame_bytes > HARD_MAX_IPC_FRAME
        {
            return Err(DashboardError::new(DashboardErrorCode::InvalidLimit));
        }
        Ok(Self {
            ingress_items,
            frame_bytes,
        })
    }

    pub(crate) const fn ingress_items(self) -> usize {
        self.ingress_items
    }

    pub(crate) const fn frame_bytes(self) -> usize {
        self.frame_bytes
    }
}

/// Literal-loopback, port-zero bind policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoopbackBind {
    address: Ipv4Addr,
    configured: bool,
}

impl LoopbackBind {
    /// Selects a fresh CSPRNG-derived literal address in 127/8 and port zero.
    pub fn fresh_ephemeral_v4() -> Result<Self, DashboardError> {
        let mut octets = [0_u8; 3];
        getrandom::fill(&mut octets)
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidLoopbackBind))?;
        Ok(Self {
            address: Ipv4Addr::new(127, octets[0], octets[1], octets[2]),
            configured: false,
        })
    }

    /// Accepts only configured literal IPv4 loopback with port zero.
    pub fn configured(authority: SocketAddrV4) -> Result<Self, DashboardError> {
        if !authority.ip().is_loopback() || authority.port() != 0 {
            return Err(DashboardError::new(DashboardErrorCode::InvalidLoopbackBind));
        }
        Ok(Self {
            address: *authority.ip(),
            configured: true,
        })
    }

    pub(crate) const fn socket_addr(self) -> SocketAddrV4 {
        SocketAddrV4::new(self.address, 0)
    }

    pub(crate) const fn matches_authority(self, authority: SocketAddrV4) -> bool {
        authority.ip().to_bits() == self.address.to_bits() && authority.port() != 0
    }

    /// Reports whether origin reuse was explicitly configured.
    #[must_use]
    pub const fn origin_reuse_warning(self) -> bool {
        self.configured
    }
}

/// Default-off immutable startup configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DashboardStartupConfig {
    /// Construct no dashboard state, listener, token, sink, or entropy.
    #[default]
    Off,
    /// Explicit dashboard-on configuration.
    Enabled {
        /// Startup-fixed capture acceptance.
        acceptance: DashboardPayloadAcceptance,
        /// Literal-loopback bind policy.
        bind: LoopbackBind,
        /// Memory/TTL limits.
        retention: RetentionLimits,
        /// Client/response limits.
        clients: ClientLimits,
        /// Ingress/frame limits.
        ipc: IpcLimits,
    },
}

impl DashboardStartupConfig {
    /// Creates an explicit enabled configuration with conservative limits.
    #[must_use]
    pub const fn enabled(acceptance: DashboardPayloadAcceptance, bind: LoopbackBind) -> Self {
        Self::Enabled {
            acceptance,
            bind,
            retention: RetentionLimits::conservative(),
            clients: ClientLimits::conservative(),
            ipc: IpcLimits::conservative(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_types::inspection::InspectionDomainV1;

    #[test]
    fn default_is_absent_and_owner_acknowledgements_are_independent() {
        assert_eq!(
            DashboardStartupConfig::default(),
            DashboardStartupConfig::Off
        );
        let provider = DashboardPayloadAcceptance::provider_visible();
        assert!(provider
            .descriptor()
            .captures(InspectionDomainV1::ProviderVisible));
        assert!(!provider.descriptor().captures(InspectionDomainV1::OwnerRaw));
        assert!(!provider
            .descriptor()
            .captures(InspectionDomainV1::OwnerRestored));

        let raw = provider.with_owner_raw(OwnerRawRiskAcknowledgement::acknowledge_pii_risk());
        assert!(raw.descriptor().captures(InspectionDomainV1::OwnerRaw));
        assert!(!raw.descriptor().captures(InspectionDomainV1::OwnerRestored));
        let both = raw.with_owner_restored(
            OwnerRestoredRiskAcknowledgement::acknowledge_reidentification_risk(),
        );
        assert!(both.descriptor().captures(InspectionDomainV1::OwnerRaw));
        assert!(both
            .descriptor()
            .captures(InspectionDomainV1::OwnerRestored));
    }

    #[test]
    fn every_limit_rejects_zero_and_one_over_hard_ceiling() {
        assert!(RetentionLimits::new(0, 1, Duration::from_secs(1)).is_err());
        assert!(RetentionLimits::new(HARD_MAX_EVENTS + 1, HARD_MAX_BYTES, HARD_MAX_TTL).is_err());
        assert!(RetentionLimits::new(HARD_MAX_EVENTS, HARD_MAX_BYTES + 1, HARD_MAX_TTL).is_err());
        assert!(RetentionLimits::new(
            HARD_MAX_EVENTS,
            HARD_MAX_BYTES,
            HARD_MAX_TTL + Duration::from_secs(1)
        )
        .is_err());
        assert!(IpcLimits::new(HARD_MAX_INGRESS + 1, HARD_MAX_IPC_FRAME).is_err());
        assert!(IpcLimits::new(HARD_MAX_INGRESS, HARD_MAX_IPC_FRAME + 1).is_err());
    }

    #[test]
    fn configured_bind_is_literal_loopback_port_zero_only() {
        assert!(LoopbackBind::configured(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).is_ok());
        assert!(LoopbackBind::configured(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 80)).is_err());
        assert!(
            LoopbackBind::configured(SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 0)).is_err()
        );
    }
}
