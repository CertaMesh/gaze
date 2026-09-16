//! The bundled `secrets` rulepack is opt-in (todo #3646). Credentials are not PII,
//! so `security_token.anchored` and `password.field` must stay inert under every
//! default activation and fire only when `secrets` is loaded by name.
//!
//! Mutation this file must catch: re-adding `password.field` (or
//! `security_token.anchored`) to `embedded/core.toml` turns the default-activation
//! tests RED, because the credential would be tokenized without an opt-in.

use std::fs;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

const AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const PASSWORD: &str = "synthetic-credential-value";

fn input() -> String {
    format!("rotate {AWS_KEY} today\npassword: {PASSWORD}\n")
}

fn clean_text(args: &[&str]) -> String {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .write_stdin(input().into_bytes())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clean failed: status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    json["clean_text"].as_str().expect("clean_text").to_string()
}

/// `bundled = None` omits `[policy.rulepacks]`, which selects the default `core` bundle.
/// The default rule tokenizes every class, so a credential recognizer that leaked into
/// the default activation would be visible here. (A run without `--policy` is not a
/// usable probe: the no-policy CLI preserves every custom class.)
fn policy_with_bundled(bundled: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    let rulepacks = bundled
        .map(|bundled| format!("[policy.rulepacks]\nbundled = [{bundled}]\n"))
        .unwrap_or_default();
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

{rulepacks}
[[rule]]
kind = "default"
action = "tokenize"
"#
        ),
    )
    .unwrap();
    (dir, path)
}

fn assert_credentials_untouched(clean: &str) {
    assert_eq!(
        clean,
        input(),
        "no credential may be tokenized without the secrets opt-in"
    );
    assert!(!clean.contains(":security_token_") && !clean.contains(":password_"));
}

fn assert_both_credentials_tokenized(clean: &str) {
    assert!(!clean.contains(AWS_KEY), "AWS key survived: {clean:?}");
    assert!(!clean.contains(PASSWORD), "password survived: {clean:?}");
    assert!(clean.contains(":security_token_"), "{clean:?}");
    assert!(clean.contains(":password_"), "{clean:?}");
    assert!(clean.starts_with("rotate <") && clean.contains("\npassword: <"));
}

#[test]
fn default_bundle_selection_emits_no_credential_tokens() {
    let (_dir, policy) = policy_with_bundled(None);
    let clean = clean_text(&["--policy", policy.to_str().unwrap()]);
    assert_credentials_untouched(&clean);
}

#[test]
fn explicit_core_bundle_emits_no_credential_tokens() {
    assert_credentials_untouched(&clean_text(&["--rulepack-bundled", "core"]));
    let (_dir, policy) = policy_with_bundled(Some(r#""core""#));
    assert_credentials_untouched(&clean_text(&["--policy", policy.to_str().unwrap()]));
}

#[test]
fn secrets_bundle_opt_in_tokenizes_both_credentials() {
    assert_both_credentials_tokenized(&clean_text(&["--rulepack-bundled", "core,secrets"]));
    let (_dir, policy) = policy_with_bundled(Some(r#""core", "secrets""#));
    assert_both_credentials_tokenized(&clean_text(&["--policy", policy.to_str().unwrap()]));
}

#[test]
fn username_field_record_is_no_longer_detected_by_any_bundle() {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--rulepack-bundled", "core,secrets"])
        .write_stdin("username: synthetic.login\n")
        .output()
        .unwrap();
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert_eq!(json["clean_text"], "username: synthetic.login\n");
    assert_eq!(json["stats"]["detections"], 0);
}
