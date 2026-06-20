//! Track B — ingest pipeline: redact-before-index (run corpus text through the gaze
//! Pipeline so raw PII NEVER enters the index), canonicalize + project entities, build
//! the index. Add a CI gate asserting no raw PII enters the store. Owner: sub-orchestrator B.
//! Contract: `crate::model::{CanonicalEntity, IndexEntity, IndexSearchHit}`, `crate::util::domain_alias`.
//! Reference: spike `build_internal_hit` / `synthetic_docs`.

use std::ops::Range;

use gaze::{
    CleanDocument, DictionaryBundle, EmittedTokenSpan, LocaleTag, Pipeline, RawDocument,
    SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};

use crate::adapter::CorpusIndexStore;
use crate::error::BridgeError;
use crate::model::{CanonicalEntity, IndexDomain, IndexEntity, IndexSearchHit};
use crate::traits::DomainProjector;
use crate::util::domain_alias;

/// Redact-before-index corpus ingestor.
///
/// The caller supplies the configured gaze pipeline and domain projector. Ingest
/// creates an ephemeral gaze session, tokenizes raw text, derives owner-side raw
/// values only from emitted raw spans, and rewrites the clean snippet from
/// current-session gaze tokens to stable domain aliases.
pub struct CorpusIngestor<'a> {
    pipeline: &'a Pipeline,
    domain: &'a IndexDomain,
    projector: &'a dyn DomainProjector,
    locale_chain: Vec<LocaleTag>,
    safety_net_policy: SafetyNetPolicy,
    reject_safety_net_suspects: bool,
}

impl<'a> CorpusIngestor<'a> {
    pub fn new(
        pipeline: &'a Pipeline,
        domain: &'a IndexDomain,
        projector: &'a dyn DomainProjector,
    ) -> Self {
        Self::with_locale_chain(pipeline, domain, projector, vec![LocaleTag::Global])
    }

    pub fn with_locale_chain(
        pipeline: &'a Pipeline,
        domain: &'a IndexDomain,
        projector: &'a dyn DomainProjector,
        locale_chain: Vec<LocaleTag>,
    ) -> Self {
        Self {
            pipeline,
            domain,
            projector,
            locale_chain,
            safety_net_policy: SafetyNetPolicy::new(
                SafetyNetMode::Strict,
                SafetyNetFallback::Redact,
            ),
            reject_safety_net_suspects: true,
        }
    }

    pub fn with_safety_net_resolution(mut self) -> Self {
        self.safety_net_policy =
            SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Strict);
        self.reject_safety_net_suspects = false;
        self
    }

    pub fn ingest_text(
        &self,
        doc_id: impl Into<String>,
        raw_text: &str,
    ) -> Result<IndexSearchHit, BridgeError> {
        let session = Session::new(Scope::Ephemeral).map_err(|err| {
            BridgeError::Session(format!("failed to create ingest session: {err}"))
        })?;
        let fallback_locale_chain = [LocaleTag::Global];
        let locale_chain = if self.locale_chain.is_empty() {
            fallback_locale_chain.as_slice()
        } else {
            self.locale_chain.as_slice()
        };
        let (clean_doc, spans, leak_report) = self
            .pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw_text.to_string()),
                locale_chain,
                &DictionaryBundle::default(),
                self.safety_net_policy,
            )
            .map_err(|err| BridgeError::Session(format!("failed to clean corpus text: {err}")))?;

        if self.reject_safety_net_suspects && leak_report.stats.suspect_count > 0 {
            return Err(BridgeError::Session(format!(
                "safety net reported {} possible ingest leak(s)",
                leak_report.stats.suspect_count
            )));
        }

        let CleanDocument::Text(clean_text) = clean_doc else {
            return Err(BridgeError::Session(
                "text ingest produced non-text clean document".to_string(),
            ));
        };

        build_index_hit(
            doc_id.into(),
            raw_text,
            clean_text,
            &spans,
            self.domain,
            self.projector,
        )
    }

    pub fn ingest_text_into_store<S: CorpusIndexStore>(
        &self,
        store: &mut S,
        doc_id: impl Into<String>,
        raw_text: &str,
    ) -> Result<IndexSearchHit, BridgeError> {
        let hit = self.ingest_text(doc_id, raw_text)?;
        store.insert_hit(self.domain.domain_id.clone(), hit.clone());
        Ok(hit)
    }
}

fn build_index_hit(
    doc_id: String,
    raw_text: &str,
    clean_text: String,
    spans: &[EmittedTokenSpan],
    domain: &IndexDomain,
    projector: &dyn DomainProjector,
) -> Result<IndexSearchHit, BridgeError> {
    let mut snippet = clean_text;
    let mut entities = Vec::with_capacity(spans.len());
    let mut replacements = Vec::with_capacity(spans.len());

    for span in spans {
        let raw_value = slice(raw_text, span.raw_span.clone(), "raw")?;
        slice(&snippet, span.clean_span.clone(), "clean")?;

        let canonical = CanonicalEntity::from_raw(span.class.clone(), raw_value);
        let index_ref = projector.project(domain, &canonical)?;
        let alias = domain_alias(&span.class, &index_ref.fingerprint_hex);

        entities.push(IndexEntity {
            class: span.class.clone(),
            raw_value: raw_value.to_string(),
            index_ref,
            domain_alias: alias.clone(),
        });
        replacements.push((span.clean_span.clone(), alias));
    }

    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, alias) in replacements {
        slice(&snippet, range.clone(), "clean")?;
        snippet.replace_range(range, &alias);
    }

    Ok(IndexSearchHit {
        doc_id,
        snippet,
        entities,
    })
}

fn slice<'a>(text: &'a str, range: Range<usize>, label: &str) -> Result<&'a str, BridgeError> {
    text.get(range.clone()).ok_or_else(|| {
        BridgeError::Session(format!(
            "invalid {label} span {}..{} emitted during ingest",
            range.start, range.end
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, PiiClass};

    use super::*;
    use crate::adapter::InMemorySearchAdapter;
    use crate::model::{IndexedEntityRef, SearchHandle, ValidatedSearchRequest};
    use crate::traits::SearchAdapter;

    const DOMAIN: &str = "tenant_demo/customer_docs/v1";
    const RAW_EMAIL: &str = "alice@example.invalid";

    #[derive(Debug)]
    struct FixtureEmailDetector;

    impl Detector for FixtureEmailDetector {
        fn detect(&self, input: &str) -> Vec<Detection> {
            email_spans(input)
                .into_iter()
                .map(|span| Detection::new(span, PiiClass::Email, "fixture.email"))
                .collect()
        }
    }

    #[derive(Debug, Default)]
    struct RecordingProjector {
        canonical_values: RefCell<Vec<String>>,
    }

    impl RecordingProjector {
        fn projected_ref(
            &self,
            domain: &IndexDomain,
            canonical: &CanonicalEntity,
        ) -> IndexedEntityRef {
            let fingerprint_hex = crate::util::sha256_hex(&canonical.canonical_value);
            IndexedEntityRef {
                domain_id: domain.domain_id.clone(),
                key_id: domain.projection_key_id.clone(),
                entity_class: canonical.class.clone(),
                fingerprint_hex,
            }
        }
    }

    impl DomainProjector for RecordingProjector {
        fn project(
            &self,
            domain: &IndexDomain,
            entity: &CanonicalEntity,
        ) -> Result<IndexedEntityRef, BridgeError> {
            self.canonical_values
                .borrow_mut()
                .push(entity.canonical_value.clone());
            Ok(self.projected_ref(domain, entity))
        }
    }

    fn pipeline() -> Pipeline {
        Pipeline::builder()
            .detector(FixtureEmailDetector)
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .unwrap()
    }

    fn domain() -> IndexDomain {
        IndexDomain {
            domain_id: DOMAIN.to_string(),
            tenant_id: "tenant_demo".to_string(),
            corpus_type: "customer_docs".to_string(),
            purpose: "support_lookup".to_string(),
            allowed_roles: vec!["support".to_string()],
            allowed_tools: vec!["search".to_string()],
            allowed_actions: vec!["search".to_string()],
            allowed_entity_classes: vec![PiiClass::Email],
            snippets_allowed: true,
            raw_restore_allowed: false,
            co_searchable_with: Vec::new(),
            projection_key_id: "projection-key-v1".to_string(),
        }
    }

    fn ingest_fixture() -> (IndexSearchHit, RecordingProjector, IndexDomain) {
        let pipeline = pipeline();
        let domain = domain();
        let projector = RecordingProjector::default();
        let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector);
        let hit = ingestor
            .ingest_text("doc-1", "Email alice@example.invalid about the case.")
            .unwrap();

        (hit, projector, domain)
    }

    #[test]
    fn runs_gaze_pipeline_and_rewrites_snippet_to_domain_aliases() {
        let (hit, projector, _) = ingest_fixture();
        let alias = &hit.entities[0].domain_alias;

        assert_eq!(
            projector.canonical_values.borrow().as_slice(),
            ["email:alice@example.invalid"]
        );
        assert!(hit.snippet.contains(alias));
        assert!(!hit.snippet.contains(RAW_EMAIL));
        assert!(!hit.snippet.contains(":Email_1>"));
        assert!(hit.snippet.starts_with("Email <Email_"));
    }

    #[test]
    fn projected_entities_keep_raw_value_only_in_owner_side_entity_field() {
        let (hit, _, _) = ingest_fixture();
        let entity = &hit.entities[0];

        assert_eq!(entity.class, PiiClass::Email);
        assert_eq!(entity.raw_value, RAW_EMAIL);
        assert_eq!(entity.index_ref.entity_class, PiiClass::Email);
        assert_eq!(
            entity.domain_alias,
            domain_alias(&PiiClass::Email, &entity.index_ref.fingerprint_hex)
        );
        assert!(!hit.snippet.contains(RAW_EMAIL));
    }

    #[test]
    fn can_insert_ingested_hit_into_store_without_raw_search_keys() {
        let pipeline = pipeline();
        let domain = domain();
        let projector = RecordingProjector::default();
        let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector);
        let mut store = crate::adapter::InMemoryCorpusIndexStore::new();

        let hit = ingestor
            .ingest_text_into_store(
                &mut store,
                "doc-1",
                "Email alice@example.invalid about the case.",
            )
            .unwrap();
        let fingerprint = &hit.entities[0].index_ref.fingerprint_hex;
        let stored_hits = store.hits_for_entity(&domain.domain_id, fingerprint);

        assert_eq!(stored_hits.len(), 1);
        assert_eq!(stored_hits[0].doc_id, "doc-1");
        assert!(!stored_hits[0].snippet.contains(RAW_EMAIL));
        assert!(stored_hits[0]
            .snippet
            .contains(&hit.entities[0].domain_alias));
    }

    #[test]
    fn ingested_hits_search_by_projected_entity_ref() {
        let (hit, _, domain) = ingest_fixture();
        let entity_ref = hit.entities[0].index_ref.clone();
        let mut adapter = InMemorySearchAdapter::new();
        adapter.insert_hit(domain.domain_id.clone(), hit);

        let hits = adapter
            .search(
                &handle(entity_ref.clone()),
                ValidatedSearchRequest {
                    requested_entity_ref: entity_ref,
                    filters: Vec::new(),
                },
                10,
            )
            .unwrap();

        assert_eq!(hits.len(), 1);
        assert!(!hits[0].snippet.contains(RAW_EMAIL));
        assert!(!hits[0].snippet.contains(":Email_1>"));
        assert!(hits[0].snippet.contains("<Email_"));
    }

    #[test]
    fn no_entities_returns_clean_hit_with_empty_entities() {
        let pipeline = pipeline();
        let domain = domain();
        let projector = RecordingProjector::default();
        let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector);

        let hit = ingestor
            .ingest_text("doc-plain", "No contact needed.")
            .unwrap();

        assert_eq!(hit.doc_id, "doc-plain");
        assert_eq!(hit.snippet, "No contact needed.");
        assert!(hit.entities.is_empty());
        assert!(projector.canonical_values.borrow().is_empty());
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

    fn email_spans(input: &str) -> Vec<Range<usize>> {
        input
            .split_whitespace()
            .filter_map(|word| {
                let trimmed = word.trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':'));
                if is_fixture_email(trimmed) {
                    let start = word.as_ptr() as usize - input.as_ptr() as usize;
                    Some(start..start + trimmed.len())
                } else {
                    None
                }
            })
            .collect()
    }

    fn is_fixture_email(value: &str) -> bool {
        let Some((local, domain)) = value.split_once('@') else {
            return false;
        };
        !local.is_empty()
            && domain.contains('.')
            && domain
                .rsplit_once('.')
                .is_some_and(|(_, suffix)| suffix.len() >= 2)
    }
}
