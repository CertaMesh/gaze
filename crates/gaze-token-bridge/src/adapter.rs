//! Track B — `SearchAdapter` implementation: the real corpus index (store + query by
//! `IndexedEntityRef`), replacing the spike's in-memory fake. Enforces
//! entity_ref/domain/expiry/nonce guards defensively. Owner: sub-orchestrator B.
//! Contract: `crate::traits::SearchAdapter`, `crate::model::{ValidatedSearchRequest, IndexSearchHit}`.
//! Reference: spike `InMemorySearchAdapter`.
//!
//! Owner-side hits deliberately have no serde path:
//!
//! ```compile_fail
//! use gaze_token_bridge::model::IndexSearchHit;
//!
//! let hit = IndexSearchHit {
//!     doc_id: String::new(),
//!     snippet: String::new(),
//!     entities: Vec::new(),
//! };
//! serde_json::to_string(&hit).unwrap();
//! ```

use std::collections::{HashMap, HashSet};

use crate::error::DenyReason;
use crate::model::{DomainId, IndexSearchHit, SearchHandle, ValidatedSearchRequest};
use crate::traits::SearchAdapter;

/// Persistence-shaped store boundary for owner-side corpus hits.
///
/// Implementations may persist the document payload separately from the inverted
/// index. The searchable keyspace is strictly `(domain_id, fingerprint_hex)`;
/// raw PII must not be needed to query the store.
pub trait CorpusIndexStore {
    fn insert_hit(&mut self, domain_id: DomainId, hit: IndexSearchHit);

    fn hits_for_entity(&self, domain_id: &DomainId, fingerprint_hex: &str) -> Vec<IndexSearchHit>;
}

/// In-process corpus store backed by a document vector plus a fingerprint index.
#[derive(Debug, Default)]
pub struct InMemoryCorpusIndexStore {
    hits_by_domain: HashMap<DomainId, Vec<IndexSearchHit>>,
    hit_refs_by_entity: HashMap<DomainId, HashMap<String, Vec<usize>>>,
}

impl InMemoryCorpusIndexStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_hit(&mut self, domain_id: impl Into<DomainId>, hit: IndexSearchHit) {
        <Self as CorpusIndexStore>::insert_hit(self, domain_id.into(), hit);
    }

    pub fn is_empty(&self) -> bool {
        self.hits_by_domain.is_empty()
    }
}

impl CorpusIndexStore for InMemoryCorpusIndexStore {
    fn insert_hit(&mut self, domain_id: DomainId, hit: IndexSearchHit) {
        let hit_index = self
            .hits_by_domain
            .entry(domain_id.clone())
            .or_default()
            .len();
        let fingerprints = hit
            .entities
            .iter()
            .filter(|entity| entity.index_ref.domain_id == domain_id)
            .map(|entity| entity.index_ref.fingerprint_hex.clone())
            .collect::<HashSet<_>>();

        self.hits_by_domain
            .entry(domain_id.clone())
            .or_default()
            .push(hit);

        let by_fingerprint = self.hit_refs_by_entity.entry(domain_id).or_default();
        for fingerprint in fingerprints {
            by_fingerprint
                .entry(fingerprint)
                .or_default()
                .push(hit_index);
        }
    }

    fn hits_for_entity(&self, domain_id: &DomainId, fingerprint_hex: &str) -> Vec<IndexSearchHit> {
        let Some(hit_refs_by_fingerprint) = self.hit_refs_by_entity.get(domain_id) else {
            return Vec::new();
        };
        let Some(hit_refs) = hit_refs_by_fingerprint.get(fingerprint_hex) else {
            return Vec::new();
        };
        let Some(domain_hits) = self.hits_by_domain.get(domain_id) else {
            return Vec::new();
        };

        hit_refs
            .iter()
            .filter_map(|hit_index| domain_hits.get(*hit_index).cloned())
            .collect()
    }
}

/// Defensive, single-use search adapter over a corpus index store.
#[derive(Debug)]
pub struct InMemorySearchAdapter<S = InMemoryCorpusIndexStore> {
    store: S,
    used_nonces: HashSet<String>,
}

impl InMemorySearchAdapter<InMemoryCorpusIndexStore> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_hit(&mut self, domain_id: impl Into<DomainId>, hit: IndexSearchHit) {
        self.store.insert_hit(domain_id, hit);
    }
}

impl<S> InMemorySearchAdapter<S> {
    pub fn with_store(store: S) -> Self {
        Self {
            store,
            used_nonces: HashSet::new(),
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }
}

impl<S: Default> Default for InMemorySearchAdapter<S> {
    fn default() -> Self {
        Self::with_store(S::default())
    }
}

impl<S: CorpusIndexStore> SearchAdapter for InMemorySearchAdapter<S> {
    fn search(
        &mut self,
        handle: &SearchHandle,
        request: ValidatedSearchRequest,
        now_tick: u64,
    ) -> Result<Vec<IndexSearchHit>, DenyReason> {
        let ValidatedSearchRequest {
            requested_entity_ref,
            filters: _,
        } = request;

        if requested_entity_ref != handle.entity_ref {
            return Err(DenyReason::EntityRefMismatch);
        }
        if handle.entity_ref.domain_id != handle.target_domain {
            return Err(DenyReason::HandleDomainMismatch);
        }
        if now_tick > handle.expires_at_tick {
            return Err(DenyReason::ExpiredHandle);
        }
        if !self.used_nonces.insert(handle.nonce.clone()) {
            return Err(DenyReason::ReplayedHandle);
        }

        Ok(self
            .store
            .hits_for_entity(&handle.target_domain, &handle.entity_ref.fingerprint_hex)
            .into_iter()
            .take(handle.max_results)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use gaze::PiiClass;

    use super::*;
    use crate::model::{BridgeSearchResponse, IndexEntity, IndexedEntityRef};

    const DOMAIN: &str = "tenant_demo/customer_docs/v1";
    const OTHER_DOMAIN: &str = "tenant_demo/legal_docs/v1";
    const EMAIL_FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const NAME_FP: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn entity_ref(domain_id: &str, class: PiiClass, fingerprint_hex: &str) -> IndexedEntityRef {
        IndexedEntityRef {
            domain_id: domain_id.to_string(),
            key_id: "projection-key-v1".to_string(),
            entity_class: class,
            fingerprint_hex: fingerprint_hex.to_string(),
        }
    }

    fn owner_hit(doc_id: &str, index_ref: IndexedEntityRef) -> IndexSearchHit {
        let alias = crate::util::domain_alias(&index_ref.entity_class, &index_ref.fingerprint_hex);
        IndexSearchHit {
            doc_id: doc_id.to_string(),
            snippet: format!("Owner-side profile {alias} has a pending support case."),
            entities: vec![IndexEntity {
                class: index_ref.entity_class.clone(),
                raw_value: "alice@example.invalid".to_string(),
                index_ref,
                domain_alias: alias,
            }],
        }
    }

    fn handle(entity_ref: IndexedEntityRef) -> SearchHandle {
        SearchHandle {
            principal_id: "principal_support_1".to_string(),
            tenant_id: "tenant_demo".to_string(),
            workspace_id: "workspace_support".to_string(),
            agent_run_id: "agent_run_1".to_string(),
            conversation_session_id: "conversation_1".to_string(),
            source_token_hash: "source-token-sha256".to_string(),
            target_domain: entity_ref.domain_id.clone(),
            action: "search".to_string(),
            purpose: "support_lookup".to_string(),
            entity_class: entity_ref.entity_class.clone(),
            entity_ref,
            allowed_filters: Vec::new(),
            max_results: 10,
            expires_at_tick: 10,
            nonce: "nonce-1".to_string(),
            audit_id: "audit-1".to_string(),
        }
    }

    fn request(requested_entity_ref: IndexedEntityRef) -> ValidatedSearchRequest {
        ValidatedSearchRequest {
            requested_entity_ref,
            filters: Vec::new(),
        }
    }

    #[test]
    fn returns_hits_by_entity_ref_within_domain() {
        let email_ref = entity_ref(DOMAIN, PiiClass::Email, EMAIL_FP);
        let same_fingerprint_other_domain = entity_ref(OTHER_DOMAIN, PiiClass::Email, EMAIL_FP);
        let name_ref = entity_ref(DOMAIN, PiiClass::Name, NAME_FP);
        let mut adapter = InMemorySearchAdapter::new();
        adapter.insert_hit(DOMAIN, owner_hit("doc-email", email_ref.clone()));
        adapter.insert_hit(
            OTHER_DOMAIN,
            owner_hit("doc-other-domain", same_fingerprint_other_domain),
        );
        adapter.insert_hit(DOMAIN, owner_hit("doc-name", name_ref));

        let hits = adapter
            .search(&handle(email_ref.clone()), request(email_ref), 10)
            .unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, "doc-email");
    }

    #[test]
    fn requested_entity_ref_mismatch_denies() {
        let email_ref = entity_ref(DOMAIN, PiiClass::Email, EMAIL_FP);
        let name_ref = entity_ref(DOMAIN, PiiClass::Name, NAME_FP);
        let mut adapter = InMemorySearchAdapter::new();

        let reason = adapter
            .search(&handle(email_ref), request(name_ref), 10)
            .unwrap_err();

        assert_eq!(reason, DenyReason::EntityRefMismatch);
    }

    #[test]
    fn handle_domain_mismatch_denies() {
        let email_ref = entity_ref(DOMAIN, PiiClass::Email, EMAIL_FP);
        let mut bad_handle = handle(email_ref.clone());
        bad_handle.target_domain = OTHER_DOMAIN.to_string();
        let mut adapter = InMemorySearchAdapter::new();

        let reason = adapter
            .search(&bad_handle, request(email_ref), 10)
            .unwrap_err();

        assert_eq!(reason, DenyReason::HandleDomainMismatch);
    }

    #[test]
    fn expired_handle_denies_after_expiry_tick() {
        let email_ref = entity_ref(DOMAIN, PiiClass::Email, EMAIL_FP);
        let mut adapter = InMemorySearchAdapter::new();

        let reason = adapter
            .search(&handle(email_ref.clone()), request(email_ref), 11)
            .unwrap_err();

        assert_eq!(reason, DenyReason::ExpiredHandle);
    }

    #[test]
    fn replayed_handle_denies() {
        let email_ref = entity_ref(DOMAIN, PiiClass::Email, EMAIL_FP);
        let mut adapter = InMemorySearchAdapter::new();
        adapter.insert_hit(DOMAIN, owner_hit("doc-email", email_ref.clone()));
        let handle = handle(email_ref.clone());

        let first = adapter.search(&handle, request(email_ref.clone()), 10);
        let second = adapter.search(&handle, request(email_ref), 10);

        assert!(first.is_ok());
        assert_eq!(second.unwrap_err(), DenyReason::ReplayedHandle);
    }

    #[test]
    fn max_results_truncates_hits() {
        let email_ref = entity_ref(DOMAIN, PiiClass::Email, EMAIL_FP);
        let mut handle = handle(email_ref.clone());
        handle.max_results = 2;
        let mut adapter = InMemorySearchAdapter::new();
        adapter.insert_hit(DOMAIN, owner_hit("doc-1", email_ref.clone()));
        adapter.insert_hit(DOMAIN, owner_hit("doc-2", email_ref.clone()));
        adapter.insert_hit(DOMAIN, owner_hit("doc-3", email_ref.clone()));

        let hits = adapter.search(&handle, request(email_ref), 10).unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].doc_id, "doc-1");
        assert_eq!(hits[1].doc_id, "doc-2");
    }

    #[test]
    fn searchable_index_does_not_key_on_raw_values_or_aliases() {
        let email_ref = entity_ref(DOMAIN, PiiClass::Email, EMAIL_FP);
        let mut store = InMemoryCorpusIndexStore::new();
        store.insert_hit(DOMAIN, owner_hit("doc-email", email_ref));

        let by_fingerprint = store.hit_refs_by_entity.get(DOMAIN).unwrap();

        assert!(by_fingerprint.contains_key(EMAIL_FP));
        assert!(!by_fingerprint.contains_key("alice@example.invalid"));
        assert!(!by_fingerprint
            .keys()
            .any(|key| key.contains("<Email_") || key.contains("alice@example.invalid")));
    }

    #[test]
    fn owner_side_hits_remain_outside_agent_visible_serde_path() {
        let model_source = include_str!("model.rs");
        let index_entity_derive = derive_for(model_source, "IndexEntity");
        let index_search_hit_derive = derive_for(model_source, "IndexSearchHit");
        let response = BridgeSearchResponse {
            target_domain: DOMAIN.to_string(),
            audit_id: "audit-1".to_string(),
            results: Vec::new(),
        };
        let json = serde_json::to_string(&response).unwrap();

        assert!(!index_entity_derive.contains("Serialize"));
        assert!(!index_entity_derive.contains("Deserialize"));
        assert!(!index_search_hit_derive.contains("Serialize"));
        assert!(!index_search_hit_derive.contains("Deserialize"));
        assert!(!json.contains(EMAIL_FP));
        assert!(!json.contains("alice@example.invalid"));
    }

    fn derive_for<'a>(model_source: &'a str, struct_name: &str) -> &'a str {
        model_source
            .split(&format!("pub struct {struct_name}"))
            .next()
            .and_then(|before_struct| {
                before_struct
                    .lines()
                    .rev()
                    .find(|line| line.trim_start().starts_with("#[derive"))
            })
            .expect("model struct should have a derive attribute")
    }
}
