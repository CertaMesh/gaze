use gaze::PiiClass;
use gaze_token_bridge::policy::RegistryPolicyGate;
use gaze_token_bridge::registry::IndexDomainRegistry;
use gaze_token_bridge::traits::PolicyGate;
use gaze_token_bridge::util::sha256_hex;
use gaze_token_bridge::{
    AuthGrant, BridgeRequest, DenyReason, PolicyOutcome, Principal, RedactionSession,
    RequestedScope,
};

const POLICY_JSON: &str = include_str!("../fixtures/policy.json");
const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";
const LEGAL_DOMAIN: &str = "tenant_demo/legal_docs/v1";

fn registry() -> IndexDomainRegistry {
    IndexDomainRegistry::from_json(POLICY_JSON).expect("fixture policy loads")
}

fn registry_with_elevated_cross_domain_rule() -> IndexDomainRegistry {
    let mut policy: serde_json::Value =
        serde_json::from_str(POLICY_JSON).expect("fixture policy parses as json");
    let rules = policy
        .get_mut("rules")
        .and_then(serde_json::Value::as_array_mut)
        .expect("fixture policy has rule array");
    rules.push(serde_json::json!({
        "roles": ["admin"],
        "tenant_id": "tenant_demo",
        "workspace_id": "workspace_demo",
        "tool_name": "legal_search",
        "action": "search_documents",
        "purpose": "legal_lookup",
        "target_domain": LEGAL_DOMAIN,
        "allowed_entity_classes": ["name", "email", "organization"],
        "max_results": 20,
        "allow_cross_domain": true,
        "elevated_audit_required": true
    }));

    let policy = serde_json::to_string(&policy).expect("fixture policy serializes");
    IndexDomainRegistry::from_json(&policy).expect("augmented policy loads")
}

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

fn session_with_token(
    principal: &Principal,
    class: PiiClass,
    raw: &str,
) -> (RedactionSession, String) {
    let session = RedactionSession::ephemeral_for(&principal.id).expect("session creates");
    let token = session.tokenize(&class, raw).expect("token mints");
    (session, token)
}

fn support_customer_request(source_token: &str) -> BridgeRequest {
    BridgeRequest {
        principal: support_principal(),
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
        agent_run_id: "agent-run-policy-1".to_string(),
        conversation_session_id: "conversation-policy-1".to_string(),
        tool_name: "support_search".to_string(),
        action: "search_documents".to_string(),
        purpose: "support_lookup".to_string(),
        source_token: source_token.to_string(),
        target_domain: CUSTOMER_DOMAIN.to_string(),
        requested_scope: RequestedScope::SameDomain,
    }
}

fn support_legal_request(source_token: &str) -> BridgeRequest {
    BridgeRequest {
        principal: support_principal(),
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
        agent_run_id: "agent-run-policy-1".to_string(),
        conversation_session_id: "conversation-policy-1".to_string(),
        tool_name: "legal_search".to_string(),
        action: "search_documents".to_string(),
        purpose: "legal_lookup".to_string(),
        source_token: source_token.to_string(),
        target_domain: LEGAL_DOMAIN.to_string(),
        requested_scope: RequestedScope::SameDomain,
    }
}

fn admin_legal_request(source_token: &str) -> BridgeRequest {
    BridgeRequest {
        principal: admin_principal(),
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
        agent_run_id: "agent-run-policy-2".to_string(),
        conversation_session_id: "conversation-policy-1".to_string(),
        tool_name: "legal_search".to_string(),
        action: "search_documents".to_string(),
        purpose: "legal_lookup".to_string(),
        source_token: source_token.to_string(),
        target_domain: LEGAL_DOMAIN.to_string(),
        requested_scope: RequestedScope::SameDomain,
    }
}

fn evaluate(session: &RedactionSession, request: &BridgeRequest) -> PolicyOutcome {
    let registry = registry();
    evaluate_with_registry(&registry, session, request)
}

fn evaluate_with_registry(
    registry: &IndexDomainRegistry,
    session: &RedactionSession,
    request: &BridgeRequest,
) -> PolicyOutcome {
    let mut gate = RegistryPolicyGate::new(registry);
    gate.evaluate(session, request)
}

fn unwrap_grant(outcome: PolicyOutcome) -> AuthGrant {
    match outcome {
        PolicyOutcome::Grant(grant) => grant,
        PolicyOutcome::Deny { .. } => panic!("expected grant"),
    }
}

fn unwrap_deny(outcome: PolicyOutcome) -> (DenyReason, Option<PiiClass>, Option<String>) {
    match outcome {
        PolicyOutcome::Grant(_) => panic!("expected deny"),
        PolicyOutcome::Deny {
            reason,
            entity_class,
            raw_sha256,
        } => (reason, entity_class, raw_sha256),
    }
}

#[test]
fn default_deny_when_no_allow_rule_matches() {
    let principal = support_principal();
    let (session, token) = session_with_token(&principal, PiiClass::Name, "Dr. Schmidt");
    let mut request = support_customer_request(&token);
    request.tool_name = "legal_search".to_string();

    let (reason, entity_class, raw_sha256) = unwrap_deny(evaluate(&session, &request));

    assert_eq!(reason, DenyReason::NoMatchingAllowRule);
    assert_eq!(entity_class, Some(PiiClass::Name));
    assert_eq!(raw_sha256, Some(sha256_hex("Dr. Schmidt")));
}

#[test]
fn class_not_allowed_denies_after_owner_side_resolution() {
    let principal = admin_principal();
    let class = PiiClass::custom("customer_id");
    let (session, token) = session_with_token(&principal, class.clone(), "demo-customer-id-001");

    let (reason, entity_class, raw_sha256) =
        unwrap_deny(evaluate(&session, &admin_legal_request(&token)));

    assert_eq!(reason, DenyReason::ClassNotAllowed);
    assert_eq!(entity_class, Some(class));
    assert_eq!(raw_sha256, Some(sha256_hex("demo-customer-id-001")));
}

#[test]
fn principal_not_allowed_denies_before_token_resolution() {
    let principal = support_principal();
    let (session, token) = session_with_token(&principal, PiiClass::Name, "Dr. Schmidt");

    let (reason, entity_class, raw_sha256) =
        unwrap_deny(evaluate(&session, &support_legal_request(&token)));

    assert_eq!(reason, DenyReason::PrincipalNotAllowed);
    assert_eq!(entity_class, None);
    assert_eq!(raw_sha256, None);
}

#[test]
fn session_principal_mismatch_denies_before_token_resolution() {
    let support = support_principal();
    let (session, token) = session_with_token(&support, PiiClass::Name, "Dr. Schmidt");

    let (reason, entity_class, raw_sha256) =
        unwrap_deny(evaluate(&session, &admin_legal_request(&token)));

    assert_eq!(reason, DenyReason::SessionPrincipalMismatch);
    assert_eq!(entity_class, None);
    assert_eq!(raw_sha256, None);
    assert_eq!(session.session_id(), "conversation:principal_support_1");
}

#[test]
fn owner_bound_purpose_ignores_request_override() {
    let principal = support_principal();
    let (session, token) = session_with_token(&principal, PiiClass::Email, "alice@example.invalid");
    let mut request = support_customer_request(&token);
    request.purpose = "legal_lookup".to_string();

    let grant = unwrap_grant(evaluate(&session, &request));

    assert_eq!(grant.effective_purpose, "support_lookup");
    assert_eq!(grant.entity_class, PiiClass::Email);
}

#[test]
fn support_legal_request_still_denies_with_purpose_override() {
    let principal = support_principal();
    let (session, token) = session_with_token(&principal, PiiClass::Name, "Dr. Schmidt");
    let mut request = support_legal_request(&token);
    request.purpose = "support_lookup".to_string();

    let (reason, entity_class, raw_sha256) = unwrap_deny(evaluate(&session, &request));

    assert_eq!(reason, DenyReason::PrincipalNotAllowed);
    assert_eq!(entity_class, None);
    assert_eq!(raw_sha256, None);
}

#[test]
fn cross_domain_search_denies_by_default() {
    let principal = support_principal();
    let (session, token) = session_with_token(&principal, PiiClass::Name, "Dr. Schmidt");
    let mut request = support_customer_request(&token);
    request.requested_scope = RequestedScope::CrossDomain {
        source_domain: LEGAL_DOMAIN.to_string(),
    };

    let (reason, entity_class, raw_sha256) = unwrap_deny(evaluate(&session, &request));

    assert_eq!(reason, DenyReason::CrossDomainDenied);
    assert_eq!(entity_class, Some(PiiClass::Name));
    assert_eq!(raw_sha256, Some(sha256_hex("Dr. Schmidt")));
}

#[test]
fn cross_domain_elevated_admin_rule_allows() {
    let principal = admin_principal();
    let (session, token) = session_with_token(&principal, PiiClass::Name, "Dr. Schmidt");
    let mut request = admin_legal_request(&token);
    request.requested_scope = RequestedScope::CrossDomain {
        source_domain: CUSTOMER_DOMAIN.to_string(),
    };

    let registry = registry_with_elevated_cross_domain_rule();
    let grant = unwrap_grant(evaluate_with_registry(&registry, &session, &request));

    assert_eq!(grant.effective_purpose, "legal_lookup");
    assert_eq!(grant.entity_class, PiiClass::Name);
    assert_eq!(grant.entity_ref.domain_id, LEGAL_DOMAIN);
    assert!(grant.elevated_audit);
}

#[test]
fn same_domain_grant_contains_policy_metadata() {
    let principal = support_principal();
    let (session, token) = session_with_token(&principal, PiiClass::Name, "Dr. Schmidt");

    let grant = unwrap_grant(evaluate(&session, &support_customer_request(&token)));

    assert_eq!(grant.entity_ref.domain_id, CUSTOMER_DOMAIN);
    assert_eq!(grant.entity_ref.key_id, "customer-v2");
    assert_eq!(grant.entity_class, PiiClass::Name);
    assert_eq!(grant.entity_ref.entity_class, PiiClass::Name);
    assert_eq!(grant.effective_purpose, "support_lookup");
    assert_eq!(grant.max_results, 20);
    assert_eq!(
        grant.allowed_filters,
        vec!["doc_id".to_string(), "corpus_type".to_string()]
    );
    assert!(!grant.elevated_audit);
    assert_eq!(grant.raw_sha256, sha256_hex("Dr. Schmidt"));
}
