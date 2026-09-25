#![cfg(not(feature = "safety-net-nym"))]

use std::fs;

use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn policy_nym_refuses_when_binary_lacks_feature() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "conversation"
[policy.rulepacks]
bundled = ["core"]
[[rule]]
kind = "default"
action = "tokenize"
[safety_net]
backend = "nym"
"#,
    )
    .unwrap();
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--policy", path.to_str().unwrap()])
        .write_stdin("synthetic text")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"], "SafetyNetConfig");
    assert!(error["detail"]
        .as_str()
        .unwrap()
        .contains("safety-net-nym feature"));
}
