#![cfg(all(unix, feature = "safety-net-openai"))]
use gaze_recognizers::safety_net::openai_filter::{
    OpenAiFilterBackend, SubprocessOpenAiFilterBackend, SubprocessOpenAiFilterConfig,
};
use gaze_types::SafetyNetError;
use serial_test::file_serial;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

fn backend(script: &str, timeout: Duration) -> (tempfile::TempDir, SubprocessOpenAiFilterBackend) {
    let dir = tempfile::tempdir().unwrap();
    let command = dir.path().join("synthetic-backend");
    fs::write(&command, script).unwrap();
    fs::set_permissions(&command, fs::Permissions::from_mode(0o700)).unwrap();
    let backend = SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(command)
            .with_timeout(timeout)
            .with_stderr_diagnostics(true),
    )
    .unwrap();
    (dir, backend)
}

#[test]
#[file_serial(gaze_subprocess)]
fn truncated_unicode_email_discards_the_entire_unfinished_token() {
    let email = format!("aliceé{}@example.invalid", "x".repeat(60));
    let payload = format!("{}{email}", "safe ".repeat(40));
    assert!(payload.len() > 256);
    let script = format!("#!/bin/sh\ncat >/dev/null\nprintf '%s' '{payload}' >&2\nexit 7\n");
    let (_dir, backend) = backend(&script, Duration::from_secs(10));
    let error = backend.infer("clean").unwrap_err();
    let SafetyNetError::Runtime { message } = error else {
        panic!("runtime")
    };
    assert!(
        !message.contains("alice"),
        "unfinished sensitive token prefix was disclosed: {message}"
    );
}
