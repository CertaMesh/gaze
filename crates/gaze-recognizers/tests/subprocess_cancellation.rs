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

/// Caps retries so sustained overload fails with a diagnosis instead of
/// spinning until the CI job timeout.
const MAX_FORK_ATTEMPTS: u32 = 6;

/// Runs the fixture until an attempt forks its descendant before the backend
/// deadline kills the fixture's parent.
///
/// The deadline counts from spawn, so a loaded host can kill the parent before
/// it forks; that attempt proves nothing and the next one doubles the
/// deadline. The parent writes `.forked` right after `fork()`, and `infer`
/// has reaped the parent before it returns, so a missing marker is final:
/// the test never waits on the fixture's startup.
fn infer_after_backend_forks(
    body: &str,
    input: &str,
) -> (tempfile::TempDir, SafetyNetError, Vec<tempfile::TempDir>) {
    let mut timeout = Duration::from_secs(1);
    // Kept alive so a descendant forked just before the kill still finds
    // `.release` and exits instead of polling a deleted directory.
    let mut abandoned = Vec::new();
    for _ in 0..MAX_FORK_ATTEMPTS {
        let dir = tempfile::tempdir().unwrap();
        let command = script(dir.path(), body);
        let error = infer_with_timeout(&command, input, timeout).unwrap_err();
        if dir.path().join("synthetic-backend.forked").exists() {
            return (dir, error, abandoned);
        }
        release(dir.path());
        abandoned.push(dir);
        timeout = (timeout * 2).min(Duration::from_secs(30));
    }
    panic!(
        "fixture was killed before forking on all {MAX_FORK_ATTEMPTS} attempts \
         (deadlines doubled from 1s up to {timeout:?})"
    );
}

fn release(dir: &Path) {
    fs::write(dir.join("synthetic-backend.release"), b"").unwrap();
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
        let (dir, error, _abandoned) = infer_after_backend_forks(
            &format!(
                r#"#!/usr/bin/env python3
import os, sys, time
sys.stdin.buffer.read()
if os.fork() != 0:
    open(sys.argv[0] + '.forked', 'w').close()
else:
    os.close(0)
    os.close({other})
    fd = {held}
    os.set_blocking(fd, False)
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
    // The descendant leaves stdin unread until the test releases it, so the
    // pipe stays full past any deadline and only cancellation can end the
    // write.
    let body = r#"#!/usr/bin/env python3
import os, sys, time
if os.fork() != 0:
    open(sys.argv[0] + '.forked', 'w').close()
else:
    os.close(1)
    os.close(2)
    end = time.monotonic() + 60
    while not os.path.exists(sys.argv[0] + '.release') and time.monotonic() < end:
        time.sleep(0.01)
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
    let (dir, error, _abandoned) = infer_after_backend_forks(body, &input);
    assert!(
        matches!(error, SafetyNetError::Runtime { ref message } if message.contains("timed out")),
        "{error:?}"
    );
    release(dir.path());
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
