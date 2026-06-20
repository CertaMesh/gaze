//! `search_documents` chokepoint tool acceptance tests.
//!
//! Drives [`SearchDocumentsTool::run`] directly: the sealed `gaze_mcp_core::ToolCtx`
//! cannot be constructed outside its crate, so the unit-testable sync core is the
//! seam. Asserts the owner-side session model (token resolves owner-side), the
//! never-leak invariant (no raw PII / alias / fingerprint in agent output), and the
//! no-oracle deny (no `DenyReason` variant ever surfaces; denies are
//! indistinguishable across causes).
//!
//! The whole file is gated on the `chokepoint` feature; with the feature off it
//! compiles to an empty crate, so the default test graph is unchanged.
#![cfg(feature = "chokepoint")]

use std::collections::HashMap;

use gaze::{
    LeakSuspect, LocaleTag, PiiClass, Pipeline, SafetyNet, SafetyNetContext, SafetyNetError,
};
use gaze_token_bridge::bridge::{SearchDocumentsTool, TokenBridge};
use gaze_token_bridge::Principal;
use serde_json::{json, Value};

const POLICY_JSON: &str = include_str!("../fixtures/policy.json");
const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";
const LEGAL_DOMAIN: &str = "tenant_demo/legal_docs/v1";
const SUPPORT_ID: &str = "principal_support_1";

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
        id: SUPPORT_ID.to_string(),
        roles: vec!["support".to_string()],
        tenant_id: "tenant_demo".to_string(),
        workspace_id: "workspace_demo".to_string(),
    }
}

/// The bundled demo policy, rewritten so the single agent-tier `search_documents`
/// tool is the authorized policy `tool_name` in both domains. The bundled fixture
/// keys on `support_search` / `legal_search`; this tool fixes its policy tool_name
/// to its wire name, so adopters author rules under `search_documents`. Patching
/// the frozen fixture in-test keeps projection keys correct without editing it.
fn search_documents_policy() -> String {
    let mut policy: Value = serde_json::from_str(POLICY_JSON).expect("fixture policy parses");
    for domain in policy["domains"]
        .as_array_mut()
        .expect("policy has domains array")
    {
        domain["allowed_tools"] = json!(["search_documents"]);
    }
    for rule in policy["rules"]
        .as_array_mut()
        .expect("policy has rules array")
    {
        rule["tool_name"] = json!("search_documents");
    }
    serde_json::to_string(&policy).expect("augmented policy serializes")
}

fn demo_bridge() -> TokenBridge {
    with_test_output_safety(
        TokenBridge::from_policy_json(&search_documents_policy())
            .expect("bridge builds from policy"),
    )
}

/// A tool wired with the support principal over the search_documents-keyed bridge.
fn support_tool() -> SearchDocumentsTool {
    let mut principals = HashMap::new();
    let principal = support_principal();
    principals.insert(principal.id.clone(), principal);
    SearchDocumentsTool::new(demo_bridge(), principals)
}

/// Every `DenyReason` variant name (error.rs). None may appear in agent output.
const DENY_REASON_VARIANTS: &[&str] = &[
    "MalformedToken",
    "UnknownToken",
    "TenantOrWorkspaceMismatch",
    "DomainNotFound",
    "PrincipalNotAllowed",
    "ClassNotAllowed",
    "NoMatchingAllowRule",
    "CrossDomainDenied",
    "ExpiredHandle",
    "ReplayedHandle",
    "HandleDomainMismatch",
    "EntityRefMismatch",
    "SessionPrincipalMismatch",
    "SnippetsDisabled",
    "TranslatorFailed",
];

/// Raw fixture values that must never reach agent-visible output.
const RAW_FIXTURE_VALUES: &[&str] = &[
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
];

fn assert_no_raw_leak(json_str: &str) {
    for raw in RAW_FIXTURE_VALUES {
        assert!(
            !json_str.contains(raw),
            "agent-visible output leaked raw fixture value {raw}"
        );
    }
}

fn assert_no_deny_oracle(json_str: &str) {
    for variant in DENY_REASON_VARIANTS {
        assert!(
            !json_str.contains(variant),
            "agent-visible deny leaked DenyReason variant {variant}"
        );
    }
}

#[test]
fn support_search_customer_docs_allows_with_translated_results() {
    // Capture owner-side alias + fingerprint before the bridge moves into the tool,
    // so we can prove neither leaks into agent-visible output.
    let bridge = demo_bridge();
    let alias = bridge
        .primary_alias(CUSTOMER_DOMAIN, "cust-001", &PiiClass::Name)
        .expect("known customer Name alias");
    let fingerprint = bridge
        .primary_fingerprint(CUSTOMER_DOMAIN, "cust-001", &PiiClass::Name)
        .expect("known customer Name fingerprint");

    let mut principals = HashMap::new();
    let principal = support_principal();
    principals.insert(principal.id.clone(), principal);
    let tool = SearchDocumentsTool::new(bridge, principals);

    // Owner-side: the owner tokenizes the PII; the agent only ever sees the token.
    let token = tool
        .tokenize_for(SUPPORT_ID, &PiiClass::Name, "Markus Gottschaue")
        .expect("owner-side token mint");

    let out = tool.run(
        SUPPORT_ID,
        &json!({ "source_token": token, "target_domain": CUSTOMER_DOMAIN }),
    );

    assert_eq!(out["authorized"], json!(true));
    assert_eq!(out["target_domain"], json!(CUSTOMER_DOMAIN));
    assert!(
        !out["results"].as_array().expect("results array").is_empty(),
        "expected non-empty translated results"
    );

    let json_str = out.to_string();
    assert!(
        json_str.contains(&token),
        "translated results must carry the conversation token"
    );
    assert_no_raw_leak(&json_str);
    assert!(
        !json_str.contains(&alias),
        "agent output leaked the owner-side domain alias"
    );
    assert!(
        !json_str.contains(&fingerprint),
        "agent output leaked the owner-side projection fingerprint"
    );
}

#[test]
fn support_search_legal_docs_denies_uniformly() {
    let tool = support_tool();
    let token = tool
        .tokenize_for(SUPPORT_ID, &PiiClass::Name, "Markus Gottschaue")
        .expect("owner-side token mint");

    let out = tool.run(
        SUPPORT_ID,
        &json!({ "source_token": token, "target_domain": LEGAL_DOMAIN }),
    );

    assert_eq!(out["authorized"], json!(false));
    assert_eq!(out["target_domain"], json!(LEGAL_DOMAIN));
    assert_eq!(out["results"], json!([]));
    assert_no_deny_oracle(&out.to_string());
}

#[test]
fn unknown_principal_denies_uniformly() {
    let tool = support_tool();

    let out = tool.run(
        "principal_ghost",
        &json!({ "source_token": "<deadbeef:Name_1>", "target_domain": CUSTOMER_DOMAIN }),
    );

    assert_eq!(out["authorized"], json!(false));
    assert_eq!(out["target_domain"], json!(CUSTOMER_DOMAIN));
    assert_eq!(out["results"], json!([]));
    assert_no_deny_oracle(&out.to_string());
}

#[test]
fn malformed_args_deny_uniformly() {
    let tool = support_tool();

    for args in [
        json!({}),                                                           // both fields missing
        json!({ "source_token": "<deadbeef:Name_1>" }), // missing target_domain
        json!({ "target_domain": CUSTOMER_DOMAIN }),    // missing source_token
        json!({ "source_token": 42, "target_domain": CUSTOMER_DOMAIN }), // wrong type
        json!({ "source_token": "<deadbeef:Name_1>", "target_domain": "" }), // empty domain
    ] {
        let out = tool.run(SUPPORT_ID, &args);
        assert_eq!(out["authorized"], json!(false), "args {args} must deny");
        assert_eq!(
            out["results"],
            json!([]),
            "args {args} must return no results"
        );
        assert_no_deny_oracle(&out.to_string());
    }
}

#[test]
fn guessed_or_foreign_token_denies_uniformly() {
    let tool = support_tool();
    // A well-formed token never minted in this principal's owner-side session.
    let out = tool.run(
        SUPPORT_ID,
        &json!({ "source_token": "<fffffff0:Name_99>", "target_domain": CUSTOMER_DOMAIN }),
    );

    assert_eq!(out["authorized"], json!(false));
    assert_eq!(out["results"], json!([]));
    assert_no_deny_oracle(&out.to_string());
}

#[test]
fn deny_outputs_are_indistinguishable_across_causes() {
    let tool = support_tool();
    let token = tool
        .tokenize_for(SUPPORT_ID, &PiiClass::Name, "Markus Gottschaue")
        .expect("owner-side token mint");

    // Policy deny (support principal lacks the legal-domain role).
    let policy_deny = tool.run(
        SUPPORT_ID,
        &json!({ "source_token": token, "target_domain": LEGAL_DOMAIN }),
    );
    // Unknown principal, same target_domain echo.
    let unknown_deny = tool.run(
        "principal_ghost",
        &json!({ "source_token": "<deadbeef:Name_1>", "target_domain": LEGAL_DOMAIN }),
    );
    // Unknown-token deny, same target_domain echo.
    let token_deny = tool.run(
        SUPPORT_ID,
        &json!({ "source_token": "<fffffff0:Name_99>", "target_domain": LEGAL_DOMAIN }),
    );

    assert_eq!(
        policy_deny, unknown_deny,
        "deny shape must not reveal whether the principal is known"
    );
    assert_eq!(
        policy_deny, token_deny,
        "deny shape must not reveal whether the token resolved"
    );
}
