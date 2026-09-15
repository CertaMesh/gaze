//! End-to-end pin for the pipeline → bridge-ingest seam under residual coverage.
//!
//! The in-crate ingest tests build `EmittedTokenSpan`s by hand, which proves how
//! `build_index_hit` behaves *given* a fragment but not that the real pipeline
//! hands the bridge one. That gap is not theoretical: mutating the pipeline so a
//! residual fragment is emitted as `EmittedTokenOrigin::Whole` — the exact defect
//! that would push fragment raw bytes into the persistent index as a
//! canonicalized, fingerprinted, query-reachable entity — leaves every existing
//! `gaze-token-bridge` test green.
//!
//! So this drives the real `Pipeline` through the public `CorpusIngestor` API and
//! asserts the protection at the far end. Mislabel a fragment upstream and this
//! file goes red.

use std::ops::Range;

use gaze::{
    Action, Candidate, ConflictTier, DefaultRule, DetectContext, PiiClass, Pipeline, Recognizer,
};
use gaze_token_bridge::ingest::CorpusIngestor;
use gaze_token_bridge::model::{CanonicalEntity, IndexDomain, IndexedEntityRef};
use gaze_token_bridge::traits::DomainProjector;
use gaze_token_bridge::util::sha256_hex;
use gaze_token_bridge::BridgeError;

/// The losing `password` original covers `11..21`; the winning `Name` selection
/// keeps `0..15`. `15..21` (`" right"`) is evidenced but unselected, so residual
/// coverage protects it. Byte 21 (the closing quote) is outside the admitted
/// union and stays in the clear — that is the documented limit, not a defect.
const RAW: &str = "password: \"left right\"\nmarker";
/// The fragment's raw bytes. Nothing owner-side may store these verbatim.
const FRAGMENT_RAW: &str = " right";

#[derive(Debug, Clone)]
struct OverlappingPair;

impl Recognizer for OverlappingPair {
    fn id(&self) -> &str {
        "fixture.overlapping.pair"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        _input: &str,
        _ctx: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze::registry::DetectError> {
        Ok(vec![
            candidate(0..15, PiiClass::Name, "fixture.name"),
            candidate(
                11..21,
                PiiClass::custom("password").expect("valid custom class"),
                "fixture.field",
            ),
        ])
    }
}

fn candidate(span: Range<usize>, class: PiiClass, id: &str) -> Candidate {
    Candidate::new(
        span,
        class,
        id,
        0.9,
        0,
        None,
        "counter",
        id,
        ConflictTier::None,
        vec![],
    )
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

fn domain() -> IndexDomain {
    IndexDomain {
        domain_id: "tenant_demo/customer_docs/v1".to_string(),
        tenant_id: "tenant_demo".to_string(),
        corpus_type: "customer_docs".to_string(),
        purpose: "support_lookup".to_string(),
        allowed_roles: vec!["support".to_string()],
        allowed_tools: vec!["search".to_string()],
        allowed_actions: vec!["search".to_string()],
        allowed_entity_classes: vec![PiiClass::Name],
        snippets_allowed: true,
        raw_restore_allowed: false,
        co_searchable_with: Vec::new(),
        projection_key_id: "projection-key-v1".to_string(),
    }
}

fn pipeline() -> Pipeline {
    Pipeline::builder()
        .recognizer(OverlappingPair)
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .expect("fixture pipeline builds")
}

#[test]
fn a_real_pipeline_residual_never_becomes_an_indexed_entity() {
    let pipeline = pipeline();
    let domain = domain();
    let projector = DeterministicProjector;
    let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector);

    let hit = ingestor
        .ingest_text("doc-residual-1", RAW)
        .expect("ingest succeeds");

    // Guard the fixture itself: if the pipeline stops producing a residual here,
    // this test would pass vacuously and prove nothing.
    let (_, spans, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &gaze::Session::new(gaze::Scope::Ephemeral).expect("session"),
            gaze::RawDocument::Text(RAW.to_string()),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            Default::default(),
        )
        .expect("clean succeeds");
    let fragments = spans
        .iter()
        .filter(|span| span.origin.is_residual_fragment())
        .collect::<Vec<_>>();
    assert_eq!(
        fragments.len(),
        1,
        "expected exactly one residual fragment. If every span says `Whole`, the \
         producer stopped labelling residuals rather than the fixture going stale, \
         and the bridge will index this fragment as an entity. Got: {spans:?}"
    );
    assert_eq!(
        spans.iter().filter(|span| span.origin.is_whole()).count(),
        1,
        "fixture must produce exactly one whole selection, got {spans:?}"
    );
    // The residual takes its class from the representative parent original —
    // here the losing `password` field, not the winning Name. That is precisely
    // why a fragment must not be indexed as an entity: `custom:password`
    // covering " right" would be a forged entity with a real-looking class.
    let fragment_class = fragments[0].class.clone();
    assert_eq!(
        fragment_class,
        PiiClass::custom("password").expect("valid custom class"),
        "residual class should come from the representative parent"
    );

    // 1. The fragment is not an entity, so there is no posting to retrieve it by.
    assert_eq!(
        hit.entities.len(),
        1,
        "a residual fragment must not become an IndexEntity: {:?}",
        hit.entities
    );
    assert!(
        hit.entities
            .iter()
            .all(|entity| entity.raw_value != FRAGMENT_RAW),
        "fragment raw bytes were indexed as an entity value: {:?}",
        hit.entities
    );

    // 2. Its raw bytes never reach the persistent snippet.
    assert!(
        !hit.snippet.contains(FRAGMENT_RAW),
        "fragment raw bytes leaked into the persistent snippet: {:?}",
        hit.snippet
    );

    // 3. It is nonetheless covered, by a location-only placeholder that carries
    //    no value, no fingerprint and no ingest-session token.
    let placeholder = gaze_token_bridge::util::fragment_placeholder(&fragment_class);
    assert!(
        hit.snippet.contains(&placeholder),
        "expected a location-only placeholder in {:?}",
        hit.snippet
    );
    assert!(!gaze_token_bridge::util::contains_domain_alias(
        &placeholder
    ));
}
