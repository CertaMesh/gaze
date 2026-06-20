//! Shared data model.
//!
//! Three visibility tiers, do not blur them:
//! - **Agent-visible** ([`AgentSearchHit`], [`BridgeSearchResponse`]): safe for the LLM.
//!   These carry ONLY current-session tokens — never raw PII, aliases, or fingerprints.
//! - **Owner-side** ([`IndexEntity`], [`IndexSearchHit`]): carry raw values + index aliases.
//!   NOT `Serialize` by design, so they cannot be emitted to the agent path.
//! - **Trusted-runtime inputs** ([`BridgeRequest`], [`Principal`]): supplied by the
//!   runtime, never by the LLM. In particular `purpose` is owner-bound (the policy gate
//!   resolves the effective purpose from config; a request-supplied purpose is ignored).

use gaze::PiiClass;
use serde::{Deserialize, Serialize};

use crate::error::DenyReason;
use crate::util::collapse_ascii_whitespace;

/// `tenant/corpus/version` identifier, e.g. `tenant_demo/customer_docs/v1`.
pub type DomainId = String;

/// Authenticated caller. Extends gaze's `Principal` shape with tenant/workspace.
/// The LLM never constructs this; the trusted runtime resolves it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub roles: Vec<String>,
    pub tenant_id: String,
    pub workspace_id: String,
}

impl Principal {
    /// True if the principal holds any of the `required` roles.
    pub fn has_any_role(&self, required: &[String]) -> bool {
        self.roles
            .iter()
            .any(|role| required.iter().any(|req| req == role))
    }
}

/// Same-domain vs explicit cross-domain search. There is no "all domains" variant
/// by design — cross-domain is always a bounded, named source set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestedScope {
    SameDomain,
    CrossDomain { source_domain: DomainId },
}

/// A bridge request, fully expanded with trusted context by the runtime.
/// `purpose` here is the request-stated purpose; the policy gate ignores it for
/// authorization and uses the owner-bound purpose resolved from policy config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub principal: Principal,
    pub tenant_id: String,
    pub workspace_id: String,
    pub agent_run_id: String,
    pub conversation_session_id: String,
    pub tool_name: String,
    pub action: String,
    pub purpose: String,
    pub source_token: String,
    pub target_domain: DomainId,
    pub requested_scope: RequestedScope,
}

/// A raw value reduced to its canonical, class-tagged form prior to projection.
/// Canonicalization is frozen here so ingest and query agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalEntity {
    pub class: PiiClass,
    pub canonical_value: String,
}

impl CanonicalEntity {
    /// Canonicalize a raw value for a class. Whitespace-collapsed + lowercased,
    /// prefixed with the canonical class string. Deterministic and shared.
    pub fn from_raw(class: PiiClass, raw: &str) -> Self {
        let normalized = match &class {
            PiiClass::Email => raw.trim().to_ascii_lowercase(),
            PiiClass::Name | PiiClass::Location | PiiClass::Organization => {
                collapse_ascii_whitespace(raw).to_ascii_lowercase()
            }
            PiiClass::Custom(_) => raw.trim().to_ascii_lowercase(),
        };
        Self {
            canonical_value: format!("{}:{normalized}", class.to_canonical_str()),
            class,
        }
    }
}

/// A registered, bounded, policy-scoped searchable corpus. NOT a universal identity
/// graph and NOT a long-lived session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDomain {
    pub domain_id: DomainId,
    pub tenant_id: String,
    pub corpus_type: String,
    pub purpose: String,
    pub allowed_roles: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub allowed_actions: Vec<String>,
    pub allowed_entity_classes: Vec<PiiClass>,
    pub snippets_allowed: bool,
    pub raw_restore_allowed: bool,
    pub co_searchable_with: Vec<DomainId>,
    pub projection_key_id: String,
}

impl IndexDomain {
    /// True if `class` may be bridged into this domain.
    pub fn allows_class(&self, class: &PiiClass) -> bool {
        self.allowed_entity_classes.iter().any(|c| c == class)
    }
}

/// A single allow rule. Absence of a matching rule is a deny (default-deny).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRule {
    pub roles: Vec<String>,
    pub tenant_id: String,
    pub workspace_id: String,
    pub tool_name: String,
    pub action: String,
    pub purpose: String,
    pub target_domain: DomainId,
    pub allowed_entity_classes: Vec<PiiClass>,
    pub max_results: usize,
    pub allow_cross_domain: bool,
    pub elevated_audit_required: bool,
}

/// A canonical entity projected into one domain's keyspace. Owner-side index key;
/// never agent-visible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedEntityRef {
    pub domain_id: DomainId,
    pub key_id: String,
    pub entity_class: PiiClass,
    pub fingerprint_hex: String,
}

/// A short-lived, single-use, entity-bound search capability. Useless outside its
/// domain/action/entity, and never sufficient to restore raw PII.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHandle {
    pub principal_id: String,
    pub tenant_id: String,
    pub workspace_id: String,
    pub agent_run_id: String,
    pub conversation_session_id: String,
    pub source_token_hash: String,
    pub target_domain: DomainId,
    pub action: String,
    pub purpose: String,
    pub entity_class: PiiClass,
    /// The specific entity this handle was minted for. The adapter MUST reject a
    /// request whose entity_ref differs (confused-deputy guard).
    pub entity_ref: IndexedEntityRef,
    pub allowed_filters: Vec<String>,
    pub max_results: usize,
    pub expires_at_tick: u64,
    pub nonce: String,
    pub audit_id: String,
}

/// A caller-supplied search request. Filter values may carry raw PII and MUST be
/// projected/redacted owner-side before reaching the adapter (see [`ValidatedSearchRequest`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub requested_entity_ref: IndexedEntityRef,
    pub filters: Vec<FilterClause>,
}

impl SearchRequest {
    /// A request that searches exactly the handle's bound entity, no extra filters.
    pub fn from_handle(handle: &SearchHandle) -> Self {
        Self {
            requested_entity_ref: handle.entity_ref.clone(),
            filters: Vec::new(),
        }
    }
}

/// A raw, PII-bearing filter clause as supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterClause {
    pub field: String,
    pub class: PiiClass,
    pub value: String,
}

/// A filter clause after owner-side projection. The adapter only ever sees these —
/// the value is a `projected:<key_id>:<fingerprint>` token, never raw PII.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterFilterClause {
    pub field: String,
    pub value: String,
}

/// A search request after runtime validation + filter projection. Produced by the
/// bridge runtime; consumed by the adapter. The adapter trusts
/// that entity_ref/nonce/expiry checks live in the runtime, but still enforces the
/// entity_ref/domain/expiry/nonce guards defensively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSearchRequest {
    pub requested_entity_ref: IndexedEntityRef,
    pub filters: Vec<AdapterFilterClause>,
}

/// One index entity as stored owner-side: raw value + its domain alias + index ref.
/// NOT `Serialize` — it must never reach the agent path.
#[derive(Debug, Clone)]
pub struct IndexEntity {
    pub class: PiiClass,
    pub raw_value: String,
    pub index_ref: IndexedEntityRef,
    pub domain_alias: String,
}

/// One owner-side search hit: a snippet plus the entities it mentions. The snippet
/// contains domain aliases that the response translator replaces with session tokens.
/// NOT `Serialize` — owner-side only.
#[derive(Debug, Clone)]
pub struct IndexSearchHit {
    pub doc_id: String,
    pub snippet: String,
    pub entities: Vec<IndexEntity>,
}

/// One agent-visible hit: snippet rewritten into the active session namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSearchHit {
    pub doc_id: String,
    pub snippet: String,
}

/// The agent-visible response. Contains only current-session tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeSearchResponse {
    pub target_domain: DomainId,
    pub audit_id: String,
    pub results: Vec<AgentSearchHit>,
}

/// Result of an end-to-end search: an agent-visible response, or a typed deny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSearchOutcome {
    Allowed(BridgeSearchResponse),
    Denied(crate::error::DenyReason),
}

impl BridgeSearchOutcome {
    pub fn unwrap_allowed(self) -> BridgeSearchResponse {
        match self {
            Self::Allowed(response) => response,
            Self::Denied(reason) => panic!("expected allow, got deny: {reason:?}"),
        }
    }

    pub fn unwrap_denied(self) -> crate::error::DenyReason {
        match self {
            Self::Allowed(response) => panic!("expected deny, got allow: {response:?}"),
            Self::Denied(reason) => reason,
        }
    }
}

/// Authorization decision: an issued handle (+ whether elevated audit applies), or a deny.
// Transient decision value matched immediately by callers; boxing the handle would
// add deref friction at each match site for negligible benefit. Allow the size asymmetry.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeDecision {
    Allow {
        handle: SearchHandle,
        elevated_audit: bool,
    },
    Deny {
        reason: crate::error::DenyReason,
    },
}

/// Authorization grant emitted by policy evaluation. The bridge turns this into
/// a short-lived capability; policy evaluation does not mint handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthGrant {
    pub entity_ref: IndexedEntityRef,
    pub entity_class: PiiClass,
    pub effective_purpose: String,
    pub max_results: usize,
    pub allowed_filters: Vec<String>,
    pub elevated_audit: bool,
    pub raw_sha256: String,
}

/// Policy outcome: either enough owner-side authorization data for the bridge to issue
/// a capability, or a typed fail-closed deny.
// Transient decision value matched immediately by bridge runtime; keep it direct to
// mirror BridgeDecision and avoid allocation in the hot path.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyOutcome {
    Grant(AuthGrant),
    Deny {
        reason: DenyReason,
        entity_class: Option<PiiClass>,
        raw_sha256: Option<String>,
    },
}

/// Audit decision discriminant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditDecision {
    Allow,
    Deny,
}

/// One append-only audit event per bridge decision. Raw values appear only as
/// `raw_sha256`; the source token only as `source_token_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub audit_id: String,
    pub decision: AuditDecision,
    pub deny_reason: Option<crate::error::DenyReason>,
    pub principal_id: String,
    pub target_domain: DomainId,
    pub action: String,
    pub purpose: String,
    pub entity_class: Option<PiiClass>,
    pub source_token_hash: String,
    pub raw_sha256: Option<String>,
    pub elevated_audit: bool,
}
