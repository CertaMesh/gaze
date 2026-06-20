//! Track C (gated) — `TokenBridge` runtime: orchestrate resolve → canonicalize →
//! project (A) → policy (A) → mint capability (C) → adapter (B) → translate (C) → audit (C).
//! Also the chokepoint integration (ToolResources sealed handle, `search_documents` tool).
//! Owner: sub-orchestrator C. Reference: spike `TokenBridge`.
//!
//! # Ownership note
//! [`crate::policy::RegistryPolicyGate`] borrows its registry, while [`TokenBridge::demo`]
//! must return an owned value. A struct that owns the registry *and* a gate borrowing it
//! would be self-referential, so the bridge owns the registry and constructs a fresh gate
//! per authorization. Consequence: the gate's per-principal rate-limit buckets do not
//! persist across calls. Tracked as a follow-up (the bundled fixture limit is never hit).

use std::collections::HashMap;
use std::ops::Range;

use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, PiiClass, Pipeline};

use crate::adapter::InMemorySearchAdapter;
use crate::audit::VecAuditSink;
use crate::capability::CapabilityRuntime;
use crate::error::{BridgeError, DenyReason};
use crate::ingest::CorpusIngestor;
use crate::model::{
    AdapterFilterClause, AuditDecision, AuditEvent, BridgeDecision, BridgeRequest,
    BridgeSearchOutcome, BridgeSearchResponse, DomainId, IndexSearchHit, PolicyOutcome,
    SearchHandle, SearchRequest,
};
use crate::policy::RegistryPolicyGate;
use crate::projection::HmacDomainProjector;
use crate::registry::IndexDomainRegistry;
use crate::session::RedactionSession;
use crate::traits::{
    BridgeAuditSink, CapabilityIssuer, PolicyGate, ResponseTranslator, SearchAdapter,
};
use crate::translate::SessionResponseTranslator;
use crate::util::sha256_hex;

// Track C chokepoint integration — gated, OFF by default. Implements
// `gaze_mcp_core::Tool` for the owner-side `SearchDocumentsTool` without touching
// gaze-mcp-core's sealed surface. See `chokepoint` module docs.
#[cfg(feature = "chokepoint")]
mod chokepoint;
#[cfg(feature = "chokepoint")]
pub use chokepoint::SearchDocumentsTool;

/// Bundled demo policy (two domains, support + admin rules). Owner-side fixture.
const DEMO_POLICY_JSON: &str = include_str!("../fixtures/policy.json");
const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";
const LEGAL_DOMAIN: &str = "tenant_demo/legal_docs/v1";

/// Owner-side bridge runtime. Composes the registry-backed policy gate (A), the corpus
/// search adapter (B), the capability runtime (C), and an append-only audit sink (C).
#[derive(Debug)]
pub struct TokenBridge {
    registry: IndexDomainRegistry,
    adapter: InMemorySearchAdapter,
    capability: CapabilityRuntime,
    audit: VecAuditSink,
    /// Owner-side copy of ingested hits, keyed by domain, for test introspection
    /// (`primary_alias` / `primary_fingerprint`). Never `Serialize`, never agent-visible.
    corpus: HashMap<DomainId, Vec<IndexSearchHit>>,
    /// The projected filters last handed to the adapter (proves raw filter values are
    /// projected owner-side before the adapter sees them).
    last_adapter_filters: Vec<AdapterFilterClause>,
}

impl TokenBridge {
    /// Build the bundled demo bridge: load the demo policy, ingest the synthetic corpus
    /// into both domains via the Track B redact-before-index ingestor, and compose.
    pub fn demo() -> Result<Self, BridgeError> {
        Self::from_policy_json(DEMO_POLICY_JSON)
    }

    /// Build a bridge from an explicit policy JSON document, ingesting the synthetic
    /// corpus into both demo domains. Used by tests that need an augmented policy (e.g.
    /// an elevated cross-domain rule) without mutating the bundled fixture.
    pub fn from_policy_json(policy_json: &str) -> Result<Self, BridgeError> {
        let registry = IndexDomainRegistry::from_json(policy_json)?;
        let mut adapter = InMemorySearchAdapter::new();
        let mut corpus: HashMap<DomainId, Vec<IndexSearchHit>> = HashMap::new();
        let docs = synthetic_docs();

        for domain_id in [CUSTOMER_DOMAIN, LEGAL_DOMAIN] {
            let domain = registry
                .domain(domain_id)
                .ok_or_else(|| BridgeError::Policy(format!("missing demo domain {domain_id}")))?
                .clone();
            let pipeline = build_demo_pipeline(&docs)?;
            let projector = HmacDomainProjector::new(&registry);
            let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector);

            for doc in &docs {
                let raw_text = doc.raw_text(domain_id);
                let hit =
                    ingestor.ingest_text_into_store(adapter.store_mut(), doc.id, &raw_text)?;
                corpus.entry(domain_id.to_string()).or_default().push(hit);
            }
        }

        Ok(Self {
            registry,
            adapter,
            capability: CapabilityRuntime::new(),
            audit: VecAuditSink::new(),
            corpus,
            last_adapter_filters: Vec::new(),
        })
    }

    /// Authorize a request and, on success, mint a single-use capability. Records exactly
    /// one audit event per call (the authorization decision). The owner-side policy gate
    /// does all authz internally; the bridge trusts the resulting grant.
    pub fn authorize(
        &mut self,
        session: &RedactionSession,
        request: &BridgeRequest,
    ) -> BridgeDecision {
        // Scope the gate's borrow of `self.registry` to this block so the subsequent
        // mutable borrows of `self.capability` / `self.audit` are legal.
        let outcome = {
            let mut gate = RegistryPolicyGate::new(&self.registry);
            gate.evaluate(session, request)
        };

        match outcome {
            PolicyOutcome::Grant(grant) => {
                let handle = self.capability.issue(&grant, request);
                self.audit.record(AuditEvent {
                    audit_id: handle.audit_id.clone(),
                    decision: AuditDecision::Allow,
                    deny_reason: None,
                    principal_id: request.principal.id.clone(),
                    target_domain: request.target_domain.clone(),
                    action: request.action.clone(),
                    purpose: grant.effective_purpose.clone(),
                    entity_class: Some(grant.entity_class.clone()),
                    source_token_hash: handle.source_token_hash.clone(),
                    raw_sha256: Some(grant.raw_sha256.clone()),
                    elevated_audit: grant.elevated_audit,
                });
                BridgeDecision::Allow {
                    handle,
                    elevated_audit: grant.elevated_audit,
                }
            }
            PolicyOutcome::Deny {
                reason,
                entity_class,
                raw_sha256,
            } => {
                let audit_id = self.capability.next_audit_id();
                self.audit.record(AuditEvent {
                    audit_id,
                    decision: AuditDecision::Deny,
                    deny_reason: Some(reason.clone()),
                    principal_id: request.principal.id.clone(),
                    target_domain: request.target_domain.clone(),
                    action: request.action.clone(),
                    // No grant ⇒ no owner-bound purpose; record the request-stated one.
                    purpose: request.purpose.clone(),
                    entity_class,
                    source_token_hash: sha256_hex(&request.source_token),
                    raw_sha256,
                    elevated_audit: false,
                });
                BridgeDecision::Deny { reason }
            }
        }
    }

    /// Execute a previously issued handle over exactly its bound entity (no extra filters).
    pub fn execute_handle(
        &mut self,
        session: &RedactionSession,
        handle: &SearchHandle,
    ) -> Result<BridgeSearchResponse, DenyReason> {
        self.execute_search_request(session, handle, SearchRequest::from_handle(handle))
    }

    /// Execute a handle against an explicit search request: validate the handle, project
    /// raw filter values owner-side, search the adapter, then translate hits into the
    /// active session namespace. Every failure path fails closed to a typed [`DenyReason`].
    pub fn execute_search_request(
        &mut self,
        session: &RedactionSession,
        handle: &SearchHandle,
        request: SearchRequest,
    ) -> Result<BridgeSearchResponse, DenyReason> {
        self.capability
            .validate_handle(handle, &request.requested_entity_ref)?;
        let validated = self
            .capability
            .project_request(&self.registry, handle, request)?;
        self.last_adapter_filters = validated.filters.clone();

        let now = self.capability.now();
        let hits = self.adapter.search(handle, validated, now)?;
        let results = SessionResponseTranslator::translate(session, hits)?;

        Ok(BridgeSearchResponse {
            target_domain: handle.target_domain.clone(),
            audit_id: handle.audit_id.clone(),
            results,
        })
    }

    /// End-to-end search: authorize (audited) → execute → translate. Any deny — at the
    /// policy gate or during execution — surfaces as a typed [`BridgeSearchOutcome::Denied`].
    pub fn search(
        &mut self,
        session: &RedactionSession,
        request: &BridgeRequest,
    ) -> BridgeSearchOutcome {
        match self.authorize(session, request) {
            BridgeDecision::Allow { handle, .. } => match self.execute_handle(session, &handle) {
                Ok(response) => BridgeSearchOutcome::Allowed(response),
                Err(reason) => BridgeSearchOutcome::Denied(reason),
            },
            BridgeDecision::Deny { reason } => BridgeSearchOutcome::Denied(reason),
        }
    }

    /// Advance the logical clock (controls handle expiry in tests / batch runs).
    pub fn advance_clock(&mut self, ticks: u64) {
        self.capability.advance_clock(ticks);
    }

    /// Append-only audit trail of bridge decisions.
    pub fn audit(&self) -> &[AuditEvent] {
        self.audit.events()
    }

    /// Projected filters last handed to the adapter (owner-side test introspection).
    pub fn last_adapter_filters(&self) -> &[AdapterFilterClause] {
        &self.last_adapter_filters
    }

    /// Owner-side: the stable domain alias of the first `class` entity in a known doc.
    pub fn primary_alias(&self, domain_id: &str, doc_id: &str, class: &PiiClass) -> Option<String> {
        self.entity(domain_id, doc_id, class)
            .map(|entity| entity.domain_alias.clone())
    }

    /// Owner-side: the projection fingerprint of the first `class` entity in a known doc.
    pub fn primary_fingerprint(
        &self,
        domain_id: &str,
        doc_id: &str,
        class: &PiiClass,
    ) -> Option<String> {
        self.entity(domain_id, doc_id, class)
            .map(|entity| entity.index_ref.fingerprint_hex.clone())
    }

    fn entity(
        &self,
        domain_id: &str,
        doc_id: &str,
        class: &PiiClass,
    ) -> Option<&crate::model::IndexEntity> {
        self.corpus
            .get(domain_id)?
            .iter()
            .find(|hit| hit.doc_id == doc_id)?
            .entities
            .iter()
            .find(|entity| &entity.class == class)
    }
}

/// A structured synthetic record. Mirrors the spike fixture set; reused by the PR3 example.
#[derive(Debug, Clone)]
pub struct SyntheticDoc {
    pub id: &'static str,
    pub name: &'static str,
    pub email: &'static str,
    pub customer_id: &'static str,
    pub organization: &'static str,
}

impl SyntheticDoc {
    /// Render this record as raw corpus text for a domain. The text is fed through the
    /// Track B redact-before-index ingestor, so the raw values below never enter the
    /// index in cleartext — only their projected aliases and owner-side raw fields do.
    fn raw_text(&self, domain_id: &str) -> String {
        if domain_id == CUSTOMER_DOMAIN {
            format!(
                "Customer profile {} / {} / {} is linked to {}.",
                self.name, self.email, self.customer_id, self.organization
            )
        } else {
            format!(
                "Legal matter references {} at {}; contact route {}.",
                self.name, self.organization, self.email
            )
        }
    }
}

/// The bundled synthetic corpus (mirrors the spike). Reused by the PR3 example.
pub fn synthetic_docs() -> Vec<SyntheticDoc> {
    vec![
        SyntheticDoc {
            id: "cust-001",
            name: "Markus Gottschaue",
            email: "markus@example.invalid",
            customer_id: "91A",
            organization: "Globex GmbH",
        },
        SyntheticDoc {
            id: "cust-002",
            name: "Dr. Schmidt",
            email: "schmidt@example.invalid",
            customer_id: "72B",
            organization: "Initech AG",
        },
        SyntheticDoc {
            id: "cust-003",
            name: "Lena Torres",
            email: "lena@example.invalid",
            customer_id: "48C",
            organization: "Umbrella LLC",
        },
        SyntheticDoc {
            id: "cust-004",
            name: "Prof. Weber",
            email: "weber@example.invalid",
            customer_id: "54D",
            organization: "Soylent GmbH",
        },
        SyntheticDoc {
            id: "cust-005",
            name: "Ana Rossi",
            email: "ana@example.invalid",
            customer_id: "33E",
            organization: "Vehement Capital Partners",
        },
    ]
}

/// Build the gaze pipeline that redacts the synthetic corpus before indexing. The
/// synthetic values are known fixtures, so a deterministic literal detector keeps the
/// demo reproducible (the bridge's job here is the runtime, not detector quality).
fn build_demo_pipeline(docs: &[SyntheticDoc]) -> Result<Pipeline, BridgeError> {
    Pipeline::builder()
        .detector(SyntheticCorpusDetector::from_docs(docs))
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Organization, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::custom("customer_id"),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .map_err(|err| BridgeError::Policy(format!("failed to build demo pipeline: {err:?}")))
}

/// Deterministic literal detector over the known synthetic values. Emits a non-overlapping
/// detection for every known `(value, class)` pair present in the input (longest first).
#[derive(Debug)]
struct SyntheticCorpusDetector {
    entries: Vec<(String, PiiClass)>,
}

impl SyntheticCorpusDetector {
    fn from_docs(docs: &[SyntheticDoc]) -> Self {
        let mut entries = Vec::with_capacity(docs.len() * 4);
        for doc in docs {
            entries.push((doc.name.to_string(), PiiClass::Name));
            entries.push((doc.email.to_string(), PiiClass::Email));
            entries.push((doc.organization.to_string(), PiiClass::Organization));
            entries.push((doc.customer_id.to_string(), PiiClass::custom("customer_id")));
        }
        // Longest values first so a longer value claims its span before any shorter
        // value that might be a substring of it.
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
        entries.dedup();
        Self { entries }
    }
}

impl Detector for SyntheticCorpusDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        let mut detections = Vec::new();
        let mut claimed: Vec<Range<usize>> = Vec::new();
        for (value, class) in &self.entries {
            if value.is_empty() {
                continue;
            }
            let mut cursor = 0;
            while let Some(offset) = input[cursor..].find(value.as_str()) {
                let start = cursor + offset;
                let end = start + value.len();
                let overlaps = claimed
                    .iter()
                    .any(|range| start < range.end && range.start < end);
                if !overlaps {
                    detections.push(Detection::new(
                        start..end,
                        class.clone(),
                        "synthetic.corpus",
                    ));
                    claimed.push(start..end);
                }
                cursor = end;
            }
        }
        detections
    }
}
