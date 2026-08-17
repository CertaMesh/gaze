#![cfg(feature = "safety-net-openai")]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use gaze::{LeakKind, LeakSuspect, PiiClass};
use gaze_audit::{LeakSuspectLogEntry, LeakSuspectLogger, SqliteLogger};
use serde_json::Value;
use serial_test::file_serial;
use tempfile::{tempdir, TempDir};

fn test_subprocess_timeout_ms() -> String {
    let seconds = std::env::var("GAZE_TEST_SUBPROCESS_TIMEOUT_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("test subprocess timeout must be an integer")
        })
        .unwrap_or(60);
    assert!(seconds > 0, "test subprocess timeout must be positive");
    seconds.saturating_mul(1_000).to_string()
}

fn write_mock_opf(body: &str) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mock-opf");
    fs::write(
        &path,
        format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{}'
"#,
            body
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    (dir, path)
}

fn write_arg_logging_mock_opf(body: &str, arg_log: &Path) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mock-opf");
    fs::write(
        &path,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
cat >/dev/null
printf '%s\n' '{}'
"#,
            arg_log.display(),
            body
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    (dir, path)
}

fn checkpoint_dir() -> TempDir {
    let dir = tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    dir
}

fn clean(args: &[String], input: &str) -> std::process::Output {
    Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap()
}

fn clean_with_tolerant_env(args: &[String], input: &str) -> std::process::Output {
    Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .env("GAZE_ALLOW_TOLERANT", "1")
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap()
}

fn safety_args(command: &Path, checkpoint: &Path) -> Vec<String> {
    vec![
        "--safety-net".to_string(),
        "openai-filter".to_string(),
        "--openai-filter-command".to_string(),
        command.display().to_string(),
        "--openai-filter-checkpoint".to_string(),
        checkpoint.display().to_string(),
        "--safety-net-timeout-ms".to_string(),
        test_subprocess_timeout_ms(),
    ]
}

#[test]
#[file_serial(gaze_subprocess)]
fn explicit_openai_filter_device_reaches_opf_argv() {
    let arg_dir = tempdir().unwrap();
    let arg_log = arg_dir.path().join("opf.args");
    let (_opf_dir, opf) = write_arg_logging_mock_opf("[]", &arg_log);
    let checkpoint = checkpoint_dir();
    let mut args = safety_args(&opf, checkpoint.path());
    args.extend(["--openai-filter-device".to_string(), "cuda".to_string()]);

    let out = clean(&args, "Plain text");

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let argv = fs::read_to_string(arg_log).unwrap();
    assert!(argv.lines().any(|arg| arg == "--device"));
    assert!(argv.lines().any(|arg| arg == "cuda"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn auto_openai_filter_device_does_not_inject_opf_device_arg() {
    let arg_dir = tempdir().unwrap();
    let arg_log = arg_dir.path().join("opf.args");
    let (_opf_dir, opf) = write_arg_logging_mock_opf("[]", &arg_log);
    let checkpoint = checkpoint_dir();
    let mut args = safety_args(&opf, checkpoint.path());
    args.extend(["--openai-filter-device".to_string(), "auto".to_string()]);

    let out = clean(&args, "Plain text");

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let argv = fs::read_to_string(arg_log).unwrap();
    assert!(!argv.lines().any(|arg| arg == "--device"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn missing_checkpoint_fails_closed_with_sanitized_error() {
    let (_opf_dir, opf) = write_mock_opf("[]");
    let missing = tempdir().unwrap().path().join("missing-checkpoint");
    let out = clean(&safety_args(&opf, &missing), "Plain text");

    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    let stderr: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(stderr["error"], "SafetyNet");
    assert_eq!(stderr["variant"], "WeightsMissing");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("Plain text"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn uncovered_suspect_exits_three_in_strict_mode_without_stdout() {
    let (_opf_dir, opf) = write_mock_opf(r#"[{"label":"private_person","start":0,"end":5}]"#);
    let checkpoint = checkpoint_dir();
    let mut args = safety_args(&opf, checkpoint.path());
    args.extend(["--safety-net-mode".to_string(), "strict".to_string()]);
    let out = clean(&args, "Plain text");

    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    let stderr: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(stderr["variant"], "SuspectedLeak");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("Plain text"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn default_safety_net_mode_resolves_with_redact_fallback() {
    let (_opf_dir, opf) = write_mock_opf(r#"[{"label":"private_email","start":0,"end":5}]"#);
    let checkpoint = checkpoint_dir();
    let out = clean(&safety_args(&opf, checkpoint.path()), "Plain text");

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body: Value = serde_json::from_slice(&out.stdout).unwrap();
    let clean_text = body["clean_text"].as_str().expect("clean_text");
    assert!(!clean_text.starts_with("Plain"));
    assert!(clean_text.ends_with(" text"));
    assert_eq!(body["leak_report"]["stats"]["uncovered_count"], 1);
}

#[test]
#[file_serial(gaze_subprocess)]
fn tolerant_mode_requires_env_opt_in() {
    let (_opf_dir, opf) = write_mock_opf(r#"[{"label":"private_email","start":0,"end":5}]"#);
    let checkpoint = checkpoint_dir();
    let mut args = safety_args(&opf, checkpoint.path());
    args.extend(["--safety-net-mode".to_string(), "tolerant".to_string()]);
    let out = clean(&args, "Plain text");

    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    let stderr: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(stderr["variant"], "TolerantModeDisabled");
}

#[test]
#[file_serial(gaze_subprocess)]
fn tolerant_fallback_requires_env_opt_in() {
    let (_opf_dir, opf) = write_mock_opf(r#"[{"label":"private_email","start":0,"end":5}]"#);
    let checkpoint = checkpoint_dir();
    let mut args = safety_args(&opf, checkpoint.path());
    args.extend(["--safety-net-fallback".to_string(), "tolerant".to_string()]);
    let out = clean(&args, "Plain text");

    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    let stderr: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(stderr["variant"], "TolerantModeDisabled");
}

#[test]
#[file_serial(gaze_subprocess)]
fn class_mismatch_warns_and_reports_without_failing() {
    let (_opf_dir, opf) = write_mock_opf(r#"[{"label":"private_person","start":8,"end":17}]"#);
    let checkpoint = checkpoint_dir();
    let out = clean(
        &safety_args(&opf, checkpoint.path()),
        "Contact alice@example.invalid",
    );

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains(r#""variant":"ClassMismatch""#));
    let body: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(body["leak_report"]["stats"]["class_mismatch_count"], 1);
    assert_eq!(
        body["leak_report"]["suspects"][0]["leak_kind"],
        "class_mismatch"
    );
    assert_eq!(
        body["leak_report"]["suspects"][0]["raw_label"],
        "private_person"
    );
    assert_eq!(body["leak_report"]["suspects"][0]["mapped_class"], "Name");
    let leak_report = body["leak_report"].to_string();
    assert!(!leak_report.contains("alice@example.invalid"));
    assert!(!leak_report.contains("\"start\""));
    assert!(!leak_report.contains("\"end\""));
}

#[test]
#[file_serial(gaze_subprocess)]
fn tolerant_uncovered_outputs_report_and_logs_audit_row() {
    let (_opf_dir, opf) = write_mock_opf(r#"[{"label":"private_email","start":0,"end":5}]"#);
    let checkpoint = checkpoint_dir();
    let audit_dir = tempdir().unwrap();
    let audit = audit_dir.path().join("audit.sqlite");
    let mut args = safety_args(&opf, checkpoint.path());
    args.extend([
        "--safety-net-mode".to_string(),
        "tolerant".to_string(),
        "--audit-db".to_string(),
        audit.display().to_string(),
    ]);
    let out = clean_with_tolerant_env(&args, "Plain text");

    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("warning: tolerant mode downgrades"));
    let body: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(body["leak_report"]["stats"]["uncovered_count"], 1);

    let query = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "safety-net",
            "query",
            "--audit-db",
            audit.to_str().unwrap(),
            "--leak-kind",
            "uncovered",
        ])
        .output()
        .unwrap();
    assert!(query.status.success());
    let stdout = String::from_utf8(query.stdout).unwrap();
    assert!(stdout.contains("safety_net_id\traw_label\tmapped_class\tleak_kind"));
    assert!(stdout.contains("openai-privacy-filter-subprocess\tprivate_email\temail\tuncovered"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn safety_net_audit_query_filters_structured_field_path() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("audit.sqlite");
    let logger = SqliteLogger::new(&db).unwrap();
    let suspect = LeakSuspect::new(
        0..5,
        PiiClass::Email,
        "openai-privacy-filter-subprocess",
        Some(0.9),
        LeakKind::Uncovered,
        "private_email",
        Some("$.user.email".to_string()),
    );
    let entry = LeakSuspectLogEntry::from_suspect(
        &suspect,
        gaze::DocumentKind::Structured,
        1_700_000_000_000,
        Some("session-a".to_string()),
        None,
    );
    logger.log_leak_suspect(&entry).unwrap();

    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "safety-net",
            "query",
            "--audit-db",
            db.to_str().unwrap(),
            "--field-path",
            "$.user.email",
            "--mapped-class",
            "email",
            "--raw-label",
            "private_email",
            "--from",
            "2023-11-14T22:13:20Z",
            "--to",
            "2023-11-14T22:13:21Z",
        ])
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("$.user.email"));
    assert!(stdout.contains("\tstructured\t"));
    assert!(!stdout.contains("alice@example.invalid"));
}
