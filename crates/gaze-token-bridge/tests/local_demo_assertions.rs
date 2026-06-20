use gaze::{
    LeakSuspect, LocaleTag, PiiClass, Pipeline, SafetyNet, SafetyNetContext, SafetyNetError,
};
use gaze_token_bridge::bridge::{synthetic_docs, SyntheticDoc, TokenBridge};
use gaze_token_bridge::{
    BridgeRequest, BridgeSearchOutcome, BridgeSearchResponse, DenyReason, Principal,
    RedactionSession, RequestedScope,
};

const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";
const LEGAL_DOMAIN: &str = "tenant_demo/legal_docs/v1";
const DEMO_NAME: &str = "Markus Gottschaue";

fn support_principal() -> Principal {
    Principal {
        id: "principal_support_1".to_string(),
        roles: vec!["support".to_string()],
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
    }
}

fn admin_principal() -> Principal {
    Principal {
        id: "principal_admin_1".to_string(),
        roles: vec!["admin".to_string()],
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
    }
}

fn bridge_request(
    principal: Principal,
    tool_name: &str,
    purpose: &str,
    target_domain: &str,
    source_token: &str,
) -> BridgeRequest {
    BridgeRequest {
        principal,
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
        agent_run_id: "agent-run-1".to_string(),
        conversation_session_id: "conversation-1".to_string(),
        tool_name: tool_name.to_string(),
        action: "search_documents".to_string(),
        purpose: purpose.to_string(),
        source_token: source_token.to_string(),
        target_domain: target_domain.to_string(),
        requested_scope: RequestedScope::SameDomain,
    }
}

fn raw_fixture_values(docs: &[SyntheticDoc]) -> Vec<String> {
    let mut values = Vec::new();
    for doc in docs {
        values.push(doc.name.to_string());
        values.push(doc.email.to_string());
        values.push(doc.customer_id.to_string());
        values.push(doc.organization.to_string());
    }
    values
}

#[derive(Debug)]
struct SyntheticFixtureOutputSafetyNet {
    entries: Vec<(String, PiiClass)>,
}

impl SyntheticFixtureOutputSafetyNet {
    fn from_docs(docs: &[SyntheticDoc]) -> Self {
        let mut entries = Vec::with_capacity(docs.len() * 4);
        for doc in docs {
            entries.push((doc.name.to_string(), PiiClass::Name));
            entries.push((doc.email.to_string(), PiiClass::Email));
            entries.push((doc.customer_id.to_string(), PiiClass::custom("customer_id")));
            entries.push((doc.organization.to_string(), PiiClass::Organization));
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
        entries.dedup();
        Self { entries }
    }
}

impl SafetyNet for SyntheticFixtureOutputSafetyNet {
    fn id(&self) -> &str {
        "synthetic-fixture-output-safety-net"
    }

    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let mut suspects = Vec::new();
        for (raw, class) in &self.entries {
            let mut cursor = 0;
            while let Some(offset) = clean_text[cursor..].find(raw.as_str()) {
                let start = cursor + offset;
                let end = start + raw.len();
                let span = start..end;
                if let Some(kind) = context.manifest.diff_against(&span, class) {
                    suspects.push(LeakSuspect::new(
                        span,
                        class.clone(),
                        self.id(),
                        Some(1.0),
                        kind,
                        "synthetic_fixture",
                        context.field_path.map(str::to_string),
                    ));
                }
                cursor = end;
            }
        }
        Ok(suspects)
    }
}

fn bridge_with_fixture_safety(docs: &[SyntheticDoc]) -> TokenBridge {
    let output_safety_pipeline = Pipeline::builder()
        .register_safety_net(SyntheticFixtureOutputSafetyNet::from_docs(docs))
        .build()
        .expect("demo output safety pipeline builds");

    TokenBridge::demo()
        .expect("demo bridge builds")
        .with_output_safety_net(output_safety_pipeline, vec![LocaleTag::Global])
}

fn unwrap_allowed(outcome: BridgeSearchOutcome) -> BridgeSearchResponse {
    match outcome {
        BridgeSearchOutcome::Allowed(response) => response,
        BridgeSearchOutcome::Denied(reason) => panic!("expected allow, got deny: {reason:?}"),
    }
}

fn unwrap_denied(outcome: BridgeSearchOutcome) -> DenyReason {
    match outcome {
        BridgeSearchOutcome::Allowed(response) => {
            panic!("expected deny, got allow: {response:?}")
        }
        BridgeSearchOutcome::Denied(reason) => reason,
    }
}

#[test]
fn local_demo_flow_has_explicit_integration_coverage() {
    let docs = synthetic_docs();
    let mut bridge = bridge_with_fixture_safety(&docs);

    let support = support_principal();
    let session = RedactionSession::ephemeral_for(&support.id).expect("support session builds");
    let name_token = session
        .tokenize(&PiiClass::Name, DEMO_NAME)
        .expect("tokenize support name");

    let support_customer = bridge_request(
        support_principal(),
        "support_search",
        "support_lookup",
        CUSTOMER_DOMAIN,
        &name_token,
    );
    let customer_response = unwrap_allowed(bridge.search(&session, &support_customer));
    assert_eq!(customer_response.target_domain, CUSTOMER_DOMAIN);
    assert!(!customer_response.results.is_empty());

    let support_legal = bridge_request(
        support_principal(),
        "legal_search",
        "legal_lookup",
        LEGAL_DOMAIN,
        &name_token,
    );
    let support_legal_reason = unwrap_denied(bridge.search(&session, &support_legal));
    assert_eq!(support_legal_reason, DenyReason::PrincipalNotAllowed);

    let admin = admin_principal();
    let admin_session = RedactionSession::ephemeral_for(&admin.id).expect("admin session builds");
    let admin_token = admin_session
        .tokenize(&PiiClass::Name, DEMO_NAME)
        .expect("tokenize admin name");
    let admin_request = bridge_request(
        admin,
        "legal_search",
        "legal_lookup",
        LEGAL_DOMAIN,
        &admin_token,
    );
    let legal_response = unwrap_allowed(bridge.search(&admin_session, &admin_request));
    assert_eq!(legal_response.target_domain, LEGAL_DOMAIN);
    assert!(!legal_response.results.is_empty());

    let agent_visible =
        serde_json::to_string(&[customer_response, legal_response]).expect("serialize responses");
    for raw in raw_fixture_values(&docs) {
        assert!(
            !agent_visible.contains(raw.as_str()),
            "agent-visible output leaked raw fixture value {raw:?}"
        );
    }

    assert_eq!(bridge.audit().len(), 3);
}
