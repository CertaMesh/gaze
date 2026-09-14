#![cfg(all(unix, feature = "safety-net-openai", feature = "safety-net-kiji"))]

use gaze_recognizers::safety_net::{
    kiji_distilbert::{KijiDistilbertBackend, SubprocessKijiBackend, SubprocessKijiConfig},
    openai_filter::{
        OpenAiFilterBackend, SubprocessOpenAiFilterBackend, SubprocessOpenAiFilterConfig,
    },
};
use gaze_types::SafetyNetError;
use serial_test::file_serial;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

fn infer(command: &Path, kiji: bool, input: &str) -> Result<(), SafetyNetError> {
    if kiji {
        SubprocessKijiBackend::new(
            SubprocessKijiConfig::new(command)
                .with_timeout(Duration::from_secs(1))
                .with_max_input_bytes(input.len().max(1))
                .with_stderr_diagnostics(true),
        )
        .unwrap()
        .infer(input)
        .map(|_| ())
    } else {
        SubprocessOpenAiFilterBackend::new(
            SubprocessOpenAiFilterConfig::new(command)
                .with_timeout(Duration::from_secs(1))
                .with_max_input_bytes(input.len().max(1))
                .with_stderr_diagnostics(true),
        )
        .unwrap()
        .infer(input)
        .map(|_| ())
    }
}

fn script(dir: &Path, body: &str) -> std::path::PathBuf {
    let command = dir.join("synthetic-backend");
    fs::write(&command, body).unwrap();
    fs::set_permissions(&command, fs::Permissions::from_mode(0o700)).unwrap();
    command
}

#[test]
#[file_serial(gaze_subprocess)]
fn kiji_discards_unfinished_unicode_email() {
    let dir = tempfile::tempdir().unwrap();
    let payload = format!(
        "{}aliceé{}@example.invalid",
        "safe ".repeat(40),
        "x".repeat(60)
    );
    let command = script(
        dir.path(),
        &format!("#!/bin/sh\ncat >/dev/null\nprintf '%s' '{payload}' >&2\nexit 7\n"),
    );
    let SafetyNetError::Runtime { message } = infer(&command, true, "clean").unwrap_err() else {
        panic!("expected runtime error")
    };
    assert!(!message.contains("alice"));
    assert!(message.ends_with("[truncated]"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn both_backends_close_descendant_held_read_pipes_before_returning() {
    for kiji in [false, true] {
        for held in [1, 2] {
            let dir = tempfile::tempdir().unwrap();
            // The descendant observes EPIPE after infer returns. A detached reader
            // would keep the pipe open and make this fail even if return was fast.
            let command = script(
                dir.path(),
                &format!(
                    r#"#!/usr/bin/env python3
import os, sys, time
sys.stdin.buffer.read()
if os.fork() == 0:
    os.close(0)
    os.close({other})
    fd = {held}
    os.set_blocking(fd, False)
    open(sys.argv[0] + '.ready', 'w').close()
    end = time.monotonic() + 6
    while time.monotonic() < end:
        try:
            os.write(fd, b'w')
        except BrokenPipeError:
            open(sys.argv[0] + '.closed', 'w').close()
            os._exit(0)
        except BlockingIOError:
            pass
        time.sleep(0.01)
    os._exit(0)
os.write(1, b'[]')
os._exit(0)
"#,
                    other = 3 - held
                ),
            );
            let started = Instant::now();
            let error = infer(&command, kiji, "clean").unwrap_err();
            assert!(
                matches!(error, SafetyNetError::Runtime { ref message } if message.contains("timed out")),
                "{error:?}"
            );
            assert!(dir.path().join("synthetic-backend.ready").exists());
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "kiji={kiji}, fd={held}"
            );
            let closed = dir.path().join("synthetic-backend.closed");
            let deadline = Instant::now() + Duration::from_secs(2);
            while !closed.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                closed.exists(),
                "reader pipe remained open: kiji={kiji}, fd={held}"
            );
        }
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn both_backends_cancel_full_stdin_and_close_the_writer() {
    for kiji in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let command = script(
            dir.path(),
            r#"#!/usr/bin/env python3
import os, sys, time
if os.fork() == 0:
    os.close(1)
    os.close(2)
    open(sys.argv[0] + '.ready', 'w').close()
    time.sleep(2)
    count = 0
    os.set_blocking(0, False)
    end = time.monotonic() + 4
    while time.monotonic() < end:
        try:
            chunk = os.read(0, 8192)
            if not chunk:
                with open(sys.argv[0] + '.result', 'w') as f:
                    f.write(str(count))
                os.rename(sys.argv[0] + '.result', sys.argv[0] + '.closed')
                os._exit(0)
            count += len(chunk)
        except BlockingIOError:
            time.sleep(0.01)
    os._exit(0)
os.write(1, b'[]')
os._exit(0)
"#,
        );
        let input = "w".repeat(2 * 1024 * 1024);
        let started = Instant::now();
        let error = infer(&command, kiji, &input).unwrap_err();
        assert!(
            matches!(error, SafetyNetError::Runtime { ref message } if message.contains("timed out")),
            "{error:?}"
        );
        assert!(dir.path().join("synthetic-backend.ready").exists());
        assert!(started.elapsed() < Duration::from_secs(3));
        let closed = dir.path().join("synthetic-backend.closed");
        let deadline = Instant::now() + Duration::from_secs(4);
        while !closed.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let delivered: usize = fs::read_to_string(closed)
            .expect("writer must close for EOF")
            .parse()
            .unwrap();
        assert!(
            delivered < input.len(),
            "cancelled writer must not finish the pending input"
        );
    }
}
