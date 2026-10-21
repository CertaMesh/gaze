//! Behavioral pins for the mandatory output SafetyNet in `TokenBridge`.
//!
//! `TokenBridge::enforce_output_safety_net` is the last owner-side check before
//! translated snippets reach an agent. Every other bridge test installs a net that
//! never fires, so disabling any branch of that check left the suite green. Each
//! test below targets one branch and fails when that branch is removed:
//!
//! - a net suspect drops the offending hit, and its raw bytes appear nowhere in
//!   the agent-visible response;
//! - an output pipeline with no registered net denies;
//! - a bridge with no output net installed denies.

use std::ops::Range;

use gaze::{
    Action, Candidate, ClassRule, ConflictTier, DefaultRule, DetectContext, LeakSuspect, LocaleTag,
    PiiClass, Pipeline, Recognizer, SafetyNet, SafetyNetContext, SafetyNetError,
};
use gaze_token_bridge::adapter::InMemoryCorpusIndexStore;
use gaze_token_bridge::bridge::TokenBridge;
use gaze_token_bridge::ingest::CorpusIngestor;
use gaze_token_bridge::projection::HmacDomainProjector;
use gaze_token_bridge::registry::IndexDomainRegistry;
use gaze_token_bridge::{
    BridgeRequest, BridgeSearchOutcome, DenyReason, Principal, RedactionSession, RequestedScope,
};

const POLICY_JSON: &str = include_str!("../fixtures/policy.json");
const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";
const NAME: &str = "Markus Gottschaue";
/// A synthetic value the ingest pipeline deliberately misses. It stands in for
/// any detection gap that leaves raw PII in an owner-side snippet.
const MISSED_RAW: &str = "missed.contact@example.invalid";
const LEAKY_DOC: &str = "doc-net-leaky";
const CLEAN_DOC: &str = "doc-net-clean";

/// Output net that flags every occurrence of one literal value.
#[derive(Debug)]
struct LiteralOutputSafetyNet {
    needle: &'static str,
    locales: Vec<LocaleTag>,
}

impl LiteralOutputSafetyNet {
    fn global(needle: &'static str) -> Self {
        Self {
            needle,
            locales: vec![LocaleTag::Global],
        }
    }
}

impl SafetyNet for LiteralOutputSafetyNet {
    fn id(&self) -> &str {
        "test-literal-output-safety-net"
    }

    fn supported_locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let mut suspects = Vec::new();
        for (start, _) in clean_text.match_indices(self.needle) {
            let span = start..start + self.needle.len();
            if let Some(kind) = context.manifest.diff_against(&span, &PiiClass::Email) {
                suspects.push(LeakSuspect::new(
                    span,
                    PiiClass::Email,
                    self.id(),
                    Some(1.0),
                    kind,
                    "test_literal",
                    context.field_path.map(str::to_string),
                ));
            }
        }
        Ok(suspects)
    }
}

/// Ingest-side recognizer that finds only the demo name, so `MISSED_RAW` stays
/// in the stored snippet as an undetected residual.
#[derive(Debug, Clone)]
struct NameOnlyRecognizer;

impl Recognizer for NameOnlyRecognizer {
    fn id(&self) -> &str {
        "test.name_only"
    }

    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }

    fn token_family(&self) -> &str {
        "counter"
    }

    fn detect(
        &self,
        input: &str,
        _ctx: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze::registry::DetectError> {
        Ok(input
            .match_indices(NAME)
            .map(|(start, _)| name_candidate(start..start + NAME.len()))
            .collect())
    }
}

fn name_candidate(span: Range<usize>) -> Candidate {
    Candidate::new(
        span,
        PiiClass::Name,
        "test.name_only",
        0.9,
        0,
        None,
        "counter",
        "test.name_only",
        ConflictTier::None,
        vec![],
    )
}

fn output_pipeline(net: LiteralOutputSafetyNet) -> Pipeline {
    Pipeline::builder()
        .register_safety_net(net)
        .build()
        .expect("output safety pipeline builds")
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

fn support_session_and_token() -> (RedactionSession, String) {
    let session =
        RedactionSession::ephemeral_for(&support_principal().id).expect("support session builds");
    let token = session
        .tokenize(&PiiClass::Name, NAME)
        .expect("tokenize demo name");
    (session, token)
}

/// Owner-side store with two hits for the same name: one clean, and one whose
/// snippet still carries `MISSED_RAW` because ingest detection missed it.
fn store_with_missed_residual() -> InMemoryCorpusIndexStore {
    let registry = IndexDomainRegistry::from_json(POLICY_JSON).expect("policy loads");
    let domain = registry
        .domain(CUSTOMER_DOMAIN)
        .expect("customer domain exists")
        .clone();
    let pipeline = Pipeline::builder()
        .recognizer(NameOnlyRecognizer)
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("ingest pipeline builds");
    let projector = HmacDomainProjector::new(&registry);
    let ingestor = CorpusIngestor::new(&pipeline, &domain, &projector);

    let mut store = InMemoryCorpusIndexStore::new();
    let leaky = ingestor
        .ingest_text_into_store(
            &mut store,
            LEAKY_DOC,
            &format!("Customer profile {NAME} lists contact {MISSED_RAW}."),
        )
        .expect("leaky doc ingests");
    let clean = ingestor
        .ingest_text_into_store(
            &mut store,
            CLEAN_DOC,
            &format!("Customer profile {NAME} renewed the support plan."),
        )
        .expect("clean doc ingests");

    // Fixture guards: without them the drop assertion below could pass vacuously.
    assert!(
        leaky.snippet.contains(MISSED_RAW),
        "fixture must leave the missed value in the owner-side snippet: {:?}",
        leaky.snippet
    );
    assert!(!leaky.snippet.contains(NAME) && !clean.snippet.contains(NAME));
    assert!(!clean.snippet.contains(MISSED_RAW));
    store
}

/// Every agent-visible rendering of an outcome: JSON of the full response (all
/// fields) plus both Debug forms, so a value hidden in any field is caught.
fn agent_visible_renderings(outcome: &BridgeSearchOutcome) -> Vec<String> {
    let mut renderings = vec![format!("{outcome:?}"), format!("{outcome:#?}")];
    if let BridgeSearchOutcome::Allowed(response) = outcome {
        renderings.push(serde_json::to_string(response).expect("response serializes"));
    }
    renderings
}

#[test]
fn output_net_suspect_drops_the_hit_and_its_raw_value_never_reaches_the_agent() {
    let mut bridge =
        TokenBridge::from_policy_json_and_store(POLICY_JSON, store_with_missed_residual())
            .expect("store bridge builds")
            .with_output_safety_net(
                output_pipeline(LiteralOutputSafetyNet::global(MISSED_RAW)),
                vec![LocaleTag::Global],
            );
    let (session, token) = support_session_and_token();

    let outcome = bridge.search(&session, &support_customer_request(&token));

    for rendering in agent_visible_renderings(&outcome) {
        assert!(
            !rendering.contains(MISSED_RAW),
            "net-flagged raw value reached agent-visible output: {rendering}"
        );
        assert!(!rendering.contains(NAME), "raw name leaked: {rendering}");
    }
    let response = outcome.unwrap_allowed();
    let doc_ids = response
        .results
        .iter()
        .map(|hit| hit.doc_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        doc_ids,
        vec![CLEAN_DOC],
        "the flagged hit must be dropped and the clean hit kept"
    );
    assert!(response.results[0].snippet.contains(&token));
}

#[test]
fn output_pipeline_with_no_registered_net_denies() {
    let (session, token) = support_session_and_token();
    let request = support_customer_request(&token);

    // Control: the same request allows once a real net is registered, so the
    // deny below comes from the empty output pipeline and nothing else.
    let mut control = TokenBridge::demo()
        .expect("demo bridge builds")
        .with_output_safety_net(
            output_pipeline(LiteralOutputSafetyNet::global(MISSED_RAW)),
            vec![LocaleTag::Global],
        );
    assert!(!control
        .search(&session, &request)
        .unwrap_allowed()
        .results
        .is_empty());

    let empty_output_pipeline = Pipeline::builder()
        .build()
        .expect("net-free pipeline builds");
    let mut bridge = TokenBridge::demo()
        .expect("demo bridge builds")
        .with_output_safety_net(empty_output_pipeline, vec![LocaleTag::Global]);

    assert_eq!(
        bridge.search(&session, &request),
        BridgeSearchOutcome::Denied(DenyReason::TranslatorFailed)
    );
}

#[test]
fn bridge_without_output_net_denies() {
    let mut bridge = TokenBridge::demo().expect("demo bridge builds");
    let (session, token) = support_session_and_token();

    assert_eq!(
        bridge.search(&session, &support_customer_request(&token)),
        BridgeSearchOutcome::Denied(DenyReason::TranslatorFailed)
    );
}
