#![cfg(all(unix, feature = "safety-net-openai"))]

use gaze_recognizers::safety_net::openai_filter::{
    OpenAiFilterBackend, SubprocessOpenAiFilterBackend, SubprocessOpenAiFilterConfig,
};
use gaze_types::SafetyNetError;
use serial_test::file_serial;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

fn infer_with_timeout(
    command: &Path,
    input: &str,
    timeout: Duration,
) -> Result<(), SafetyNetError> {
    SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(command)
            .with_timeout(timeout)
            .with_max_input_bytes(input.len().max(1))
            .with_stderr_diagnostics(true),
    )
    .unwrap()
    .infer(input)
    .map(|_| ())
}

fn infer(command: &Path, input: &str) -> Result<(), SafetyNetError> {
    infer_with_timeout(command, input, Duration::from_secs(1))
}

fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn infer_after_backend_starts(body: &str, input: &str) -> (tempfile::TempDir, SafetyNetError) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let dir = tempfile::tempdir().unwrap();
        let command = script(dir.path(), body);
        let error = infer(&command, input).unwrap_err();
        // A heavily loaded host can spend the whole backend timeout before
        // the fixture starts. Only inspect cancellation on a started attempt.
        if wait_for_file(
            &dir.path().join("synthetic-backend.ready"),
            Duration::from_millis(500),
        ) {
            return (dir, error);
        }
        assert!(Instant::now() < deadline, "synthetic backend never started");
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
fn discards_unfinished_unicode_email() {
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
    // This assertion concerns the final returned diagnostic. Give the
    // subprocess time to finish writing before evaluating that value.
    let SafetyNetError::Runtime { message } =
        infer_with_timeout(&command, "clean", Duration::from_secs(30)).unwrap_err()
    else {
        panic!("expected runtime error")
    };
    assert!(!message.contains("alice"));
    assert!(message.ends_with("[truncated]"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn closes_descendant_held_read_pipes_before_returning() {
    for held in [1, 2] {
        // The descendant observes EPIPE after infer returns. A detached reader
        // would keep the pipe open and make this fail even if return was fast.
        let (dir, error) = infer_after_backend_starts(
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
    end = time.monotonic() + 60
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
            "clean",
        );
        assert!(
            matches!(error, SafetyNetError::Runtime { ref message } if message.contains("timed out")),
            "{error:?}"
        );
        let closed = dir.path().join("synthetic-backend.closed");
        assert!(
            wait_for_file(&closed, Duration::from_secs(30)),
            "reader pipe remained open: fd={held}"
        );
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn cancels_full_stdin_and_closes_the_writer() {
    let body = r#"#!/usr/bin/env python3
import os, sys, time
if os.fork() == 0:
    os.close(1)
    os.close(2)
    open(sys.argv[0] + '.ready', 'w').close()
    time.sleep(2)
    count = 0
    os.set_blocking(0, False)
    end = time.monotonic() + 60
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
"#;
    let input = "w".repeat(2 * 1024 * 1024);
    let (dir, error) = infer_after_backend_starts(body, &input);
    assert!(
        matches!(error, SafetyNetError::Runtime { ref message } if message.contains("timed out")),
        "{error:?}"
    );
    let closed = dir.path().join("synthetic-backend.closed");
    assert!(
        wait_for_file(&closed, Duration::from_secs(30)),
        "writer must close for EOF"
    );
    let delivered: usize = fs::read_to_string(closed)
        .expect("writer must close for EOF")
        .parse()
        .unwrap();
    assert!(
        delivered < input.len(),
        "cancelled writer must not finish the pending input"
    );
}
