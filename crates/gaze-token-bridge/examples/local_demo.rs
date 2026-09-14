//! Synthetic-data walkthrough of the gaze-token-bridge owner-side authorization and
//! translation bridge.
//!
//! Run with:
//!
//! ```text
//! cargo run -p gaze-token-bridge --example local_demo
//! ```
//!
//! This uses bundled synthetic fixtures to show the bridge flow. For a
//! bring-your-own-data redaction example, run:
//!
//! ```text
//! cargo run -p gaze-pii --example scan_folder -- --path ./my-data
//! ```

use gaze::{
    LeakSuspect, LocaleTag, PiiClass, Pipeline, SafetyNet, SafetyNetContext, SafetyNetError,
};
use gaze_token_bridge::bridge::{synthetic_docs, TokenBridge};
use gaze_token_bridge::{
    BridgeRequest, BridgeSearchOutcome, BridgeSearchResponse, Principal, RedactionSession,
    RequestedScope,
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

#[derive(Debug)]
struct SyntheticFixtureOutputSafetyNet {
    entries: Vec<(String, PiiClass)>,
}

impl SyntheticFixtureOutputSafetyNet {
    fn from_docs(docs: &[gaze_token_bridge::bridge::SyntheticDoc]) -> Self {
        let mut entries = Vec::with_capacity(docs.len() * 4);
        for doc in docs {
            entries.push((doc.name.to_string(), PiiClass::Name));
            entries.push((doc.email.to_string(), PiiClass::Email));
            entries.push((
                doc.customer_id.to_string(),
                PiiClass::custom("customer_id").expect("valid custom class"),
            ));
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

fn print_header(step: &str) {
    println!("\n=== {step} ===");
}

fn print_results(response: &BridgeSearchResponse) {
    for hit in &response.results {
        println!("  [{}] {}", hit.doc_id, hit.snippet);
    }
}

fn print_outcome(outcome: BridgeSearchOutcome) {
    match outcome {
        BridgeSearchOutcome::Allowed(response) => {
            println!("ALLOWED. target_domain = {}", response.target_domain);
            print_results(&response);
        }
        BridgeSearchOutcome::Denied(_) => {
            println!("DENIED (authorization failed)");
        }
    }
}

fn main() {
    println!("gaze-token-bridge - local synthetic demo");
    println!("Owner-side authorization and translation over bundled fixtures.");

    print_header("Step 1 - ingest synthetic corpus");
    let docs = synthetic_docs();
    let output_safety_pipeline = Pipeline::builder()
        .register_safety_net(SyntheticFixtureOutputSafetyNet::from_docs(&docs))
        .build()
        .expect("demo output safety pipeline builds");
    let mut bridge = TokenBridge::demo()
        .expect("demo bridge builds")
        .with_output_safety_net(output_safety_pipeline, vec![LocaleTag::Global]);
    println!(
        "Ingested {} synthetic docs into two policy-scoped domains:",
        docs.len()
    );
    println!("  - {CUSTOMER_DOMAIN}");
    println!("  - {LEGAL_DOMAIN}");

    print_header("Step 2 - mint a lookup token");
    let support = support_principal();
    let session = RedactionSession::ephemeral_for(&support.id).expect("ephemeral session builds");
    println!("owner-side synthetic input: name = \"{DEMO_NAME}\"");
    let name_token = session
        .tokenize(&PiiClass::Name, DEMO_NAME)
        .expect("tokenize name");
    println!("session token passed to the bridge: {name_token}");

    print_header("Step 3 - support searches customer docs");
    let support_customer = bridge_request(
        support_principal(),
        "support_search",
        "support_lookup",
        CUSTOMER_DOMAIN,
        &name_token,
    );
    print_outcome(bridge.search(&session, &support_customer));

    print_header("Step 4 - support searches legal docs");
    let support_legal = bridge_request(
        support_principal(),
        "legal_search",
        "legal_lookup",
        LEGAL_DOMAIN,
        &name_token,
    );
    print_outcome(bridge.search(&session, &support_legal));

    print_header("Step 5 - admin searches legal docs");
    let admin = admin_principal();
    let admin_session = RedactionSession::ephemeral_for(&admin.id).expect("admin session builds");
    let admin_token = admin_session
        .tokenize(&PiiClass::Name, DEMO_NAME)
        .expect("tokenize name for admin");
    let admin_request = bridge_request(
        admin,
        "legal_search",
        "legal_lookup",
        LEGAL_DOMAIN,
        &admin_token,
    );
    print_outcome(bridge.search(&admin_session, &admin_request));

    println!("\naudit events recorded: {}", bridge.audit().len());
}
