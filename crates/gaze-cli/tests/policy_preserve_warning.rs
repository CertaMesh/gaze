use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

const DETECTORS: &str = r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = []

[[policy.custom_recognizers]]
kind = "regex"
name = "alpha"
pattern = 'ALPHA-[0-9]+'
class = "custom:alpha"

[[policy.custom_recognizers]]
kind = "regex"
name = "beta"
pattern = 'BETA-[0-9]+'
class = "custom:beta"
"#;

fn run_clean(rules: &str, input: &str) -> std::process::Output {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(&path, format!("{DETECTORS}\n{rules}")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args(["clean", "--policy", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn explicit_preserve_default_warns_for_only_unruled_registered_class() {
    let output = run_clean(
        "[[rule]]\nkind = \"class\"\nclass = \"custom:alpha\"\naction = \"tokenize\"\n\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n",
        "ALPHA-1 BETA-2",
    );
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("1 detected class"), "{stderr}");
    assert!(stderr.contains("custom:beta"), "{stderr}");
    assert!(!stderr.contains("custom:alpha"), "{stderr}");
    assert!(stderr.contains("gaze setup --force"), "{stderr}");
}

#[test]
fn missing_default_warns_for_unruled_registered_class() {
    let output = run_clean(
        "[[rule]]\nkind = \"class\"\nclass = \"custom:alpha\"\naction = \"tokenize\"\n",
        "ALPHA-1 BETA-2",
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("custom:beta"));
}

#[test]
fn protective_default_and_complete_class_rules_do_not_warn() {
    let tokenize = run_clean("[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n", "BETA-2");
    assert!(tokenize.status.success());
    assert!(!String::from_utf8_lossy(&tokenize.stderr).contains("detected class"));

    let complete = run_clean(
        "[[rule]]\nkind = \"class\"\nclass = \"custom:alpha\"\naction = \"tokenize\"\n\n[[rule]]\nkind = \"class\"\nclass = \"custom:beta\"\naction = \"tokenize\"\n\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n",
        "BETA-2",
    );
    assert!(complete.status.success());
    assert!(!String::from_utf8_lossy(&complete.stderr).contains("detected class"));
}

#[test]
fn failed_clean_keeps_one_json_error_on_stderr() {
    let output = run_clean("[[rule]]\nkind = \"default\"\naction = \"preserve\"\n", "");
    // Invalid safety-net configuration fails after policy assembly.
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(&path, format!("{DETECTORS}\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n")).unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args(["clean", "--policy", path.to_str().unwrap(), "--safety-net-registry"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!failed.status.success());
    let stderr = String::from_utf8(failed.stderr).unwrap();
    let _: serde_json::Value = serde_json::from_str(&stderr).expect("one JSON error value");
}

#[test]
fn daemon_warns_once_for_multiple_requests() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(&path, format!("{DETECTORS}\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args(["daemon", "--policy", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{\"session_id\":\"one\",\"text\":\"ALPHA-1\"}\n{\"session_id\":\"two\",\"text\":\"BETA-2\"}\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.matches("detected class").count(), 1, "{stderr}");
}
