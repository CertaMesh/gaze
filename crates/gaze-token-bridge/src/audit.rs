//! Track C (gated) — `BridgeAuditSink`: append-only audit, one event per bridge
//! decision, raw values only as sha256. Follow the `gaze-audit` restricted-column +
//! Dylint-isolation pattern when persisting durably. Owner: sub-orchestrator C.
//! Contract: `crate::traits::BridgeAuditSink`, `crate::model::AuditEvent`. Reference: spike `BridgeAudit`.

use crate::model::AuditEvent;
use crate::traits::BridgeAuditSink;

/// In-memory append-only audit sink. One [`AuditEvent`] per bridge decision; raw values
/// are already reduced to `raw_sha256` by the time they reach here (enforced by the
/// `AuditEvent` shape). A durable, restricted-column SQLite sink (the `gaze-audit`
/// Dylint-isolation pattern) is the follow-up for production persistence.
#[derive(Debug, Default)]
pub struct VecAuditSink {
    events: Vec<AuditEvent>,
}

impl VecAuditSink {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BridgeAuditSink for VecAuditSink {
    fn record(&mut self, event: AuditEvent) {
        self.events.push(event);
    }

    fn events(&self) -> &[AuditEvent] {
        &self.events
    }
}
