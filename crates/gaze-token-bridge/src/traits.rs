//! FROZEN CONTRACT — trait surfaces each track implements. Master-owned.
//!
//! These signatures are the seam between tracks. Tracks add `impl`s in their own
//! modules; they do not change these signatures. A signature change is a master
//! decision (raise NEED DIRECTION).

use crate::error::{BridgeError, DenyReason};
use crate::model::{
    AgentSearchHit, AuditEvent, AuthGrant, BridgeRequest, CanonicalEntity, IndexDomain,
    IndexSearchHit, IndexedEntityRef, PolicyOutcome, SearchHandle, ValidatedSearchRequest,
};
use crate::session::RedactionSession;

/// Track A — deterministic domain projection.
/// Implementations MUST key only on `(tenant, domain)` (via the domain's
/// `projection_key_id`); never salt with principal (would break shared-corpus lookup).
pub trait DomainProjector {
    fn project(
        &self,
        domain: &IndexDomain,
        entity: &CanonicalEntity,
    ) -> Result<IndexedEntityRef, BridgeError>;
}

/// Track A — owner-side authorization. Default-deny; resolves an owner-bound purpose
/// from policy config (ignoring any request-supplied purpose); emits a typed decision.
pub trait PolicyGate {
    fn evaluate(&mut self, session: &RedactionSession, request: &BridgeRequest) -> PolicyOutcome;
}

/// Track A — projection key material, addressed by `key_id`. Supports rotation by
/// retaining old keys for read; a missing key fails closed (no silent re-key).
pub trait KeyManager {
    fn key(&self, key_id: &str) -> Option<&[u8]>;
}

/// Track B — the corpus index + search. Receives an already-validated, filter-projected
/// request. Still enforces entity_ref/domain/expiry/nonce guards defensively, returning
/// owner-side hits (never agent-visible output).
pub trait SearchAdapter {
    fn search(
        &mut self,
        handle: &SearchHandle,
        request: ValidatedSearchRequest,
        now_tick: u64,
    ) -> Result<Vec<IndexSearchHit>, DenyReason>;
}

/// Track C — translate owner-side hits into the active session namespace, minting
/// fresh session tokens for newly-discovered entities. MUST fail closed if any domain
/// alias or raw value would remain in the output.
pub trait ResponseTranslator {
    fn translate(
        session: &RedactionSession,
        hits: Vec<IndexSearchHit>,
    ) -> Result<Vec<AgentSearchHit>, DenyReason>;
}

/// Track C — append-only audit sink. One event per bridge decision; raw values only
/// as sha256.
pub trait BridgeAuditSink {
    fn record(&mut self, event: AuditEvent);
    fn events(&self) -> &[AuditEvent];
}

/// Track C (R2, master-authorized) — mint a short-lived, entity-bound capability from a
/// policy grant. The issuer copies trusted request context + the grant's owner-side
/// authorization into a single-use [`SearchHandle`]; it never re-authorizes (the grant
/// already encodes the owner-side decision) and never decides scope itself.
pub trait CapabilityIssuer {
    fn issue(&mut self, grant: &AuthGrant, request: &BridgeRequest) -> SearchHandle;
}
