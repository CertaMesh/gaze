//! Snapshot compatibility fixtures for every envelope version current releases write.
//!
//! UPGRADE.md promises that manifests from an older minor still restore on newer ones. Each
//! checked-in fixture is an export written by the v6, v7 or v8 writer, so a reader change, a
//! stored-class encoding change or lost per-entry metadata that breaks a stored manifest fails
//! here. The tokens are pinned literally on purpose: `Custom:postal_code` must keep restoring,
//! and keep its counter, even after the class becomes a first-class variant.
//!
//! The writer is `session::tests::write_snapshot_compat_fixtures` (ignored). Never regenerate a
//! fixture to make this test pass; add a new fixture for a new envelope instead.

use gaze::{Error, PiiClass, SensitiveSnapshot, Session, SourceSpelling};
use serde_json::Value;

const V6: &[u8] = include_bytes!("fixtures/session_snapshot_v6.bin");
const V7: &[u8] = include_bytes!("fixtures/session_snapshot_v7.bin");
const V8: &[u8] = include_bytes!("fixtures/session_snapshot_v8.bin");

fn import(bytes: &[u8]) -> Session {
    Session::import(SensitiveSnapshot::from(bytes.to_vec())).expect("fixture imports")
}

fn payload(bytes: &[u8]) -> Value {
    serde_json::from_slice(&bytes[97..]).expect("payload json")
}

/// Every fixture holds the same three base entries under its own session prefix.
fn assert_base_entries_restore(session: &Session, hex: &str) {
    let clean = format!(
        "Reviewer <{hex}:Name_1> (<{hex}:Email_1>) at postcode <{hex}:Custom:postal_code_1>."
    );
    assert_eq!(
        session.restore_strict_text(&clean).expect("strict restore"),
        "Reviewer Dr. Schmidt (alice@example.invalid) at postcode 12345."
    );

    // An imported class must keep its value and counter keys, or the next turn would mint a
    // duplicate token for a stored value or reuse a stored token for a new one.
    let postal = PiiClass::custom("postal_code").unwrap();
    assert_eq!(
        session.tokenize(&postal, "12345").unwrap(),
        format!("<{hex}:Custom:postal_code_1>")
    );
    assert_eq!(
        session.tokenize(&postal, "54321").unwrap(),
        format!("<{hex}:Custom:postal_code_2>")
    );
}

/// Re-exporting an imported fixture keeps its envelope version and per-entry metadata.
fn assert_reexport_keeps(session: &Session, version: u8) -> Value {
    let bytes = session.export().expect("re-export").into_bytes();
    assert_eq!(bytes[0], version);
    let payload = payload(&bytes);
    let name = payload["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["raw"] == "Dr. Schmidt")
        .expect("name entry");
    assert_eq!(name["evidence"], "pattern");
    payload
}

#[test]
fn v6_fixture_restores_byte_exact_and_keeps_evidence() {
    assert_eq!(V6[0], 6);
    let session = import(V6);
    assert_base_entries_restore(&session, "e9560063");
    assert_reexport_keeps(&session, 6);
}

#[test]
fn v7_fixture_restores_its_json_escaped_spelling() {
    assert_eq!(V7[0], 7);
    let session = import(V7);
    assert_base_entries_restore(&session, "828f2835");

    let escaped = "<828f2835:Name_2>";
    assert_eq!(
        session.source_spelling(escaped),
        Some(SourceSpelling::JsonEscaped)
    );
    assert_eq!(
        session.restore_strict_text(escaped).unwrap(),
        r#"Robert \"Bob\" Smith"#
    );
    assert_eq!(
        session
            .restore_strict_decoded_text_with_provenance(escaped)
            .unwrap()
            .text,
        "Robert \"Bob\" Smith"
    );
    assert_reexport_keeps(&session, 7);
}

#[test]
fn v8_fixture_keeps_its_private_identifier_namespace() {
    assert_eq!(V8[0], 8);
    let session = import(V8);
    assert_base_entries_restore(&session, "b7a96e09");

    let identifier = session
        .opaque_identifier("class", "synthetic_record_name")
        .unwrap();
    assert_eq!(identifier, PINNED_V8_IDENTIFIER);
    assert!(session.owns_opaque_identifier("class", PINNED_V8_IDENTIFIER));
    assert!(!session.owns_opaque_identifier("dictionary", PINNED_V8_IDENTIFIER));
    assert!(assert_reexport_keeps(&session, 8)["opaque_identifier_key"].is_array());
}

/// Derived from the fixture's stored key; a change means a host-context namespace would silently move.
const PINNED_V8_IDENTIFIER: &str =
    "66a31bcfb2f689cda187bc8facaa39dc3c59691f90af170d524d774a9855e79a";

#[test]
fn tampered_fixtures_fail_closed() {
    for fixture in [V6, V7, V8] {
        let mut bytes = fixture.to_vec();
        let last = bytes.len() - 2;
        bytes[last] ^= 0x01;
        assert!(matches!(
            Session::import(SensitiveSnapshot::from(bytes)),
            Err(Error::InvalidSnapshotSignature)
        ));
    }
}
