//! `PolicyGate` implementation: default-deny evaluation over
//! principal/tenant/workspace/role/tool/action/owner-bound-purpose/domain/entity-class/scope.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use gaze::PiiClass;

use crate::error::DenyReason;
use crate::model::{
    AuthGrant, BridgeRequest, CanonicalEntity, DomainId, IndexDomain, PolicyOutcome, PolicyRule,
    RequestedScope,
};
use crate::projection::HmacDomainProjector;
use crate::registry::{EntityRateLimit, IndexDomainRegistry};
use crate::session::RedactionSession;
use crate::traits::{DomainProjector, PolicyGate};
use crate::util::sha256_hex;

/// Persistent per-principal rate-limit bucket state. Owned by the long-lived runtime
/// (e.g. [`crate::bridge::TokenBridge`]) and injected by `&mut` into each short-lived
/// [`RegistryPolicyGate`]. Keeping the buckets OUTSIDE the per-call gate is what lets the
/// corpus-enumeration rate limit accumulate across calls instead of resetting every call
/// Default-constructs empty.
#[derive(Debug, Default)]
pub struct RateLimitState {
    buckets: HashMap<RateLimitBucket, HashSet<String>>,
}

impl RateLimitState {
    /// Create empty rate-limit state.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Policy gate backed by an [`IndexDomainRegistry`].
///
/// The gate is short-lived: a fresh one is built per authorization because it borrows the
/// registry. The per-principal rate-limit buckets do NOT live on the gate — they are
/// borrowed from caller-owned [`RateLimitState`] so they persist across gates.
#[derive(Debug)]
pub struct RegistryPolicyGate<'a> {
    registry: &'a IndexDomainRegistry,
    rate_limit: &'a mut RateLimitState,
}

impl<'a> RegistryPolicyGate<'a> {
    pub fn new(registry: &'a IndexDomainRegistry, rate_limit: &'a mut RateLimitState) -> Self {
        Self {
            registry,
            rate_limit,
        }
    }
}

impl PolicyGate for RegistryPolicyGate<'_> {
    fn evaluate(&mut self, session: &RedactionSession, request: &BridgeRequest) -> PolicyOutcome {
        self.try_evaluate(session, request)
            .unwrap_or_else(|deny| PolicyOutcome::Deny {
                reason: deny.reason,
                entity_class: deny.entity_class,
                raw_sha256: deny.raw_sha256,
            })
    }
}

impl RegistryPolicyGate<'_> {
    fn try_evaluate(
        &mut self,
        session: &RedactionSession,
        request: &BridgeRequest,
    ) -> Result<PolicyOutcome, DeniedAuthorization> {
        if request.principal.tenant_id != request.tenant_id
            || request.principal.workspace_id != request.workspace_id
        {
            return Err(DeniedAuthorization::without_entity(
                DenyReason::TenantOrWorkspaceMismatch,
            ));
        }

        if !session.is_bound_to(&request.principal.id) {
            return Err(DeniedAuthorization::without_entity(
                DenyReason::SessionPrincipalMismatch,
            ));
        }

        let domain = self
            .registry
            .domain(&request.target_domain)
            .ok_or_else(|| DeniedAuthorization::without_entity(DenyReason::DomainNotFound))?;

        if domain.tenant_id != request.tenant_id {
            return Err(DeniedAuthorization::without_entity(
                DenyReason::TenantOrWorkspaceMismatch,
            ));
        }

        if !domain.snippets_allowed {
            return Err(DeniedAuthorization::without_entity(
                DenyReason::SnippetsDisabled,
            ));
        }

        if !request.principal.has_any_role(&domain.allowed_roles) {
            return Err(DeniedAuthorization::without_entity(
                DenyReason::PrincipalNotAllowed,
            ));
        }

        if !domain_allows_tool_action(domain, request) {
            return Err(DeniedAuthorization::without_entity(
                DenyReason::NoMatchingAllowRule,
            ));
        }

        let (resolved_class, raw_value) =
            session
                .resolve_token(&request.source_token)
                .map_err(|reason| DeniedAuthorization {
                    reason,
                    entity_class: None,
                    raw_sha256: None,
                })?;
        let raw_sha256 = sha256_hex(&raw_value);

        if !domain.allows_class(&resolved_class) {
            return Err(DeniedAuthorization::with_entity(
                DenyReason::ClassNotAllowed,
                resolved_class,
                raw_sha256,
            ));
        }

        let entity = canonicalize_entity(resolved_class.clone(), &raw_value).ok_or_else(|| {
            DeniedAuthorization::with_entity(
                DenyReason::NoMatchingAllowRule,
                resolved_class.clone(),
                raw_sha256.clone(),
            )
        })?;
        let effective_purpose = self.owner_bound_purpose(request).ok_or_else(|| {
            DeniedAuthorization::with_entity(
                DenyReason::NoMatchingAllowRule,
                resolved_class.clone(),
                raw_sha256.clone(),
            )
        })?;
        let entity_ref = HmacDomainProjector::new(self.registry)
            .project(domain, &entity)
            .map_err(|_| {
                DeniedAuthorization::with_entity(
                    DenyReason::NoMatchingAllowRule,
                    resolved_class.clone(),
                    raw_sha256.clone(),
                )
            })?;

        let mut saw_cross_domain_denial = false;
        for (rule, rate_limit) in self
            .registry
            .rules_for_domain_with_limits(&request.target_domain)
        {
            if !rule_matches_base(rule, request, &effective_purpose, &resolved_class) {
                continue;
            }

            match scope_authorization(self.registry, domain, rule, request) {
                ScopeAuthorization::Allowed => {
                    enforce_rate_limit(
                        &mut self.rate_limit.buckets,
                        rate_limit,
                        request,
                        &resolved_class,
                        &raw_sha256,
                    )?;
                    return Ok(PolicyOutcome::Grant(AuthGrant {
                        entity_ref,
                        entity_class: resolved_class,
                        effective_purpose,
                        max_results: rule.max_results,
                        allowed_filters: allowed_filters(),
                        elevated_audit: rule.elevated_audit_required,
                        raw_sha256,
                    }));
                }
                ScopeAuthorization::CrossDomainDenied => {
                    saw_cross_domain_denial = true;
                }
                ScopeAuthorization::NoMatch => {}
            }
        }

        let reason = if saw_cross_domain_denial {
            DenyReason::CrossDomainDenied
        } else {
            DenyReason::NoMatchingAllowRule
        };
        Err(DeniedAuthorization::with_entity(
            reason,
            resolved_class,
            raw_sha256,
        ))
    }

    fn owner_bound_purpose(&self, request: &BridgeRequest) -> Option<String> {
        self.registry
            .rules_for_domain(&request.target_domain)
            .find(|rule| {
                rule.tenant_id == request.tenant_id
                    && rule.workspace_id == request.workspace_id
                    && rule.tool_name == request.tool_name
                    && rule.action == request.action
                    && request.principal.has_any_role(&rule.roles)
            })
            .map(|rule| rule.purpose.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeAuthorization {
    Allowed,
    CrossDomainDenied,
    NoMatch,
}

#[derive(Debug)]
struct DeniedAuthorization {
    reason: DenyReason,
    entity_class: Option<PiiClass>,
    raw_sha256: Option<String>,
}

impl DeniedAuthorization {
    fn without_entity(reason: DenyReason) -> Self {
        Self {
            reason,
            entity_class: None,
            raw_sha256: None,
        }
    }

    fn with_entity(reason: DenyReason, entity_class: PiiClass, raw_sha256: String) -> Self {
        Self {
            reason,
            entity_class: Some(entity_class),
            raw_sha256: Some(raw_sha256),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RateLimitBucket {
    principal_id: String,
    domain_id: DomainId,
    window: u64,
}

fn enforce_rate_limit(
    rate_limit_buckets: &mut HashMap<RateLimitBucket, HashSet<String>>,
    rate_limit: Option<EntityRateLimit>,
    request: &BridgeRequest,
    entity_class: &PiiClass,
    raw_sha256: &str,
) -> Result<(), DeniedAuthorization> {
    let Some(rate_limit) = rate_limit else {
        return Ok(());
    };

    let bucket = RateLimitBucket {
        principal_id: request.principal.id.clone(),
        domain_id: request.target_domain.clone(),
        window: current_window(rate_limit.window_seconds),
    };
    let entities = rate_limit_buckets.entry(bucket).or_default();
    if entities.contains(raw_sha256) {
        return Ok(());
    }
    if entities.len() >= rate_limit.max_entities_per_window {
        return Err(DeniedAuthorization::with_entity(
            DenyReason::NoMatchingAllowRule,
            entity_class.clone(),
            raw_sha256.to_string(),
        ));
    }

    entities.insert(raw_sha256.to_string());
    Ok(())
}

fn domain_allows_tool_action(domain: &IndexDomain, request: &BridgeRequest) -> bool {
    domain
        .allowed_tools
        .iter()
        .any(|tool| tool == &request.tool_name)
        && domain
            .allowed_actions
            .iter()
            .any(|action| action == &request.action)
}

fn rule_matches_base(
    rule: &PolicyRule,
    request: &BridgeRequest,
    effective_purpose: &str,
    entity_class: &PiiClass,
) -> bool {
    rule.tenant_id == request.tenant_id
        && rule.workspace_id == request.workspace_id
        && rule.tool_name == request.tool_name
        && rule.action == request.action
        && rule.purpose == effective_purpose
        && rule.target_domain == request.target_domain
        && request.principal.has_any_role(&rule.roles)
        && rule
            .allowed_entity_classes
            .iter()
            .any(|allowed| allowed == entity_class)
}

fn scope_authorization(
    registry: &IndexDomainRegistry,
    target_domain: &IndexDomain,
    rule: &PolicyRule,
    request: &BridgeRequest,
) -> ScopeAuthorization {
    match &request.requested_scope {
        RequestedScope::SameDomain if rule.allow_cross_domain => ScopeAuthorization::NoMatch,
        RequestedScope::SameDomain => ScopeAuthorization::Allowed,
        RequestedScope::CrossDomain { source_domain } => {
            if cross_domain_allowed(registry, target_domain, rule, request, source_domain) {
                ScopeAuthorization::Allowed
            } else {
                ScopeAuthorization::CrossDomainDenied
            }
        }
    }
}

fn cross_domain_allowed(
    registry: &IndexDomainRegistry,
    target_domain: &IndexDomain,
    rule: &PolicyRule,
    request: &BridgeRequest,
    source_domain_id: &DomainId,
) -> bool {
    if !rule.allow_cross_domain || !rule.elevated_audit_required {
        return false;
    }

    let Some(source_domain) = registry.domain(source_domain_id) else {
        return false;
    };

    source_domain.tenant_id == request.tenant_id
        && domains_are_mutually_co_searchable(source_domain, target_domain)
}

fn domains_are_mutually_co_searchable(
    source_domain: &IndexDomain,
    target_domain: &IndexDomain,
) -> bool {
    source_domain
        .co_searchable_with
        .iter()
        .any(|domain_id| domain_id == &target_domain.domain_id)
        && target_domain
            .co_searchable_with
            .iter()
            .any(|domain_id| domain_id == &source_domain.domain_id)
}

fn allowed_filters() -> Vec<String> {
    vec!["doc_id".to_string(), "corpus_type".to_string()]
}

fn canonicalize_entity(class: PiiClass, raw_value: &str) -> Option<CanonicalEntity> {
    let entity = CanonicalEntity::from_raw(class, raw_value);
    let value_prefix = format!("{}:", entity.class.to_canonical_str());
    let normalized = entity.canonical_value.strip_prefix(&value_prefix)?;
    if normalized.is_empty() {
        None
    } else {
        Some(entity)
    }
}

fn current_window(window_seconds: u64) -> u64 {
    let window_seconds = window_seconds.max(1);
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() / window_seconds)
}
