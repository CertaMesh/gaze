use std::collections::BTreeMap;

use gaze::{RawDocument, Value};

fn main() {
    let raw = RawDocument::Structured(BTreeMap::from([(
        "email".to_string(),
        Value::String("alice@example.com".to_string()),
    )]));

    let _ = serde_json::to_string(&raw).unwrap();
}
