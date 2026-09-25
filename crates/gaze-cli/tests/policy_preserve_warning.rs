use std::fs;
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
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
    run_clean_with_policy(&format!("{DETECTORS}\n{rules}"), input)
}

fn run_clean_with_policy(policy: &str, input: &str) -> std::process::Output {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(&path, policy).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args(["clean", "--policy", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn raw_family_fallback_is_named_after_all_recognizer_classes_are_ruled() {
    const INPUT: &str = "Überweisung DE89 3704 0044 0532 0130 00";
    const BASE: &str = "[session]\nscope = \"persistent\"\nttl_secs = 86400\n\n[policy.rulepacks]\nbundled = [\"core\", \"locale-de\"]\n\n[locale]\nactive = [\"de-DE\"]\n";
    const DEFAULT: &str = "[[rule]]\nkind = \"default\"\naction = \"preserve\"\n";
    const FAMILY: &str = "custom:family:payment-card-or-iban";

    let initial = run_clean_with_policy(&format!("{BASE}\n{DEFAULT}"), INPUT);
    assert!(
        initial.status.success(),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let initial_stderr = String::from_utf8(initial.stderr).unwrap();
    let warning = initial_stderr
        .lines()
        .find(|line| line.contains("detected classes without a reachable class rule:"))
        .expect("preserve diagnostic");
    let listed = warning
        .split_once("class rule: ")
        .unwrap()
        .1
        .split_once("; values can reach")
        .unwrap()
        .0;
    assert!(listed.split(", ").any(|class| class == FAMILY), "{warning}");

    let class_rules = listed
        .split(", ")
        .filter(|class| *class != FAMILY)
        .map(|class| {
            format!("[[rule]]\nkind = \"class\"\nclass = \"{class}\"\naction = \"preserve\"\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let preserving = run_clean_with_policy(&format!("{BASE}\n{class_rules}\n{DEFAULT}"), INPUT);
    assert!(
        preserving.status.success(),
        "{}",
        String::from_utf8_lossy(&preserving.stderr)
    );
    let clean: serde_json::Value = serde_json::from_slice(&preserving.stdout).unwrap();
    assert_eq!(clean["clean_text"], INPUT);
    let stderr = String::from_utf8(preserving.stderr).unwrap();
    assert!(
        stderr.contains(&format!("class rule: {FAMILY}; values can reach")),
        "{stderr}"
    );

    let tokenizing_rules = class_rules.replace("action = \"preserve\"", "action = \"tokenize\"");
    let protected = run_clean_with_policy(&format!("{BASE}\n{tokenizing_rules}\n{DEFAULT}"), INPUT);
    assert!(
        protected.status.success(),
        "{}",
        String::from_utf8_lossy(&protected.stderr)
    );
    let clean: serde_json::Value = serde_json::from_slice(&protected.stdout).unwrap();
    assert_ne!(clean["clean_text"], INPUT);
    let protected_stderr = String::from_utf8(protected.stderr).unwrap();
    assert!(
        !protected_stderr.contains("class rule: custom:family:"),
        "{protected_stderr}"
    );
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
fn class_rule_after_preserve_default_is_unreachable_and_warns() {
    let output = run_clean(
        "[[rule]]\nkind = \"default\"\naction = \"preserve\"\n\n[[rule]]\nkind = \"class\"\nclass = \"custom:beta\"\naction = \"tokenize\"\n",
        "BETA-2",
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("custom:beta"));
}

#[test]
fn protective_default_and_complete_class_rules_do_not_warn() {
    let tokenize = run_clean(
        "[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n",
        "BETA-2",
    );
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
fn generalize_class_warns_that_restore_is_unavailable() {
    let output = run_clean(
        "[[rule]]\nkind = \"class\"\nclass = \"custom:alpha\"\naction = \"generalize\"\n\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n",
        "ALPHA-1",
    );
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("one-way (no restore token)"), "{stderr}");
    assert!(stderr.contains("custom:alpha"), "{stderr}");
}

#[test]
fn ner_model_classes_are_in_the_warning() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!(
            "{DETECTORS}\n[ner]\nmodel_dir = \"__gaze_test_index_ner\"\n\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n"
        ),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args(["clean", "--policy", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"hello").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("name"), "{stderr}");
    assert!(stderr.contains("organization"), "{stderr}");
}

#[test]
fn failed_clean_keeps_one_json_error_on_stderr() {
    // Invalid safety-net configuration fails after policy assembly.
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!("{DETECTORS}\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n"),
    )
    .unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args([
            "clean",
            "--policy",
            path.to_str().unwrap(),
            "--safety-net-registry",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let stderr = String::from_utf8(failed.stderr).unwrap();
    let _: serde_json::Value = serde_json::from_str(&stderr).expect("one JSON error value");
}

#[test]
fn daemon_warns_once_for_multiple_requests() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!("{DETECTORS}\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n"),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args(["daemon", "--policy", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{\"session_id\":\"one\",\"text\":\"ALPHA-1\"}\n{\"session_id\":\"two\",\"text\":\"BETA-2\"}\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.matches("detected class").count(), 1, "{stderr}");
}

#[test]
fn proxy_warns_only_after_a_successful_bind() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!("{DETECTORS}\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n"),
    )
    .unwrap();
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = held.local_addr().unwrap().to_string();
    let failed = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args([
            "proxy",
            "serve",
            "--policy",
            path.to_str().unwrap(),
            "--bind",
            &bind,
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let _: serde_json::Value =
        serde_json::from_slice(&failed.stderr).expect("one JSON error value");
    drop(held);

    let mut child = Command::new(env!("CARGO_BIN_EXE_gaze"))
        .args([
            "proxy",
            "serve",
            "--policy",
            path.to_str().unwrap(),
            "--bind",
            &bind,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr_reader = std::io::BufReader::new(child.stderr.take().unwrap());
    let mut ready_output = String::new();
    loop {
        let mut line = String::new();
        assert_ne!(
            stderr_reader.read_line(&mut line).unwrap(),
            0,
            "proxy exited before readiness: {ready_output}"
        );
        ready_output.push_str(&line);
        if line.contains("detected classes") {
            break;
        }
    }
    child.kill().unwrap();
    let _ = child.wait().unwrap();
    stderr_reader.read_to_string(&mut ready_output).unwrap();
    let stderr = ready_output;
    assert_eq!(stderr.matches("detected classes").count(), 1, "{stderr}");
    assert!(stderr.contains("custom:alpha"), "{stderr}");
    assert!(stderr.contains("custom:beta"), "{stderr}");
}
