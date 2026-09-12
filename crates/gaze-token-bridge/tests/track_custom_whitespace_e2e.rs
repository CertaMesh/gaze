//! Regression coverage for custom-class whitespace canonicalization in
//! `CanonicalEntity::from_raw`, exercised through the real `TokenBridge::search`
//! (authorize -> execute -> adapter -> translate) path.
//!
//! The bridge indexes and queries by an HMAC fingerprint derived solely from
//! `CanonicalEntity::canonical_value`. If two raw values that denote the same
//! logical entity canonicalize differently, their fingerprints diverge and the
//! fingerprint-keyed adapter lookup silently misses. These tests pin the
//! contract that internal-whitespace differences must collapse for custom
//! classes, and that `Email` (which cannot contain internal whitespace) keeps
//! its trim-only path.

use gaze::{
    Action, ClassRule, DefaultRule, Detection, Detector, LeakSuspect, LocaleTag, PiiClass,
    Pipeline, SafetyNet, SafetyNetContext, SafetyNetError,
};
use gaze_token_bridge::adapter::InMemoryCorpusIndexStore;
use gaze_token_bridge::bridge::TokenBridge;
use gaze_token_bridge::ingest::CorpusIngestor;
use gaze_token_bridge::model::CanonicalEntity;
use gaze_token_bridge::projection::HmacDomainProjector;
use gaze_token_bridge::registry::IndexDomainRegistry;
use gaze_token_bridge::{
    BridgeRequest, BridgeSearchOutcome, Principal, RedactionSession, RequestedScope,
};

const POLICY_JSON: &str = include_str!("../fixtures/policy.json");
const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";

#[derive(Debug)]
struct NoopOutputSafetyNet;

impl SafetyNet for NoopOutputSafetyNet {
    fn id(&self) -> &str {
        "test-noop-output-safety-net"
    }

    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }

    fn check(
        &self,
        _clean_text: &str,
        _context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        Ok(Vec::new())
    }
}

fn with_test_output_safety(bridge: TokenBridge) -> TokenBridge {
    let pipeline = Pipeline::builder()
        .register_safety_net(NoopOutputSafetyNet)
        .build()
        .expect("test output safety pipeline");
    bridge.with_output_safety_net(pipeline, vec![LocaleTag::Global])
}

fn support_principal() -> Principal {
    Principal {
        id: "principal_support_1".to_string(),
        roles: vec!["support".to_string()],
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
    }
}

fn support_customer_request(source_token: &str) -> BridgeRequest {
    BridgeRequest {
        principal: support_principal(),
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
        agent_run_id: "agent-run-1".to_string(),
        conversation_session_id: "conversation-1".to_string(),
        tool_name: "support_search".to_string(),
        action: "search_documents".to_string(),
        purpose: "support_lookup".to_string(),
        source_token: source_token.to_string(),
        target_domain: CUSTOMER_DOMAIN.to_string(),
        requested_scope: RequestedScope::SameDomain,
    }
}

/// Detect a single literal string as a fixed class. Mirrors the
/// `SyntheticCorpusDetector` pattern but for one (value, class) pair, so the
/// ingested raw value is byte-exact to the literal (including internal
/// whitespace runs).
#[derive(Debug)]
struct LiteralDetector {
    literal: String,
    class: PiiClass,
}

impl Detector for LiteralDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        let mut detections = Vec::new();
        let mut cursor = 0;
        while let Some(offset) = input[cursor..].find(self.literal.as_str()) {
            let start = cursor + offset;
            let end = start + self.literal.len();
            detections.push(Detection::new(
                start..end,
                self.class.clone(),
                "literal.fixture",
            ));
            cursor = end;
        }
        detections
    }
}

/// Build a bridge whose customer-domain corpus contains a single
/// `custom:customer_id` entity whose raw value is `corpus_raw` (the exact bytes
/// ingested, including any internal whitespace runs). Ingest goes through the real
/// `CorpusIngestor` + `HmacDomainProjector`, so the stored fingerprint is produced
/// by the real `CanonicalEntity::from_raw`.
fn bridge_with_custom_corpus(corpus_raw: &str) -> TokenBridge {
    let registry = IndexDomainRegistry::from_json(POLICY_JSON).expect("fixture policy loads");
    let domain = registry
        .domain(CUSTOMER_DOMAIN)
        .expect("customer domain exists")
        .clone();
    let pipeline = Pipeline::builder()
        .detector(LiteralDetector {
            literal: corpus_raw.to_string(),
            class: PiiClass::custom("customer_id"),
        })
        .rule(ClassRule::new(
            PiiClass::custom("customer_id"),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("literal detector pipeline builds");
    let projector = HmacDomainProjector::new(&registry);
    let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector);
    let mut store = InMemoryCorpusIndexStore::new();
    let raw_text = format!("Customer case {corpus_raw} is open.");
    ingestor
        .ingest_text_into_store(&mut store, "case-doc", &raw_text)
        .expect("ingest succeeds");

    let bridge = TokenBridge::from_policy_json_and_store(POLICY_JSON, store)
        .expect("bridge builds over custom store");
    with_test_output_safety(bridge)
}

fn unwrap_allowed(outcome: BridgeSearchOutcome) -> usize {
    match outcome {
        BridgeSearchOutcome::Allowed(response) => response.results.len(),
        BridgeSearchOutcome::Denied(reason) => panic!("expected allow, got deny: {reason:?}"),
    }
}

#[test]
fn from_raw_collapses_internal_whitespace_for_custom_class() {
    let double_space = CanonicalEntity::from_raw(PiiClass::custom("customer_id"), "Case  123");
    let single_space = CanonicalEntity::from_raw(PiiClass::custom("customer_id"), "Case 123");
    let tab = CanonicalEntity::from_raw(PiiClass::custom("customer_id"), "Case\t123");
    let leading_trailing =
        CanonicalEntity::from_raw(PiiClass::custom("customer_id"), "  Case 123  ");

    assert_eq!(double_space.canonical_value, single_space.canonical_value);
    assert_eq!(tab.canonical_value, single_space.canonical_value);
    assert_eq!(
        leading_trailing.canonical_value,
        single_space.canonical_value
    );
    assert_eq!(single_space.canonical_value, "custom:customer_id:case 123");
}

#[test]
fn from_raw_preserves_email_trim_only_normalization() {
    // Email cannot legitimately contain internal whitespace, so it stays on the
    // trim-only path. Identical emails still canonicalize identically.
    let a = CanonicalEntity::from_raw(PiiClass::Email, "Alice@Example.invalid");
    let b = CanonicalEntity::from_raw(PiiClass::Email, "  alice@example.invalid  ");
    assert_eq!(a.canonical_value, b.canonical_value);
    assert_eq!(b.canonical_value, "email:alice@example.invalid");
}

#[test]
fn custom_class_internal_whitespace_divergence_finds_the_hit() {
    // Corpus holds "Case  123" (double space); query tokenizes "Case 123"
    // (single space). The fix collapses both to the same fingerprint, so the
    // adapter's fingerprint-keyed lookup must find the stored hit.
    let mut bridge = bridge_with_custom_corpus("Case  123");
    let session = RedactionSession::ephemeral_for(&support_principal().id).unwrap();
    let token = session
        .tokenize(&PiiClass::custom("customer_id"), "Case 123")
        .unwrap();

    let hit_count = unwrap_allowed(bridge.search(&session, &support_customer_request(&token)));

    assert_eq!(
        hit_count, 1,
        "custom entity with differing internal whitespace must resolve to one hit"
    );
}
