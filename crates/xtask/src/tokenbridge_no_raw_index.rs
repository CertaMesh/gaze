use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, PiiClass, Pipeline};
use gaze_token_bridge::adapter::{
    CorpusIndexStore, InMemoryCorpusIndexStore, InMemorySearchAdapter,
};
use gaze_token_bridge::ingest::CorpusIngestor;
use gaze_token_bridge::model::{
    CanonicalEntity, IndexDomain, IndexSearchHit, IndexedEntityRef, SearchHandle,
    ValidatedSearchRequest,
};
use gaze_token_bridge::traits::{DomainProjector, SearchAdapter};
use gaze_token_bridge::util::{domain_alias, parse_token_class, sha256_hex};
use gaze_token_bridge::BridgeError;

const RAW_EMAIL: &str = "alice@example.invalid";
const CANONICAL_EMAIL: &str = "email:alice@example.invalid";
const DOMAIN_ID: &str = "tenant_demo/customer_docs/v1";
const FIXTURE_DOC_ID: &str = "synthetic-support-doc-1";
const FIXTURE_CORPUS: &str =
    "Support note: alice@example.invalid asked about a reversible index fixture.";

#[derive(Debug, Parser)]
pub struct Args {
    /// Inject a controlled raw-snippet regression so the adversarial self-test can
    /// prove this gate fails closed.
    #[arg(long, hide = true)]
    inject_adversarial_raw_snippet: bool,
}

pub fn run(args: Args) -> Result<()> {
    println!("tokenbridge_no_raw_index: running real TokenBridge ingest guard");

    let pipeline = pipeline()?;
    let domain = domain();
    let projector = DeterministicProjector;
    let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector);
    let mut store = InMemoryCorpusIndexStore::new();
    let hit = ingestor
        .ingest_text_into_store(&mut store, FIXTURE_DOC_ID, FIXTURE_CORPUS)
        .map_err(|err| {
            anyhow!("tokenbridge_no_raw_index: failed to ingest synthetic corpus: {err:?}")
        })?;

    if args.inject_adversarial_raw_snippet {
        let mut adversarial_hit = hit.clone();
        adversarial_hit.doc_id = "doc-adversarial-raw-snippet".to_string();
        adversarial_hit.snippet = format!("Regression copy leaked {RAW_EMAIL}");
        store.insert_hit(domain.domain_id.clone(), adversarial_hit);
    }

    inspect_store_and_search_results(&domain, store, &hit)?;

    println!("tokenbridge_no_raw_index: passed");
    Ok(())
}

#[derive(Debug)]
struct FixtureEmailDetector;

impl Detector for FixtureEmailDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .match_indices(RAW_EMAIL)
            .map(|(start, raw)| {
                Detection::new(
                    start..start + raw.len(),
                    PiiClass::Email,
                    "xtask.fixture.email",
                )
            })
            .collect()
    }
}

#[derive(Debug)]
struct DeterministicProjector;

impl DomainProjector for DeterministicProjector {
    fn project(
        &self,
        domain: &IndexDomain,
        entity: &CanonicalEntity,
    ) -> Result<IndexedEntityRef, BridgeError> {
        Ok(IndexedEntityRef {
            domain_id: domain.domain_id.clone(),
            key_id: domain.projection_key_id.clone(),
            entity_class: entity.class.clone(),
            fingerprint_hex: sha256_hex(&entity.canonical_value),
        })
    }
}

fn pipeline() -> Result<Pipeline> {
    Pipeline::builder()
        .detector(FixtureEmailDetector)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .map_err(|err| anyhow!("tokenbridge_no_raw_index: failed to build pipeline: {err}"))
}

fn domain() -> IndexDomain {
    IndexDomain {
        domain_id: DOMAIN_ID.to_string(),
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

fn inspect_store_and_search_results(
    domain: &IndexDomain,
    store: InMemoryCorpusIndexStore,
    ingested_hit: &IndexSearchHit,
) -> Result<()> {
    let entity = ingested_hit
        .entities
        .first()
        .context("tokenbridge_no_raw_index: synthetic ingest emitted no index entity")?;
    let fingerprint = &entity.index_ref.fingerprint_hex;
    let stored_hits = store.hits_for_entity(&domain.domain_id, fingerprint);
    if stored_hits.is_empty() {
        bail!("tokenbridge_no_raw_index: store returned no hits for projected fingerprint");
    }

    assert_not_searchable_by_forbidden_key(&store, domain, RAW_EMAIL, "raw fixture email")?;
    assert_not_searchable_by_forbidden_key(&store, domain, CANONICAL_EMAIL, "canonical email")?;
    assert_not_searchable_by_forbidden_key(
        &store,
        domain,
        &entity.domain_alias,
        "index domain alias",
    )?;

    for hit in &stored_hits {
        assert_hit_safe("stored index hit", hit)?;
    }

    let mut adapter = InMemorySearchAdapter::with_store(store);
    let search_hits = adapter
        .search(
            &handle(entity.index_ref.clone()),
            ValidatedSearchRequest {
                requested_entity_ref: entity.index_ref.clone(),
                filters: Vec::new(),
            },
            10,
        )
        .map_err(|reason| {
            anyhow!("tokenbridge_no_raw_index: adapter search denied unexpectedly: {reason:?}")
        })?;

    if search_hits.is_empty() {
        bail!("tokenbridge_no_raw_index: adapter returned no hits for projected entity_ref");
    }
    for hit in &search_hits {
        assert_hit_safe("adapter search hit", hit)?;
    }

    Ok(())
}

fn assert_not_searchable_by_forbidden_key(
    store: &InMemoryCorpusIndexStore,
    domain: &IndexDomain,
    key: &str,
    label: &str,
) -> Result<()> {
    let hits = store.hits_for_entity(&domain.domain_id, key);
    if !hits.is_empty() {
        bail!("tokenbridge_no_raw_index: {label} became searchable in the corpus index");
    }
    Ok(())
}

fn assert_hit_safe(surface: &str, hit: &IndexSearchHit) -> Result<()> {
    assert_no_forbidden_value(surface, "doc_id", &hit.doc_id)?;
    assert_no_forbidden_value(surface, "snippet", &hit.snippet)?;
    assert_no_current_session_token(surface, &hit.snippet)?;

    if hit.entities.is_empty() {
        bail!("tokenbridge_no_raw_index: {surface} has no owner-side index entities");
    }

    for entity in &hit.entities {
        let expected_alias = domain_alias(&entity.class, &entity.index_ref.fingerprint_hex);
        if entity.domain_alias != expected_alias {
            bail!(
                "tokenbridge_no_raw_index: {surface} entity alias drifted: expected {expected_alias}, got {}",
                entity.domain_alias
            );
        }
        if !hit.snippet.contains(&expected_alias) {
            bail!(
                "tokenbridge_no_raw_index: {surface} snippet does not contain expected domain alias {expected_alias}"
            );
        }

        assert_no_forbidden_value(surface, "entity.domain_alias", &entity.domain_alias)?;
        assert_no_forbidden_value(
            surface,
            "entity.index_ref.domain_id",
            &entity.index_ref.domain_id,
        )?;
        assert_no_forbidden_value(surface, "entity.index_ref.key_id", &entity.index_ref.key_id)?;
        assert_no_forbidden_value(
            surface,
            "entity.index_ref.fingerprint_hex",
            &entity.index_ref.fingerprint_hex,
        )?;
    }

    Ok(())
}

fn assert_no_forbidden_value(surface: &str, field: &str, value: &str) -> Result<()> {
    for forbidden in [RAW_EMAIL, CANONICAL_EMAIL] {
        if value.contains(forbidden) {
            bail!(
                "tokenbridge_no_raw_index: raw PII leaked into {surface} {field}: found {forbidden}"
            );
        }
    }
    Ok(())
}

fn assert_no_current_session_token(surface: &str, snippet: &str) -> Result<()> {
    let mut cursor = 0;
    while let Some(open_rel) = snippet[cursor..].find('<') {
        let open = cursor + open_rel;
        let Some(close_rel) = snippet[open..].find('>') else {
            break;
        };
        let close = open + close_rel + 1;
        let candidate = &snippet[open..close];
        if parse_token_class(candidate).is_some() {
            bail!(
                "tokenbridge_no_raw_index: {surface} retained current-session gaze token {candidate}"
            );
        }
        cursor = close;
    }
    Ok(())
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
        nonce: "nonce-tokenbridge-no-raw-index".to_string(),
        audit_id: "audit-tokenbridge-no-raw-index".to_string(),
    }
}
