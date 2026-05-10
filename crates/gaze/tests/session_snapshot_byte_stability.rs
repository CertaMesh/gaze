//! Text-only snapshot stability guard.
//!
//! `Session::export().into_bytes()` includes a fresh signing key, Ed25519 signature, session id,
//! and `issued_at`, so the public API cannot produce byte-identical snapshots across runs without
//! a test-only deterministic session constructor. This test therefore pins a checked-in v3
//! `manifest.bin` fixture and verifies the stable text-only contract that matters to v0.6/v0.7
//! adopters: v3 envelope byte, no `document` extension, importable token map, and unchanged
//! canonical payload field shape.
//!
//! Regenerate only when the v3 text-only payload contract intentionally changes:
//! `GAZE_REGENERATE_SESSION_SNAPSHOT_FIXTURE=1 cargo test -p gaze-pii --test session_snapshot_byte_stability`.

use gaze::{PiiClass, Scope, SensitiveSnapshot, Session};
use serde_json::Value;

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/session_snapshot_v3_text_only.bin"
);

#[test]
fn checked_in_v3_text_only_snapshot_fixture_stays_importable_and_payload_shape_stable() {
    if std::env::var_os("GAZE_REGENERATE_SESSION_SNAPSHOT_FIXTURE").is_some() {
        regenerate_fixture();
        return;
    }

    let bytes = include_bytes!("fixtures/session_snapshot_v3_text_only.bin").to_vec();
    assert_eq!(bytes[0], 3);

    let payload: Value = serde_json::from_slice(&bytes[97..]).expect("fixture payload json");
    assert!(payload.get("document").is_none());
    assert_eq!(
        payload["scope"],
        serde_json::json!({"Conversation": "byte-stability"})
    );
    assert_eq!(payload["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(payload["entries"][0]["class"], "Name");
    assert_eq!(payload["entries"][0]["raw"], "Dr. Schmidt");
    assert_eq!(payload["entries"][0]["family"], "counter");
    assert_eq!(payload["next_by_class"], serde_json::json!([["Name", 1]]));

    let session = Session::import(SensitiveSnapshot::from(bytes)).expect("import fixture");
    let token = payload["entries"][0]["token"].as_str().expect("token");
    assert_eq!(session.restore(token).as_deref(), Some("Dr. Schmidt"));
}

#[test]
fn current_text_only_export_stays_v3_and_omits_document_extension() {
    let session = Session::new(Scope::Conversation("byte-stability".to_string())).expect("session");
    let _ = session
        .tokenize(&PiiClass::Name, "Dr. Schmidt")
        .expect("token");

    let bytes = session.export().expect("text-only export").into_bytes();
    assert_eq!(bytes[0], 3);

    let payload: Value = serde_json::from_slice(&bytes[97..]).expect("snapshot payload json");
    assert!(payload.get("document").is_none());
    assert_eq!(
        payload["scope"],
        serde_json::json!({"Conversation": "byte-stability"})
    );
    assert_eq!(payload["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(payload["entries"][0]["class"], "Name");
    assert_eq!(payload["entries"][0]["raw"], "Dr. Schmidt");
    assert_eq!(payload["entries"][0]["family"], "counter");
    assert_eq!(payload["next_by_class"], serde_json::json!([["Name", 1]]));
}

fn regenerate_fixture() {
    let session = Session::new(Scope::Conversation("byte-stability".to_string())).expect("session");
    let _ = session
        .tokenize(&PiiClass::Name, "Dr. Schmidt")
        .expect("token");
    let bytes = session.export().expect("text-only export").into_bytes();
    std::fs::write(FIXTURE_PATH, bytes).expect("write fixture");
}
