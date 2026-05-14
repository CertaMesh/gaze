use gaze_recognizers::safety_net::kiji_distilbert::{
    KIJI_DISTILBERT_BUNDLE_SHA256, KIJI_DISTILBERT_HF_COMMIT, KIJI_DISTILBERT_HF_REPO,
};
use serde_json::Value;

const SNAPSHOT: &str = include_str!("kiji_direct_vs_observer_snapshot.json");

fn main() {
    let snapshot: Value = serde_json::from_str(SNAPSHOT).expect("valid Kiji benchmark snapshot");
    assert_eq!(
        snapshot["pins"]["kiji_source_repo"],
        Value::String(KIJI_DISTILBERT_HF_REPO.to_string())
    );
    assert_eq!(
        snapshot["pins"]["kiji_source_commit"],
        Value::String(KIJI_DISTILBERT_HF_COMMIT.to_string())
    );
    assert_eq!(
        snapshot["pins"]["kiji_bundle_sha256"],
        Value::String(KIJI_DISTILBERT_BUNDLE_SHA256.to_string())
    );
    println!("{SNAPSHOT}");
}
