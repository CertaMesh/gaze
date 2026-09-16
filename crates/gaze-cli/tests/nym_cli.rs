//! `gaze clean --safety-net nym`: configuration fails closed, and (live) the net protects a plate.
#![cfg(feature = "safety-net-nym")]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::{tempdir, TempDir};

const PLATE_PROSE: &str = "Das Fahrzeug mit dem Kennzeichen M-AB 1234 wurde abgeschleppt.";

fn policy(extra: &str) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!(
            r#"[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "default"
action = "tokenize"
{extra}"#
        ),
    )
    .unwrap();
    (dir, path)
}

fn clean(args: &[&str], input: &str) -> std::process::Output {
    Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .env_remove("GAZE_NYM_MODEL_DIR")
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap()
}

fn stderr_json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stderr)
        .unwrap_or_else(|_| panic!("stderr={}", String::from_utf8_lossy(&out.stderr)))
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn nym_without_a_model_dir_is_a_config_error() {
    let (_dir, policy) = policy("");
    let out = clean(
        &["--policy", path_str(&policy), "--safety-net", "nym"],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    let stderr = stderr_json(&out);
    assert_eq!(stderr["error"], "SafetyNetConfig");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("M-AB"));
}

#[test]
fn nym_with_an_incomplete_bundle_fails_before_loading() {
    let (_dir, policy) = policy("");
    let bundle = tempdir().unwrap();
    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net",
            "nym",
            "--nym-model-dir",
            path_str(bundle.path()),
        ],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(stderr_json(&out)["error"], "SafetyNetArtifactMissing");
}

#[test]
fn nym_policy_table_without_the_nym_net_fails_closed() {
    let (_dir, policy) = policy(
        "\n[safety_net.nym]\nlabels = [\"LICENSE_PLATE\"]\nthreshold = { LICENSE_PLATE = 0.5 }\n",
    );
    let out = clean(&["--policy", path_str(&policy)], PLATE_PROSE);
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[safety_net.nym] requires --safety-net nym"),
        "{stderr}"
    );
}

#[test]
fn nym_policy_with_an_unmapped_label_fails_at_load() {
    let (_dir, policy) =
        policy("\n[safety_net.nym]\nlabels = [\"GIVEN_NAME\"]\nthreshold = { GIVEN_NAME = 0.5 }\n");
    let out = clean(
        &["--policy", path_str(&policy), "--safety-net", "nym"],
        PLATE_PROSE,
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("GIVEN_NAME"));
}

#[test]
fn nym_is_refused_through_the_registry() {
    let (_dir, policy) = policy("");
    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net-registry",
            "--safety-net-add",
            "nym",
        ],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--safety-net-registry"));
}

#[test]
fn nym_flags_without_a_net_are_refused() {
    let (_dir, policy) = policy("");
    let out = clean(
        &["--policy", path_str(&policy), "--nym-intra-threads", "2"],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
}

#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR pointing at the pinned bundle"]
fn live_nym_net_tokenizes_a_plate_the_rules_miss() {
    let model_dir = std::env::var("GAZE_NYM_MODEL_DIR").expect("GAZE_NYM_MODEL_DIR");
    let (_dir, policy) = policy("");

    let baseline = clean(&["--policy", path_str(&policy)], PLATE_PROSE);
    assert_eq!(baseline.status.code(), Some(0));
    let baseline: Value = serde_json::from_slice(&baseline.stdout).unwrap();
    assert!(
        baseline["clean_text"]
            .as_str()
            .unwrap()
            .contains("M-AB 1234"),
        "the rule floor alone must miss the plate for this test to mean anything"
    );

    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net",
            "nym",
            "--nym-model-dir",
            &model_dir,
        ],
        PLATE_PROSE,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let clean_text = json["clean_text"].as_str().unwrap();
    assert!(!clean_text.contains("M-AB"), "{clean_text}");
    assert!(clean_text.contains("Kennzeichen <"), "{clean_text}");
    let report = json["leak_report"].to_string();
    assert!(report.contains("nym-small-int8"), "{report}");
    assert!(report.contains("LICENSE_PLATE>=0.5"), "{report}");
}
