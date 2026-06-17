//! Runnable, synthetic-data-only walkthrough of the gaze-token-bridge owner-side
//! authorization + translation bridge.
//!
//! Run with:
//!
//! ```text
//! cargo run -p gaze-token-bridge --example local_demo
//! ```
//!
//! What it proves, end to end, over a SYNTHETIC corpus (no real PII anywhere):
//!   1. `TokenBridge::demo()` ingests the synthetic corpus into two policy-scoped
//!      IndexDomains using the real Track B redact-before-index pipeline.
//!   2. A support-bound `RedactionSession` mints a session token for a name — the
//!      agent only ever sees the token, never the raw name.
//!   3. An ALLOWED search returns snippets carrying *session* tokens (not raw PII,
//!      not index aliases).
//!   4. A DENIED search fails closed with a single uniform line (no typed-reason
//!      oracle is leaked to the agent).
//!   5. Every agent-visible byte is scanned for raw fixture values; the program
//!      fails loudly (panics, non-zero exit) if any leak is found.
//!
//! This is a demo of the *runtime*, not of detector quality: the corpus is redacted
//! at ingest by a deterministic literal detector over known synthetic values.

use gaze::PiiClass;
use gaze_token_bridge::bridge::{synthetic_docs, TokenBridge};
use gaze_token_bridge::{
    BridgeRequest, BridgeSearchOutcome, BridgeSearchResponse, Principal, RedactionSession,
    RequestedScope,
};

// Owner-side domain identifiers (`tenant/corpus/version`). These live in policy
// config; the LLM never chooses them.
const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";
const LEGAL_DOMAIN: &str = "tenant_demo/legal_docs/v1";

// The canonical happy-path entity from the synthetic corpus.
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

/// Build a bridge request. `purpose` here is the request-stated purpose; the
/// owner-side policy gate ignores it and substitutes the owner-bound purpose
/// resolved from config (so an LLM cannot widen its own authority).
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

/// Every raw value in the synthetic corpus — the exact strings that must NEVER
/// appear in agent-visible output. Derived from `synthetic_docs()` so the scan
/// stays in lockstep with the fixture.
fn raw_fixture_values() -> Vec<String> {
    let mut values = Vec::new();
    for doc in synthetic_docs() {
        values.push(doc.name.to_string());
        values.push(doc.email.to_string());
        values.push(doc.customer_id.to_string());
        values.push(doc.organization.to_string());
    }
    values
}

fn print_header(step: &str) {
    println!("\n=== {step} ===");
}

fn print_results(response: &BridgeSearchResponse) {
    for hit in &response.results {
        println!("  [{}] {}", hit.doc_id, hit.snippet);
    }
}

fn main() {
    println!("gaze-token-bridge — local demo (SYNTHETIC DATA ONLY)");
    println!("Owner-side authorization + translation bridge. Zero raw PII reaches the agent.");

    // -- Step 1: ingest the synthetic corpus into two policy-scoped IndexDomains. --
    print_header("Step 1 — ingest synthetic corpus (redact-before-index)");
    let mut bridge = TokenBridge::demo().expect("demo bridge builds");
    let doc_count = synthetic_docs().len();
    println!(
        "Ingested {doc_count} synthetic docs into 2 IndexDomains via the Track B \
         redact-before-index pipeline:"
    );
    println!("  - {CUSTOMER_DOMAIN}");
    println!("  - {LEGAL_DOMAIN}");
    println!("Raw values were tokenized at ingest; the index holds only projected aliases.");

    // -- Step 2: a support-bound session mints a token for the lookup name. --
    print_header("Step 2 — support session mints a token for the lookup name");
    let session =
        RedactionSession::ephemeral_for(&support_principal().id).expect("ephemeral session builds");
    // The raw name is OWNER-SIDE input — printed once here, labeled, and never again.
    println!("owner-side input (never sent to the agent): name = \"{DEMO_NAME}\"");
    let name_token = session
        .tokenize(&PiiClass::Name, DEMO_NAME)
        .expect("tokenize name");
    println!("session token the agent sees instead:        {name_token}");

    // Collect every agent-visible response so we can scan it for leaks at the end.
    let mut agent_visible: Vec<BridgeSearchResponse> = Vec::new();

    // -- Step 3: ALLOWED — support may search customer_docs. --
    print_header("Step 3 — ALLOWED: support -> customer_docs");
    let allowed_request = bridge_request(
        support_principal(),
        "support_search",
        "support_lookup",
        CUSTOMER_DOMAIN,
        &name_token,
    );
    match bridge.search(&session, &allowed_request) {
        BridgeSearchOutcome::Allowed(response) => {
            println!("ALLOWED. target_domain = {}", response.target_domain);
            println!("session-translated snippets (note the <...:Class_n> session tokens):");
            print_results(&response);
            agent_visible.push(response);
        }
        BridgeSearchOutcome::Denied(_) => {
            panic!("expected support -> customer_docs to be ALLOWED");
        }
    }

    // -- Step 4: DENIED — support may NOT search legal_docs. --
    print_header("Step 4 — DENIED: support -> legal_docs");
    let denied_request = bridge_request(
        support_principal(),
        "legal_search",
        "legal_lookup",
        LEGAL_DOMAIN,
        &name_token,
    );
    match bridge.search(&session, &denied_request) {
        BridgeSearchOutcome::Allowed(_) => {
            panic!("expected support -> legal_docs to be DENIED");
        }
        // No-oracle posture: the agent gets ONE uniform line. The typed DenyReason
        // is owner-side audit only — never surfaced to the agent.
        BridgeSearchOutcome::Denied(_) => {
            println!("DENIED (authorization failed)");
        }
    }

    // -- Step 4 (contrast): admin MAY search legal_docs, in its own session. --
    print_header("Step 4 (contrast) — ALLOWED: admin -> legal_docs");
    let admin_session =
        RedactionSession::ephemeral_for(&admin_principal().id).expect("admin session builds");
    let admin_token = admin_session
        .tokenize(&PiiClass::Name, DEMO_NAME)
        .expect("tokenize name for admin");
    let admin_request = bridge_request(
        admin_principal(),
        "legal_search",
        "legal_lookup",
        LEGAL_DOMAIN,
        &admin_token,
    );
    match bridge.search(&admin_session, &admin_request) {
        BridgeSearchOutcome::Allowed(response) => {
            println!("ALLOWED. target_domain = {}", response.target_domain);
            print_results(&response);
            agent_visible.push(response);
        }
        BridgeSearchOutcome::Denied(_) => {
            panic!("expected admin -> legal_docs to be ALLOWED");
        }
    }

    // -- Step 5: prove no raw PII reached the agent. --
    print_header("Step 5 — assert NO raw PII in agent-visible output");
    let raws = raw_fixture_values();
    let agent_visible_json =
        serde_json::to_string(&agent_visible).expect("agent-visible responses serialize");
    for raw in &raws {
        assert!(
            !agent_visible_json.contains(raw),
            "LEAK: raw fixture value {raw:?} appeared in agent-visible output"
        );
    }
    println!(
        "Scanned {} bytes of agent-visible JSON against {} raw fixture values.",
        agent_visible_json.len(),
        raws.len()
    );
    println!("✅ NO RAW PII IN AGENT-VISIBLE OUTPUT");
    println!(
        "audit events recorded (one per bridge decision): {}",
        bridge.audit().len()
    );
}
