use gaze_recognizers::safety_net::openai_filter::{
    OPF_CHECKPOINT_BUNDLE_SHA256, OPF_SOURCE_COMMIT, OPF_SOURCE_REPO,
};
use serde_json::Value;

const SNAPSHOT: &str = include_str!("openai_privacy_filter_direct_vs_observer_snapshot.json");

fn main() {
    let snapshot: Value = serde_json::from_str(SNAPSHOT).expect("valid OPF benchmark snapshot");
    assert_eq!(
        snapshot["pins"]["opf_source_repo"],
        Value::String(OPF_SOURCE_REPO.to_string())
    );
    assert_eq!(
        snapshot["pins"]["opf_source_commit"],
        Value::String(OPF_SOURCE_COMMIT.to_string())
    );
    assert_eq!(
        snapshot["pins"]["opf_checkpoint_bundle_sha256"],
        OPF_CHECKPOINT_BUNDLE_SHA256
            .map(|sha256| Value::String(sha256.to_string()))
            .unwrap_or(Value::Null)
    );
    println!("{SNAPSHOT}");
}
