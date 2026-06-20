//! Track C (gated) — `BridgeAuditSink`: append-only audit, one event per bridge
//! decision, raw values only as sha256. Follow the `gaze-audit` restricted-column +
//! Dylint-isolation pattern when persisting durably. Owner: sub-orchestrator C.
//! Contract: `crate::traits::BridgeAuditSink`, `crate::model::AuditEvent`. Reference: spike `BridgeAudit`.
