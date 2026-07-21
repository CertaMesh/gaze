#![cfg(unix)]
#![cfg_attr(target_os = "macos", allow(dead_code, unused_imports))]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use gaze_proxy_dashboard::{NoDumpReadiness, NoDumpReadiness::Verified};

const SYNTHETIC_CANARY: &str = "alice@example.invalid";

#[test]
#[ignore = "subprocess helper only"]
fn crash_helper() {
    let Ok(mode) = std::env::var("GAZE_CRASH_MODE") else {
        return;
    };
    assert_eq!(NoDumpReadiness::install_and_verify(), Verified);
    let canary = std::env::var("GAZE_SYNTHETIC_CANARY").unwrap();
    match mode.as_str() {
        "panic" => panic!("{canary}"),
        "abort" => {
            std::hint::black_box(canary);
            std::process::abort();
        }
        "kill" => {
            std::hint::black_box(canary);
            println!("ready");
            std::thread::sleep(Duration::from_secs(30));
        }
        _ => unreachable!(),
    }
}

#[test]
#[cfg(not(target_os = "macos"))]
fn unix_child_readiness_sets_and_verifies_zero_core_limit() {
    assert_eq!(NoDumpReadiness::install_and_verify(), Verified);
    use rustix::process::{getrlimit, Resource};
    let limit = getrlimit(Resource::Core);
    assert_eq!(limit.current, Some(0));
    assert_eq!(limit.maximum, Some(0));
}

#[test]
#[cfg(not(target_os = "macos"))]
fn forced_panic_emits_no_canary_or_owned_artifact() {
    let (output, artifact_count) = run_to_exit("panic");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SYNTHETIC_CANARY));
    assert_eq!(artifact_count, 0);
}

#[cfg(target_os = "linux")]
#[test]
fn forced_abort_emits_no_canary_or_owned_artifact() {
    let (output, artifact_count) = run_to_exit("abort");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SYNTHETIC_CANARY));
    assert_eq!(artifact_count, 0);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn forced_kill_leaves_no_owned_artifact_or_orphan() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = helper_command("kill", temp.path())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut ready = false;
    for _ in 0..8 {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line.contains("ready") {
            ready = true;
            break;
        }
    }
    assert!(ready);
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn darwin_is_honestly_unsupported_before_sensitive_child_startup() {
    assert_eq!(
        NoDumpReadiness::install_and_verify(),
        NoDumpReadiness::Unsupported
    );
}

fn run_to_exit(mode: &str) -> (std::process::Output, usize) {
    let temp = tempfile::tempdir().unwrap();
    let output = helper_command(mode, temp.path()).output().unwrap();
    let count = std::fs::read_dir(temp.path()).unwrap().count();
    (output, count)
}

fn helper_command(mode: &str, current_dir: &std::path::Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "crash_helper", "--nocapture"])
        .env("GAZE_CRASH_MODE", mode)
        .env("GAZE_SYNTHETIC_CANARY", SYNTHETIC_CANARY)
        .env("TMPDIR", current_dir)
        .current_dir(current_dir)
        .stderr(Stdio::piped());
    command
}
