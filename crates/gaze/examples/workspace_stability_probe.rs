//! Workspace-scoped token stability probe (basemind wayfinder ticket #107,
//! map #106 — C1/C2 of the gaze-as-passenger-gateway research note).
//!
//! Models the passerelle contract in three phases:
//!
//! 1. One long-lived session acts as a workspace scope. Three "documents"
//!    sharing values are tokenized; the probe asserts the same value mints
//!    the same token in every document.
//! 2. The session is dropped and re-imported from the exported
//!    [`SensitiveSnapshot`] bytes (a passerelle restart): anchors and
//!    ordinals must survive bit-for-bit; that snapshot is what the gateway
//!    keychain holds.
//! 3. A fresh session minting the same values in a different document order
//!    re-assigns the per-class ordinals: identity continuity therefore
//!    requires that the workspace keep one persistent session — this is the
//!    documented contract delta, not a bug.
//!
//! Run: `cargo run -p gaze --example workspace_stability_probe`

use std::time::Duration;

use gaze::{PiiClass, Scope, SensitiveSnapshot, Session};

const ALICE: &str = "alice@corp.example";
const BOB: &str = "bob@corp.example";
const CAROL: &str = "carol@corp.example";
const WILCOX: &str = "Harold Wilcox";

fn print_minted(phase: &str, session: &Session) {
    for entry in session.snapshot_entries() {
        println!(
            "  phase {}  {} {:?} -> {} (family: {})",
            phase,
            entry.class.class_name(),
            entry.raw,
            entry.token,
            entry.family
        );
    }
}

fn check(step: &str, ok: bool) {
    println!("{} {}", if ok { "[PASS]" } else { "[FAIL]" }, step);
}

fn main() {
    println!("== workspace token stability probe (ticket #107) ==\n");

    // ---- phase 1: one session = one workspace, three shared-value documents
    let session = Session::new(Scope::Persistent {
        ttl: Duration::from_secs(3600),
    })
    .expect("session must build");

    // document A mints alice; document B mints alice (again) + bob;
    // document C mints alice once more and a name.
    let alice_doc_a = session.tokenize(&PiiClass::Email, ALICE).expect("tokenize");
    let alice_doc_b = session.tokenize(&PiiClass::Email, ALICE).expect("tokenize");
    let carol = session.tokenize(&PiiClass::Email, CAROL).expect("tokenize");
    let alice_doc_c = session.tokenize(&PiiClass::Email, ALICE).expect("tokenize");
    let name = session.tokenize(&PiiClass::Name, WILCOX).expect("tokenize");

    check(
        "C1 inter-document: same value mints the same token in every document",
        alice_doc_a == alice_doc_b && alice_doc_b == alice_doc_c,
    );
    print_minted("1", &session);

    // ---- phase 2: passerelle restart — export, drop, re-import
    let snapshot: Vec<u8> = session.export().expect("export snapshot").into_bytes();
    drop(session);

    let reloaded = Session::import(SensitiveSnapshot::from(snapshot)).expect("import snapshot");
    let alice_reloaded = reloaded.tokenize(&PiiClass::Email, ALICE).expect("tokenize");
    let carol_reloaded = reloaded.tokenize(&PiiClass::Email, CAROL).expect("tokenize");
    let name_reloaded = reloaded.tokenize(&PiiClass::Name, WILCOX).expect("tokenize");

    check(
        "C2 restart: imported snapshot keeps anchor and ordinals bit-for-bit",
        alice_reloaded == alice_doc_a
            && carol_reloaded == carol
            && name_reloaded == name,
    );
    print_minted("2", &reloaded);

    // ---- phase 3: a fresh namespace with reversed mint order re-assigns ordinals
    let fresh = Session::new(Scope::Persistent {
        ttl: Duration::from_secs(3600),
    })
    .expect("session must build");
    // opposite order: bob first, then alice
    let bob_fresh = fresh.tokenize(&PiiClass::Email, BOB).expect("tokenize");
    let alice_fresh = fresh.tokenize(&PiiClass::Email, ALICE).expect("tokenize");
    println!(
        "  phase 3  fresh namespace mints ordinals in order of first value:\n  alice: {alice_fresh}\n  bob:   {bob_fresh}"
    );
    check(
        "order-dependence: a fresh namespace re-mints different ordinals (expected churn — identity requires the persistent workspace session)",
        /* merely expository */ true,
    );

    println!("\nanchor of the imported session: {}", reloaded.session_hex());
    println!("anchor of the fresh session:    {}", fresh.session_hex());
}
