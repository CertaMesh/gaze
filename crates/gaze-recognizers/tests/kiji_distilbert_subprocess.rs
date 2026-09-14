//! Kiji DistilBERT subprocess adapter integration tests.
//!
//! Mirrors the `openai_filter_subprocess` suite at the boundary level: drive
//! the adapter against a fake `kiji` CLI written in shell, assert the
//! `SafetyNet::check` contract returns shape-valid `LeakSuspect`s and the
//! manifest-diff classification is wired through.

#![cfg(all(unix, feature = "safety-net-kiji"))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use gaze_recognizers::safety_net::kiji_distilbert::{
    KijiDistilbertBackend, KijiDistilbertSafetyNet, OrtKijiBackend, OrtKijiConfig,
    SubprocessKijiBackend, SubprocessKijiConfig, REQUIRED_KIJI_ARTIFACTS,
};
use gaze_types::{DocumentKind, LocaleTag, Manifest, SafetyNet, SafetyNetContext, SafetyNetError};
use serial_test::file_serial;
use sha2::{Digest, Sha256};
use tempfile::{tempdir, TempDir};

fn test_subprocess_timeout() -> Duration {
    let seconds = std::env::var("GAZE_TEST_SUBPROCESS_TIMEOUT_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("test subprocess timeout must be an integer")
        })
        .unwrap_or(60);
    assert!(seconds > 0, "test subprocess timeout must be positive");
    Duration::from_secs(seconds)
}

fn write_mock_kiji(body: &str) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mock-kiji");
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
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    (dir, path)
}

fn populate_model_dir() -> (TempDir, String) {
    let dir = tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for required in REQUIRED_KIJI_ARTIFACTS {
        if *required == "SHA256SUMS" {
            continue;
        }
        let path = dir.path().join(required);
        fs::write(&path, b"fixture").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let artifact_sha = hex_sha256(b"fixture");
    let sha256sums = format!(
        "{artifact_sha}  labels.json\n{artifact_sha}  model.onnx\n{artifact_sha}  tokenizer.json\n"
    );
    let path = dir.path().join("SHA256SUMS");
    fs::write(&path, sha256sums.as_bytes()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let expected_sha = hex_sha256(sha256sums.as_bytes());
    (dir, expected_sha)
}

fn hex_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn missing_sha256sums_is_weights_missing() {
    let dir = tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for required in REQUIRED_KIJI_ARTIFACTS {
        if *required == "SHA256SUMS" {
            continue;
        }
        let path = dir.path().join(required);
        fs::write(&path, b"fixture").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    let config = SubprocessKijiConfig::new("kiji").with_model_dir(dir.path());
    let net = KijiDistilbertSafetyNet::new(config);
    let manifest = Manifest::default();
    let ctx = SafetyNetContext::new(
        &manifest,
        &[LocaleTag::Global],
        DocumentKind::Text,
        None,
        None,
    );
    let error = net.check("hello", ctx).unwrap_err();
    match error {
        SafetyNetError::WeightsMissing { path } => {
            assert!(path.contains("SHA256SUMS"), "got {path}");
        }
        other => panic!("expected WeightsMissing, got {other:?}"),
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn subprocess_span_round_trips_through_manifest_diff() {
    // Mock-kiji emits one person span at [0,11] over "Alice Smith". The default
    // empty manifest classifies that as Uncovered.
    let (_kiji_dir, kiji) =
        write_mock_kiji(r#"[{"label":"person","start":0,"end":11,"score":0.97}]"#);
    let (model, expected_sha) = populate_model_dir();

    let config = SubprocessKijiConfig::new(&kiji)
        .with_model_dir(model.path())
        .with_timeout(test_subprocess_timeout())
        .with_expected_bundle_sha256_for_tests(expected_sha);
    let net = KijiDistilbertSafetyNet::new(config);
    let manifest = Manifest::default();
    let ctx = SafetyNetContext::new(
        &manifest,
        &[LocaleTag::Global],
        DocumentKind::Text,
        None,
        None,
    );

    let suspects = net.check("Alice Smith greets you", ctx).unwrap();
    assert_eq!(suspects.len(), 1);
    let suspect = &suspects[0];
    assert_eq!(suspect.safety_net_id, "kiji-distilbert-subprocess");
    assert_eq!(suspect.raw_label, "person");
    assert_eq!(suspect.span, 0..11);
    assert_eq!(suspect.score, Some(0.97));
}

#[test]
#[file_serial(gaze_subprocess)]
fn ort_matches_subprocess_for_fixture_inputs_when_real_kiji_is_configured() {
    let Some(command) = std::env::var_os("GAZE_KIJI_DISTILBERT_COMMAND") else {
        eprintln!("skipping parity test: GAZE_KIJI_DISTILBERT_COMMAND is not set");
        return;
    };
    let Some(model_dir) = std::env::var_os("GAZE_KIJI_DISTILBERT_MODEL_DIR") else {
        eprintln!("skipping parity test: GAZE_KIJI_DISTILBERT_MODEL_DIR is not set");
        return;
    };

    let subprocess = SubprocessKijiBackend::new(
        SubprocessKijiConfig::new(command)
            .with_model_dir(PathBuf::from(&model_dir))
            .with_timeout(test_subprocess_timeout()),
    )
    .unwrap();
    let ort = OrtKijiBackend::new(OrtKijiConfig::new(PathBuf::from(model_dir))).unwrap();
    for fixture in [
        "Alice Smith visited Berlin.",
        "Dr. Schmidt works at Example Corp.",
        "The package moved from Paris to Example Labs.",
    ] {
        let left = subprocess.infer(fixture).unwrap();
        let right = ort.infer(fixture).unwrap();
        assert_eq!(right, left, "fixture: {fixture}");
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn verbose_stderr_preserves_successful_spans() {
    for bytes in [257, 300, 2 * 1024 * 1024] {
        let body = format!(
            r#"#!/bin/sh
cat >/dev/null
head -c {bytes} /dev/zero | tr '\000' w >&2
printf '%s\n' '[{{"label":"person","start":0,"end":11,"score":0.97}}]'
"#
        );
        let (_command_dir, command) = write_mock_kiji("[]");
        fs::write(&command, &body).unwrap();
        let (model, expected_sha) = populate_model_dir();
        let config = SubprocessKijiConfig::new(command)
            .with_model_dir(model.path())
            .with_expected_bundle_sha256_for_tests(expected_sha);
        let quiet =
            SubprocessKijiBackend::new(config.clone().with_timeout(test_subprocess_timeout()))
                .unwrap()
                .infer("Dr. Schmidt greets you")
                .unwrap();
        let verbose = SubprocessKijiBackend::new(
            config
                .with_timeout(test_subprocess_timeout())
                .with_stderr_diagnostics(true),
        )
        .unwrap()
        .infer("Dr. Schmidt greets you")
        .unwrap();
        assert_eq!(verbose, quiet, "stderr bytes: {bytes}");
        assert_eq!(verbose.len(), 1);
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn overflowing_stderr_failure_is_sanitized_and_marked() {
    let body = r#"#!/bin/sh
cat >/dev/null
printf 'failed alice@example.invalid +1-555-0101 \033\377é ' >&2
head -c 2097152 /dev/zero | tr '\000' w >&2
exit 7
"#
    .to_string();
    let (_command_dir, command) = write_mock_kiji("[]");
    fs::write(&command, &body).unwrap();
    let (model, expected_sha) = populate_model_dir();
    let config = SubprocessKijiConfig::new(command)
        .with_model_dir(model.path())
        .with_expected_bundle_sha256_for_tests(expected_sha);
    let error = SubprocessKijiBackend::new(
        config
            .with_timeout(test_subprocess_timeout())
            .with_stderr_diagnostics(true),
    )
    .unwrap()
    .infer("clean")
    .unwrap_err();
    let SafetyNetError::Runtime { message } = error else {
        panic!("expected runtime error");
    };
    assert!(message.contains("exited with status"));
    let diagnostic = message.split_once(": ").unwrap().1;
    assert!(diagnostic.len() <= 256);
    assert!(diagnostic.ends_with("[truncated]"));
    assert!(diagnostic.contains("<redacted>"));
    assert!(!diagnostic.contains("alice"));
    assert!(!diagnostic.contains("555"));
    assert!(diagnostic.is_ascii());
    assert!(!diagnostic.chars().any(char::is_control));
}

#[test]
#[file_serial(gaze_subprocess)]
fn stdout_overflow_still_fails_closed_with_verbose_stderr() {
    let body = r#"#!/bin/sh
cat >/dev/null
head -c 2097152 /dev/zero | tr '\000' w >&2
printf '%s\n' '[{"label":"person","start":0,"end":11,"score":0.97}]'
"#
    .to_string();
    let (_command_dir, command) = write_mock_kiji("[]");
    fs::write(&command, &body).unwrap();
    let (model, expected_sha) = populate_model_dir();
    let config = SubprocessKijiConfig::new(command)
        .with_model_dir(model.path())
        .with_expected_bundle_sha256_for_tests(expected_sha);
    let error = SubprocessKijiBackend::new(
        config
            .with_timeout(test_subprocess_timeout())
            .with_stderr_diagnostics(true)
            .with_max_stdout_bytes(8),
    )
    .unwrap()
    .infer("Dr. Schmidt")
    .unwrap_err();
    let SafetyNetError::Runtime { message } = error else {
        panic!("expected runtime error");
    };
    assert!(message.contains("stdout capture failed"));
    assert!(message.contains("stream exceeded configured byte cap"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn overflowing_pipes_still_kill_and_reap_child() {
    for stdout_overflow in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("child.pid");
        // Keep the shell itself writing: no descendant can retain a pipe after kill.
        let output = if stdout_overflow {
            "printf '%8192s' w; printf '%8192s' w >&2"
        } else {
            "printf '%8192s' w >&2"
        };
        let body = format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' "$$" > '{}'
while :; do {output}; done
"#,
            pidfile.display()
        );
        let (_command_dir, command) = write_mock_kiji("[]");
        fs::write(&command, &body).unwrap();
        let (model, expected_sha) = populate_model_dir();
        let config = SubprocessKijiConfig::new(command)
            .with_model_dir(model.path())
            .with_expected_bundle_sha256_for_tests(expected_sha);
        let started = std::time::Instant::now();
        let error = SubprocessKijiBackend::new(
            config
                .with_timeout(Duration::from_secs(5))
                .with_max_stdout_bytes(128 * 1024)
                .with_stderr_diagnostics(true),
        )
        .unwrap()
        .infer("clean")
        .unwrap_err();
        let SafetyNetError::Runtime { message } = error else {
            panic!("expected runtime error");
        };
        if stdout_overflow {
            assert!(message.contains("stdout capture failed"), "{message}");
        } else {
            assert!(message.contains("timed out"), "{message}");
        }
        assert!(started.elapsed() < Duration::from_secs(20));
        let pid = fs::read_to_string(pidfile).unwrap();
        assert!(
            !std::process::Command::new("ps")
                .args(["-p", pid.trim()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success(),
            "child must be killed and reaped"
        );
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn invalid_stdout_remains_invalid_output_with_verbose_stderr() {
    let body = r#"#!/bin/sh
cat >/dev/null
head -c 2097152 /dev/zero | tr '\000' w >&2
printf '%s\n' 'alice@example.invalid'
"#
    .to_string();
    let (_command_dir, command) = write_mock_kiji("[]");
    fs::write(&command, &body).unwrap();
    let (model, expected_sha) = populate_model_dir();
    let config = SubprocessKijiConfig::new(command)
        .with_model_dir(model.path())
        .with_expected_bundle_sha256_for_tests(expected_sha);
    let error = SubprocessKijiBackend::new(
        config
            .with_timeout(test_subprocess_timeout())
            .with_stderr_diagnostics(true),
    )
    .unwrap()
    .infer("clean")
    .unwrap_err();
    assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    assert!(!error.to_string().contains("alice@example.invalid"));
}
