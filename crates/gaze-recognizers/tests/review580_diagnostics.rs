#![cfg(all(unix, feature = "safety-net-openai"))]
use gaze_recognizers::safety_net::openai_filter::{
    OpenAiFilterBackend, SubprocessOpenAiFilterBackend, SubprocessOpenAiFilterConfig,
};
use gaze_types::SafetyNetError;
use serial_test::file_serial;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

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

#[test]
#[file_serial(gaze_subprocess)]
fn deadline_does_not_wait_for_a_descendant_holding_only_stderr() {
    // The descendant ends on its own, so even a broken join cannot hang this proof indefinitely.
    let script = r#"#!/usr/bin/env python3
import os, sys, time
sys.stdin.buffer.read()
if os.fork() == 0:
    open(sys.argv[0] + ".descendant", "w").close()
    os.close(0)
    os.close(1)
    os.write(2, b'w' * (2 * 1024 * 1024))
    time.sleep(4)
    os._exit(0)
os.write(1, b'[]')
os._exit(0)
"#;
    let (_dir, backend) = backend(script, Duration::from_secs(1));
    let started = Instant::now();
    let result = backend.infer("clean");
    assert!(result.is_err());
    assert!(
        _dir.path().join("synthetic-backend.descendant").exists(),
        "fixture must start its descendant before timeout"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "1s timeout waited {:?} for descendant-held stderr",
        started.elapsed()
    );
}
