//! Track C — end-to-end bridge runtime acceptance tests.
//!
//! Ports the throwaway spike's 22 tests (`crates/spike-token-bridge`) onto the REAL
//! `TokenBridge::demo()`, which integrates the real Track A policy/projection, Track B
//! redact-before-index ingest + search adapter, and the Track C capability/translate/audit
//! runtime. Call sites are adapted to the frozen contract API (e.g. `ephemeral_for(&str)`,
//! `&BridgeRequest`, `audit()` slice). One extra test covers the filter-field allowlist
//! hardening the bridge adds over the spike.

use gaze::PiiClass;
use gaze_token_bridge::bridge::TokenBridge;
use gaze_token_bridge::{
    AuditDecision, BridgeDecision, BridgeRequest, BridgeSearchResponse, DenyReason, FilterClause,
    Principal, RedactionSession, RequestedScope, SearchHandle, SearchRequest,
};

const POLICY_JSON: &str = include_str!("../fixtures/policy.json");
const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";
const LEGAL_DOMAIN: &str = "tenant_demo/legal_docs/v1";

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

/// A demo bridge plus a support-bound session that has already minted a Name token for
/// "Markus Gottschaue" (the canonical happy-path entity).
fn bridge_and_session() -> (TokenBridge, RedactionSession, String) {
    bridge_and_session_for(&support_principal())
}

fn bridge_and_session_for(principal: &Principal) -> (TokenBridge, RedactionSession, String) {
    let bridge = TokenBridge::demo().unwrap();
    let session = RedactionSession::ephemeral_for(&principal.id).unwrap();
    let token = session
        .tokenize(&PiiClass::Name, "Markus Gottschaue")
        .unwrap();
    (bridge, session, token)
}

/// A demo bridge whose policy is augmented with an elevated cross-domain admin rule
/// (mirrors the Track A `registry_with_elevated_cross_domain_rule` helper). The bundled
/// fixture deliberately ships without this rule; augmenting JSON keeps the fixture frozen.
fn cross_domain_bridge_and_session() -> (TokenBridge, RedactionSession, String) {
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
        "max_entities_per_window": 1000,
        "allow_cross_domain": true,
        "elevated_audit_required": true
    }));
    let policy = serde_json::to_string(&policy).expect("augmented policy serializes");

    let bridge = TokenBridge::from_policy_json(&policy).unwrap();
    let session = RedactionSession::ephemeral_for(&admin_principal().id).unwrap();
    let token = session
        .tokenize(&PiiClass::Name, "Markus Gottschaue")
        .unwrap();
    (bridge, session, token)
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

fn support_legal_request(source_token: &str) -> BridgeRequest {
    BridgeRequest {
        principal: support_principal(),
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
        agent_run_id: "agent-run-1".to_string(),
        conversation_session_id: "conversation-1".to_string(),
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
        agent_run_id: "agent-run-2".to_string(),
        conversation_session_id: "conversation-1".to_string(),
        tool_name: "legal_search".to_string(),
        action: "search_documents".to_string(),
        purpose: "legal_lookup".to_string(),
        source_token: source_token.to_string(),
        target_domain: LEGAL_DOMAIN.to_string(),
        requested_scope: RequestedScope::SameDomain,
    }
}

fn authorize_handle(
    bridge: &mut TokenBridge,
    session: &RedactionSession,
    request: &BridgeRequest,
) -> SearchHandle {
    match bridge.authorize(session, request) {
        BridgeDecision::Allow { handle, .. } => handle,
        BridgeDecision::Deny { reason } => panic!("unexpected deny: {reason:?}"),
    }
}

fn response_json(response: &BridgeSearchResponse) -> String {
    serde_json::to_string(response).unwrap()
}

fn assert_no_raw_fixture_values(value: &str) {
    for raw in [
        "Markus Gottschaue",
        "markus@example.invalid",
        "91A",
        "Globex GmbH",
        "Dr. Schmidt",
        "schmidt@example.invalid",
        "72B",
        "Initech AG",
        "Lena Torres",
        "lena@example.invalid",
        "48C",
        "Umbrella LLC",
        "Prof. Weber",
        "weber@example.invalid",
        "54D",
        "Soylent GmbH",
        "Ana Rossi",
        "ana@example.invalid",
        "33E",
        "Vehement Capital Partners",
    ] {
        assert!(
            !value.contains(raw),
            "agent-visible output leaked raw fixture value {raw}"
        );
    }
}

#[test]
fn support_search_name_in_customer_docs_allows_and_returns_translated_snippets() {
    let (mut bridge, session, token) = bridge_and_session();

    let response = bridge
        .search(&session, &support_customer_request(&token))
        .unwrap_allowed();
    let json = response_json(&response);

    assert_eq!(response.target_domain, CUSTOMER_DOMAIN);
    assert!(!response.results.is_empty());
    assert!(json.contains(&token));
    assert_no_raw_fixture_values(&json);
}

#[test]
fn support_search_in_legal_docs_denies() {
    let (mut bridge, session, token) = bridge_and_session();

    let reason = bridge
        .search(&session, &support_legal_request(&token))
        .unwrap_denied();

    assert_eq!(reason, DenyReason::PrincipalNotAllowed);
}

#[test]
fn admin_search_in_legal_docs_allows() {
    let (mut bridge, session, token) = bridge_and_session_for(&admin_principal());

    let response = bridge
        .search(&session, &admin_legal_request(&token))
        .unwrap_allowed();

    assert_eq!(response.target_domain, LEGAL_DOMAIN);
    assert!(!response.results.is_empty());
    assert_no_raw_fixture_values(&response_json(&response));
}

#[test]
fn guessed_fabricated_token_not_in_session_manifest_denies() {
    let (mut bridge, session, _) = bridge_and_session();

    let reason = bridge
        .search(&session, &support_customer_request("<fffffff0:Name_99>"))
        .unwrap_denied();

    assert_eq!(reason, DenyReason::UnknownToken);
}

#[test]
fn malformed_token_denies() {
    let (mut bridge, session, _) = bridge_and_session();

    let reason = bridge
        .search(&session, &support_customer_request("Name_1"))
        .unwrap_denied();

    assert_eq!(reason, DenyReason::MalformedToken);
}

#[test]
fn token_minted_in_different_session_denies() {
    let (mut bridge, session, _) = bridge_and_session();
    let other_session = RedactionSession::ephemeral_for(&support_principal().id).unwrap();
    let foreign_token = other_session
        .tokenize(&PiiClass::Name, "Markus Gottschaue")
        .unwrap();

    let reason = bridge
        .search(&session, &support_customer_request(&foreign_token))
        .unwrap_denied();

    assert_eq!(reason, DenyReason::UnknownToken);
}

#[test]
fn entity_class_not_allowed_in_target_domain_denies() {
    let (mut bridge, session, _) = bridge_and_session_for(&admin_principal());
    let customer_id_token = session
        .tokenize(&PiiClass::custom("customer_id"), "91A")
        .unwrap();

    let reason = bridge
        .search(&session, &admin_legal_request(&customer_id_token))
        .unwrap_denied();

    assert_eq!(reason, DenyReason::ClassNotAllowed);
}

#[test]
fn no_matching_allow_rule_denies_by_default() {
    let (mut bridge, session, token) = bridge_and_session();
    let mut request = support_customer_request(&token);
    request.action = "export_documents".to_string();

    let reason = bridge.search(&session, &request).unwrap_denied();

    assert_eq!(reason, DenyReason::NoMatchingAllowRule);
}

#[test]
fn expired_search_handle_denies() {
    let (mut bridge, session, token) = bridge_and_session();
    let handle = authorize_handle(&mut bridge, &session, &support_customer_request(&token));

    bridge.advance_clock(10);

    let reason = bridge.execute_handle(&session, &handle).unwrap_err();
    assert_eq!(reason, DenyReason::ExpiredHandle);
}

#[test]
fn replayed_search_handle_denies() {
    let (mut bridge, session, token) = bridge_and_session();
    let handle = authorize_handle(&mut bridge, &session, &support_customer_request(&token));

    let first = bridge.execute_handle(&session, &handle);
    let second = bridge.execute_handle(&session, &handle);

    assert!(first.is_ok());
    assert_eq!(second.unwrap_err(), DenyReason::ReplayedHandle);
}

#[test]
fn cross_domain_search_denies_by_default() {
    let (mut bridge, session, token) = bridge_and_session();
    let mut request = support_customer_request(&token);
    request.requested_scope = RequestedScope::CrossDomain {
        source_domain: LEGAL_DOMAIN.to_string(),
    };

    let reason = bridge.search(&session, &request).unwrap_denied();

    assert_eq!(reason, DenyReason::CrossDomainDenied);
}

#[test]
fn cross_domain_search_with_elevated_admin_policy_allows_and_is_audited() {
    let (mut bridge, session, token) = cross_domain_bridge_and_session();
    let mut request = admin_legal_request(&token);
    request.requested_scope = RequestedScope::CrossDomain {
        source_domain: CUSTOMER_DOMAIN.to_string(),
    };

    let decision = bridge.authorize(&session, &request);

    match decision {
        BridgeDecision::Allow { elevated_audit, .. } => assert!(elevated_audit),
        BridgeDecision::Deny { reason } => panic!("unexpected deny: {reason:?}"),
    }
    assert_eq!(bridge.audit().len(), 1);
    assert!(bridge.audit()[0].elevated_audit);
}

#[test]
fn returned_snippets_are_translated_to_current_session_tokens() {
    let (mut bridge, session, token) = bridge_and_session();
    let alias = bridge
        .primary_alias(CUSTOMER_DOMAIN, "cust-001", &PiiClass::Name)
        .unwrap();

    let response = bridge
        .search(&session, &support_customer_request(&token))
        .unwrap_allowed();
    let json = response_json(&response);

    assert!(!json.contains(&alias));
    assert!(!json.contains("<Name_"));
    assert!(json.contains(&token));
    assert!(json.contains(":Name_1>"));
}

#[test]
fn newly_discovered_snippet_entities_get_fresh_current_session_tokens() {
    let (mut bridge, session, token) = bridge_and_session();
    assert_eq!(session.tokens().len(), 1);

    let response = bridge
        .search(&session, &support_customer_request(&token))
        .unwrap_allowed();
    let json = response_json(&response);

    assert!(session.tokens().len() >= 4);
    assert!(json.contains(":Email_1>"));
    assert!(json.contains(":Organization_1>"));
    assert!(json.contains(":Custom:customer_id_1>"));
}

#[test]
fn raw_pii_never_appears_in_agent_visible_output() {
    let (mut bridge, session, token) = bridge_and_session();

    let response = bridge
        .search(&session, &support_customer_request(&token))
        .unwrap_allowed();

    assert_no_raw_fixture_values(&response_json(&response));
}

#[test]
fn one_bridge_audit_event_is_written_per_bridge_decision() {
    let (mut bridge, session, token) = bridge_and_session();

    let _ = bridge.authorize(&session, &support_customer_request(&token));
    let _ = bridge.authorize(&session, &support_customer_request("not-a-token"));

    assert_eq!(bridge.audit().len(), 2);
    assert_eq!(bridge.audit()[0].decision, AuditDecision::Allow);
    assert_eq!(bridge.audit()[1].decision, AuditDecision::Deny);
    assert!(bridge.audit()[0].raw_sha256.is_some());
    assert!(bridge.audit()[1].raw_sha256.is_none());
}

#[test]
fn session_manifest_is_never_exposed_in_agent_visible_output() {
    let (mut bridge, session, token) = bridge_and_session();

    let response = bridge
        .search(&session, &support_customer_request(&token))
        .unwrap_allowed();
    let json = response_json(&response);

    for forbidden in ["manifest", "snapshot", "restore", "raw", "value_by_token"] {
        assert!(
            !json.contains(forbidden),
            "exposed forbidden field {forbidden}"
        );
    }
}

#[test]
fn index_domain_alias_and_fingerprint_never_appear_in_agent_visible_output() {
    let (mut bridge, session, token) = bridge_and_session();
    let alias = bridge
        .primary_alias(CUSTOMER_DOMAIN, "cust-001", &PiiClass::Name)
        .unwrap();
    let fingerprint = bridge
        .primary_fingerprint(CUSTOMER_DOMAIN, "cust-001", &PiiClass::Name)
        .unwrap();

    let response = bridge
        .search(&session, &support_customer_request(&token))
        .unwrap_allowed();
    let json = response_json(&response);

    assert!(!json.contains(&alias));
    assert!(!json.contains(&fingerprint));
}

#[test]
fn handle_reused_with_different_entity_ref_denies() {
    let (mut bridge, session, markus_token) = bridge_and_session();
    let schmidt_token = session.tokenize(&PiiClass::Name, "Dr. Schmidt").unwrap();

    let markus_handle = authorize_handle(
        &mut bridge,
        &session,
        &support_customer_request(&markus_token),
    );
    let schmidt_handle = authorize_handle(
        &mut bridge,
        &session,
        &support_customer_request(&schmidt_token),
    );

    let request = SearchRequest {
        requested_entity_ref: schmidt_handle.entity_ref,
        filters: Vec::new(),
    };
    let reason = bridge
        .execute_search_request(&session, &markus_handle, request)
        .unwrap_err();

    assert_eq!(reason, DenyReason::EntityRefMismatch);
}

#[test]
fn owner_bound_purpose_ignores_llm_override_and_support_legal_still_denies() {
    let (mut bridge, session, token) = bridge_and_session();
    let mut customer_request = support_customer_request(&token);
    customer_request.purpose = "legal_review".to_string();

    let handle = authorize_handle(&mut bridge, &session, &customer_request);
    assert_eq!(handle.purpose, "support_lookup");

    let mut legal_request = support_legal_request(&token);
    legal_request.purpose = "legal_review".to_string();
    let reason = bridge.search(&session, &legal_request).unwrap_denied();

    assert_eq!(reason, DenyReason::PrincipalNotAllowed);
}

#[test]
fn raw_filter_values_are_projected_before_adapter_receives_request() {
    let (mut bridge, session, token) = bridge_and_session();
    let handle = authorize_handle(&mut bridge, &session, &support_customer_request(&token));
    // Filter on an allowed field; the raw PII value must still be projected owner-side
    // before the adapter sees it (the bridge enforces the handle's filter allowlist).
    let request = SearchRequest {
        requested_entity_ref: handle.entity_ref.clone(),
        filters: vec![FilterClause {
            field: "doc_id".to_string(),
            class: PiiClass::Name,
            value: "Markus Gottschaue".to_string(),
        }],
    };

    let response = bridge
        .execute_search_request(&session, &handle, request)
        .unwrap();
    let filters = serde_json::to_string(bridge.last_adapter_filters()).unwrap();

    assert!(!response.results.is_empty());
    assert!(filters.contains("projected:"));
    assert!(!filters.contains("Markus Gottschaue"));
}

#[test]
fn session_bound_to_principal_rejects_reuse_by_different_principal() {
    let (mut bridge, session, token) = bridge_and_session();

    let reason = bridge
        .search(&session, &admin_legal_request(&token))
        .unwrap_denied();

    assert_eq!(reason, DenyReason::SessionPrincipalMismatch);
    assert_eq!(session.session_id(), "conversation:principal_support_1");
}

/// Track-C hardening over the spike: a filter field outside the handle's allowlist fails
/// closed instead of being silently projected (default-deny — north-star axis 1).
#[test]
fn filter_field_outside_handle_allowlist_denies() {
    let (mut bridge, session, token) = bridge_and_session();
    let handle = authorize_handle(&mut bridge, &session, &support_customer_request(&token));
    let request = SearchRequest {
        requested_entity_ref: handle.entity_ref.clone(),
        filters: vec![FilterClause {
            field: "customer_name".to_string(),
            class: PiiClass::Name,
            value: "Markus Gottschaue".to_string(),
        }],
    };

    let reason = bridge
        .execute_search_request(&session, &handle, request)
        .unwrap_err();

    assert_eq!(reason, DenyReason::NoMatchingAllowRule);
}
