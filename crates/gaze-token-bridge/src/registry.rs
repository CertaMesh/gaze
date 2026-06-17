//! Track A — `IndexDomainRegistry`: load `IndexDomain`s + `PolicyRule`s from a policy
//! file, expose domain/rule/key lookup. Owner: sub-orchestrator A.
//! Contract: `crate::model::{IndexDomain, PolicyRule}`. Reference: spike `IndexDomainRegistry`.

use std::collections::HashMap;

use gaze::PiiClass;
use serde::Deserialize;

use crate::error::BridgeError;
use crate::keys::ProjectionKeyStore;
use crate::model::{DomainId, IndexDomain, PolicyRule};
use crate::traits::KeyManager;

/// Registry of index domains, allow rules, and all projection keys retained for
/// active and historical reads.
#[derive(Debug, Clone)]
pub struct IndexDomainRegistry {
    domains: HashMap<DomainId, IndexDomain>,
    rules: Vec<PolicyRule>,
    projection_keys: ProjectionKeyStore,
}

impl IndexDomainRegistry {
    pub fn from_json(input: &str) -> Result<Self, BridgeError> {
        let raw: RawPolicyFile =
            serde_json::from_str(input).map_err(|err| BridgeError::Policy(err.to_string()))?;

        let mut domains = HashMap::with_capacity(raw.domains.len());
        for raw_domain in raw.domains {
            let domain = IndexDomain {
                domain_id: raw_domain.domain_id,
                tenant_id: raw_domain.tenant_id,
                corpus_type: raw_domain.corpus_type,
                purpose: raw_domain.purpose,
                allowed_roles: raw_domain.allowed_roles,
                allowed_tools: raw_domain.allowed_tools,
                allowed_actions: raw_domain.allowed_actions,
                allowed_entity_classes: parse_classes(&raw_domain.allowed_entity_classes)?,
                snippets_allowed: raw_domain.snippets_allowed,
                raw_restore_allowed: raw_domain.raw_restore_allowed,
                co_searchable_with: raw_domain.co_searchable_with,
                projection_key_id: raw_domain.projection_key_id,
            };
            if domains
                .insert(domain.domain_id.clone(), domain.clone())
                .is_some()
            {
                return Err(BridgeError::Policy(format!(
                    "duplicate index domain {}",
                    domain.domain_id
                )));
            }
        }

        let projection_keys = ProjectionKeyStore::from_pairs(
            raw.projection_keys
                .into_iter()
                .map(|key| (key.key_id, key.material.into_bytes())),
        )?;

        let mut rules = Vec::with_capacity(raw.rules.len());
        for raw_rule in raw.rules {
            rules.push(PolicyRule {
                roles: raw_rule.roles,
                tenant_id: raw_rule.tenant_id,
                workspace_id: raw_rule.workspace_id,
                tool_name: raw_rule.tool_name,
                action: raw_rule.action,
                purpose: raw_rule.purpose,
                target_domain: raw_rule.target_domain,
                allowed_entity_classes: parse_classes(&raw_rule.allowed_entity_classes)?,
                max_results: raw_rule.max_results,
                allow_cross_domain: raw_rule.allow_cross_domain,
                elevated_audit_required: raw_rule.elevated_audit_required,
            });
        }

        Ok(Self {
            domains,
            rules,
            projection_keys,
        })
    }

    pub fn domain(&self, domain_id: &str) -> Option<&IndexDomain> {
        self.domains.get(domain_id)
    }

    pub fn domains(&self) -> impl Iterator<Item = &IndexDomain> {
        self.domains.values()
    }

    pub fn rules(&self) -> &[PolicyRule] {
        &self.rules
    }

    pub fn rules_for_domain<'a>(
        &'a self,
        domain_id: &'a str,
    ) -> impl Iterator<Item = &'a PolicyRule> + 'a {
        self.rules
            .iter()
            .filter(move |rule| rule.target_domain == domain_id)
    }

    pub fn projection_key(&self, key_id: &str) -> Option<&[u8]> {
        self.projection_keys.key(key_id)
    }

    pub fn projection_keys(&self) -> &ProjectionKeyStore {
        &self.projection_keys
    }
}

impl KeyManager for IndexDomainRegistry {
    fn key(&self, key_id: &str) -> Option<&[u8]> {
        self.projection_key(key_id)
    }
}

fn parse_classes(values: &[String]) -> Result<Vec<PiiClass>, BridgeError> {
    values
        .iter()
        .map(|value| {
            PiiClass::from_canonical_str(value)
                .ok_or_else(|| BridgeError::Policy(format!("unknown pii class {value}")))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct RawPolicyFile {
    domains: Vec<RawIndexDomain>,
    projection_keys: Vec<RawProjectionKey>,
    rules: Vec<RawPolicyRule>,
}

#[derive(Debug, Deserialize)]
struct RawIndexDomain {
    domain_id: DomainId,
    tenant_id: String,
    corpus_type: String,
    purpose: String,
    allowed_roles: Vec<String>,
    allowed_tools: Vec<String>,
    allowed_actions: Vec<String>,
    allowed_entity_classes: Vec<String>,
    snippets_allowed: bool,
    raw_restore_allowed: bool,
    co_searchable_with: Vec<DomainId>,
    projection_key_id: String,
}

#[derive(Debug, Deserialize)]
struct RawProjectionKey {
    key_id: String,
    material: String,
}

#[derive(Debug, Deserialize)]
struct RawPolicyRule {
    roles: Vec<String>,
    tenant_id: String,
    workspace_id: String,
    tool_name: String,
    action: String,
    purpose: String,
    target_domain: DomainId,
    allowed_entity_classes: Vec<String>,
    max_results: usize,
    allow_cross_domain: bool,
    elevated_audit_required: bool,
}
