//! Track C (gated) — capability issuance + lifecycle: mint entity-bound `SearchHandle`s
//! (nonce, expiry, single-use), validate them (entity_ref match, not expired, nonce
//! unused), project raw filter values before they reach the adapter. Owner: sub-orchestrator C.
//! Contract: `crate::model::{SearchHandle, SearchRequest, ValidatedSearchRequest}`.
//! Reference: spike `TokenBridge::{issue_handle, project_search_request}`.

use std::collections::HashSet;

use crate::error::DenyReason;
use crate::model::{
    AdapterFilterClause, AuthGrant, BridgeRequest, CanonicalEntity, IndexedEntityRef, SearchHandle,
    SearchRequest, ValidatedSearchRequest,
};
use crate::projection::HmacDomainProjector;
use crate::registry::IndexDomainRegistry;
use crate::traits::{CapabilityIssuer, DomainProjector};
use crate::util::sha256_hex;

/// Logical lifespan, in clock ticks, of a freshly issued [`SearchHandle`].
const HANDLE_TTL_TICKS: u64 = 2;

/// Monotonic logical clock. Tests advance it explicitly; nothing wall-clock-derived
/// leaks into handle expiry (keeps the contract deterministic — north-star axis 4).
#[derive(Debug, Default)]
pub struct LogicalClock {
    tick: u64,
}

impl LogicalClock {
    pub fn now(&self) -> u64 {
        self.tick
    }

    pub fn advance(&mut self, ticks: u64) {
        self.tick = self.tick.saturating_add(ticks);
    }
}

/// Owner-side capability runtime: issues entity-bound handles, validates them on use
/// (entity_ref / expiry / single-use nonce), and projects raw filter values into the
/// domain keyspace before they reach the adapter. Single-use nonce state lives here.
#[derive(Debug, Default)]
pub struct CapabilityRuntime {
    clock: LogicalClock,
    next_nonce: u64,
    next_audit: u64,
    consumed_nonces: HashSet<String>,
}

impl CapabilityRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current logical tick.
    pub fn now(&self) -> u64 {
        self.clock.now()
    }

    /// Advance the logical clock (test/runtime control over handle expiry).
    pub fn advance_clock(&mut self, ticks: u64) {
        self.clock.advance(ticks);
    }

    /// Next monotonic audit id. Shared by allow (carried in the handle) and deny rows
    /// so every bridge decision gets a unique, ordered id.
    pub fn next_audit_id(&mut self) -> String {
        self.next_audit += 1;
        format!("bridge-audit-{}", self.next_audit)
    }

    fn next_nonce_value(&mut self) -> String {
        self.next_nonce += 1;
        format!("nonce-{}", self.next_nonce)
    }

    /// Validate a handle against the entity_ref it is being used for. Fails closed on
    /// confused-deputy (entity_ref mismatch), domain mismatch, expiry, and replay.
    /// Consuming the nonce is the authoritative single-use gate; the adapter repeats
    /// these checks defensively (defense in depth — north-star axis 1).
    pub fn validate_handle(
        &mut self,
        handle: &SearchHandle,
        requested_entity_ref: &IndexedEntityRef,
    ) -> Result<(), DenyReason> {
        if requested_entity_ref != &handle.entity_ref {
            return Err(DenyReason::EntityRefMismatch);
        }
        if handle.entity_ref.domain_id != handle.target_domain {
            return Err(DenyReason::HandleDomainMismatch);
        }
        if self.clock.now() > handle.expires_at_tick {
            return Err(DenyReason::ExpiredHandle);
        }
        if !self.consumed_nonces.insert(handle.nonce.clone()) {
            return Err(DenyReason::ReplayedHandle);
        }
        Ok(())
    }

    /// Validate filter fields against the handle allowlist and project each raw filter
    /// value into `projected:<key_id>:<fingerprint>` so the adapter never sees raw PII.
    /// A filter field outside the handle allowlist fails closed (default-deny).
    pub fn project_request(
        &self,
        registry: &IndexDomainRegistry,
        handle: &SearchHandle,
        request: SearchRequest,
    ) -> Result<ValidatedSearchRequest, DenyReason> {
        let domain = registry
            .domain(&handle.target_domain)
            .ok_or(DenyReason::DomainNotFound)?;
        let projector = HmacDomainProjector::new(registry);

        let mut filters = Vec::with_capacity(request.filters.len());
        for filter in request.filters {
            if !handle
                .allowed_filters
                .iter()
                .any(|allowed| allowed == &filter.field)
            {
                return Err(DenyReason::NoMatchingAllowRule);
            }
            let entity = CanonicalEntity::from_raw(filter.class, &filter.value);
            let projected = projector
                .project(domain, &entity)
                .map_err(|_| DenyReason::NoMatchingAllowRule)?;
            filters.push(AdapterFilterClause {
                field: filter.field,
                value: format!(
                    "projected:{}:{}",
                    projected.key_id, projected.fingerprint_hex
                ),
            });
        }

        Ok(ValidatedSearchRequest {
            requested_entity_ref: request.requested_entity_ref,
            filters,
        })
    }
}

impl CapabilityIssuer for CapabilityRuntime {
    fn issue(&mut self, grant: &AuthGrant, request: &BridgeRequest) -> SearchHandle {
        let audit_id = self.next_audit_id();
        let nonce = self.next_nonce_value();
        SearchHandle {
            principal_id: request.principal.id.clone(),
            tenant_id: request.tenant_id.clone(),
            workspace_id: request.workspace_id.clone(),
            agent_run_id: request.agent_run_id.clone(),
            conversation_session_id: request.conversation_session_id.clone(),
            source_token_hash: sha256_hex(&request.source_token),
            target_domain: request.target_domain.clone(),
            action: request.action.clone(),
            // Owner-bound purpose comes from the grant, never from the request.
            purpose: grant.effective_purpose.clone(),
            entity_class: grant.entity_class.clone(),
            entity_ref: grant.entity_ref.clone(),
            allowed_filters: grant.allowed_filters.clone(),
            max_results: grant.max_results,
            expires_at_tick: self.clock.now() + HANDLE_TTL_TICKS,
            nonce,
            audit_id,
        }
    }
}
