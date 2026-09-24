#![cfg(all(unix, feature = "safety-net-openai"))]
use gaze_recognizers::safety_net::openai_filter::{
    OpenAiFilterBackend, SubprocessOpenAiFilterBackend, SubprocessOpenAiFilterConfig,
};
use gaze_types::SafetyNetError;
use serial_test::file_serial;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::Duration,
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
    let script = r#"#!/usr/bin/env python3
import os, sys, time
sys.stdin.buffer.read()
if os.fork() == 0:
    os.close(0)
    os.close(1)
    open(sys.argv[0] + ".ready", "w").close()
    os.write(2, b'w' * (2 * 1024 * 1024))
    end = time.monotonic() + 30
    while not os.path.exists(sys.argv[0] + ".release") and time.monotonic() < end:
        time.sleep(0.01)
    os._exit(0)
os.write(1, b'[]')
os._exit(0)
"#;
    'attempt: loop {
        let (dir, backend) = backend(script, Duration::from_secs(1));
        let ready = dir.path().join("synthetic-backend.ready");
        let release = dir.path().join("synthetic-backend.release");
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = sender.send(backend.infer("clean"));
        });

        while !ready.exists() {
            match receiver.try_recv() {
                Ok(result) => {
                    worker.join().unwrap();
                    assert!(
                        matches!(&result, Err(SafetyNetError::Runtime { message }) if message.contains("timed out")),
                        "fixture failed before startup: {result:?}"
                    );
                    continue 'attempt;
                }
                Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
                Err(TryRecvError::Disconnected) => {
                    panic!("inference worker exited without a result")
                }
            }
        }
        // The 15s bound starts after readiness and measures whether cancellation joins a live stderr holder.
        let result = receiver.recv_timeout(Duration::from_secs(15));
        fs::write(release, b"").unwrap();
        worker.join().unwrap();
        let result = result.expect("deadline waited for descendant-held stderr");
        assert!(
            matches!(&result, Err(SafetyNetError::Runtime { message }) if message.contains("timed out")),
            "{result:?}"
        );
        break;
    }
}
