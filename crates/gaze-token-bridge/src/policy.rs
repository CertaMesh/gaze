//! Track A — `PolicyGate` implementation: default-deny evaluation over
//! principal/tenant/workspace/role/tool/action/owner-bound-purpose/domain/entity-class/scope,
//! plus rate-limit (T.3). Owner: sub-orchestrator A.
//! Contract: `crate::traits::PolicyGate`, `crate::model::{BridgeRequest, BridgeDecision}`.
//! Reference: spike `TokenBridge::try_authorize` + `owner_bound_purpose`.
