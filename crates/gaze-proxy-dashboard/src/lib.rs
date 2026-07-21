#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Fail-closed, memory-only dashboard runtime for provider-neutral Gaze inspection events.
//!
//! The dashboard is absent by default. When explicitly prepared, pairing acknowledgement
//! precedes creation of the pending inspection consumer. Sensitive state belongs in the
//! killable child entrypoint; provider request paths see only a bounded nonblocking sink.

mod assets;
mod auth;
mod child;
mod collector;
mod config;
mod error;
mod ipc;
mod runtime;
mod security_headers;
mod server;
mod sink;
mod store;
mod supervisor;
mod view;

pub use auth::{
    CanonicalAuthorizationV1, CsrfSecretV1, PageSessionSecretV1, PairingSecret,
    SensitiveBootstrapResponseV1, AUTHORIZATION_SCHEME_V1,
};
pub use child::{ChildConfig, ChildInheritedHandles, DashboardChildEntrypoint, NoDumpReadiness};
pub use config::{
    ClientLimits, DashboardPayloadAcceptance, DashboardStartupConfig, IpcLimits, LoopbackBind,
    OwnerRawRiskAcknowledgement, OwnerRestoredRiskAcknowledgement, RetentionLimits,
};
pub use error::{DashboardError, DashboardErrorCode, DashboardStatus};
pub use ipc::{DeliveredAckV1, PairingEnvelopeV1};
pub use runtime::{DashboardControl, DashboardLaunch, DashboardLifecycle};
pub use server::{
    DashboardHttp1Gate, ValidatedDashboardRequestV1, ValidatedRequest, CONSTANT_REJECTION_RESPONSE,
    MAX_HTTP_REQUEST_BYTES,
};
pub use sink::{DashboardInspectionSink, IngressCounters};
pub use supervisor::{
    DashboardSupervisor, PairedDashboard, PairingDelivery, PendingDashboardActivation,
    SpawnedDashboardChild,
};
pub use view::{
    AttestationTraceVm, DecisionTraceVm, DomainsAvailabilityVm, EventSummaryVm,
    ProjectionAvailabilityVm, RuntimeVm, SafeJsonShapeVm, SseTimelineEntryVm, SseTimelineVm,
    TrafficCountsVm,
};
