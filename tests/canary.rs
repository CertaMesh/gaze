//! The canary harness. Every fixture row contains the literal string
//! `CANARY_EMAIL_DO_NOT_LEAK@test.local`. Any CleanRow, audit-log write,
//! or error message that contains this literal is a leak. This test is
//! consumed in M5 once we have an end-to-end flow; here we assert the
//! primitive (an Anonymizer never emits the canary unchanged).

use gaze::anon::{Anonymizer, PiiDetector};
use gaze::policy::classifier::{Classifier, PiiClass};
use gaze::types::{RawRow, Value};
use std::collections::BTreeMap;

pub const CANARY: &str = "CANARY_EMAIL_DO_NOT_LEAK@test.local";

pub fn assert_no_canary_in<S: AsRef<str>>(haystack: S, context: &str) {
    let h = haystack.as_ref();
    assert!(!h.contains(CANARY), "canary leaked in {}: {}", context, h);
}

fn row_with_canary() -> RawRow {
    let mut cols = BTreeMap::new();
    cols.insert("email".into(), Value::Text(CANARY.into()));
    RawRow { columns: cols }
}

#[test]
fn anonymizer_never_emits_canary_on_known_column() {
    let a = Anonymizer::new(Classifier::new().with_column("email", PiiClass::Email));
    let cleaned = a.clean(row_with_canary());
    let json = serde_json::to_string(cleaned.columns()).unwrap();
    assert_no_canary_in(&json, "CleanRow");
}

#[test]
fn detector_finds_canary_in_generic_text() {
    // If a column isn't declared as Email in the policy, the column rule
    // won't rewrite the canary. The detector (layer 2) must still catch it.
    // This test asserts detection; replacement is wired in M5.
    let d = gaze::anon::WorkaDetector::new();
    let hits = d.detect(&format!("note: contact {} for access", CANARY));
    assert!(
        hits.iter().any(|h| matches!(h.class, PiiClass::Email)),
        "Worka detector must find canary email in freeform text"
    );
}
