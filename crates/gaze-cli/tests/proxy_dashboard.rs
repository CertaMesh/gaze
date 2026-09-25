//! Integration suite for the `gaze proxy` dashboard opt-in surface.
//!
//! Covers the CLI-owned seam: default-off observability, every flag-decision
//! failure path (owner acknowledgements, stdio pairing descriptors, bind and
//! retention validation), pre-spawn activation failure with provider
//! continuity, sanitized child-mode errors, and the daemon relay flag
//! surface. The registration-bound activation lifecycle, post-registration
//! failure paths, purge/disable serialization, process isolation, and
//! crash/no-dump behavior are exhaustively proven by the
//! `gaze-proxy-dashboard` and `gaze-inspection` crate suites, which the
//! workspace test run executes; the spawned-child success paths among them
//! are platform-gated to non-macOS Unix by the child's mandatory
//! `RLIMIT_CORE=0` no-dump readiness gate.

#![cfg(all(feature = "dashboard", unix))]

use std::io::Read;
use std::net::{SocketAddr, TcpListener};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

#[path = "support/proxy_health.rs"]
mod proxy_health;
use proxy_health::proxy_answers_health;

fn gaze_binary() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin("gaze"))
}

/// Drives `gaze proxy serve` without asserting how fast it starts (user ruling
/// 2026-09-16). The CLI prints every dashboard decision line before the
/// provider binds, so a provider that answers its health check has already
/// written all of them. Each test therefore waits only for that answer
/// (failing fast if the provider exits), then stops the provider and checks
/// its whole stderr: a missing line fails at once instead of hanging on a live
/// provider.
struct ServeProbe {
    child: Child,
    stderr: std::sync::mpsc::Receiver<String>,
    addr: SocketAddr,
    // Declared last so it is released only after `Drop` has stopped the child.
    _one_at_a_time: MutexGuard<'static, ()>,
}

/// Runs one provider at a time. Each probe reserves a port, frees it for the
/// provider, and polls it until the provider serves. With several probes
/// polling at once, one probe's outgoing connection can take another's freed
/// port, so that provider's bind fails ("http server failed", exit 7), or two
/// polling sockets pair up and `connect` succeeds with nothing listening. A
/// 1 s slow-startup probe produced both failures.
static ONE_PROVIDER_AT_A_TIME: Mutex<()> = Mutex::new(());

impl ServeProbe {
    fn spawn(dashboard_flags: &[&str]) -> Self {
        // A test that panicked while holding the lock poisons it; the lock
        // still serializes, so take it anyway.
        let one_at_a_time = ONE_PROVIDER_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let addr = TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("reserve a loopback port");
        let mut command = gaze_binary();
        command
            .args(["proxy", "serve", "--bind", &addr.to_string()])
            .args(dashboard_flags)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn gaze proxy serve");
        let mut stderr_pipe = child.stderr.take().expect("piped stderr");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            let mut pending = String::new();
            loop {
                match stderr_pipe.read(&mut buffer) {
                    Ok(0) | Err(_) => {
                        if !pending.is_empty() {
                            let _ = sender.send(pending);
                        }
                        break;
                    }
                    Ok(n) => {
                        pending.push_str(&String::from_utf8_lossy(&buffer[..n]));
                        while let Some(newline) = pending.find('\n') {
                            let line = pending[..newline].to_owned();
                            pending = pending[newline + 1..].to_owned();
                            if sender.send(line).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        Self {
            child,
            stderr: receiver,
            addr,
            _one_at_a_time: one_at_a_time,
        }
    }

    /// Proves the provider is serving: waits until it answers its health
    /// check, failing as soon as it exits instead of after a fixed sleep.
    fn assert_provider_serving(&mut self) {
        while !proxy_answers_health(self.addr) {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                // The provider has exited, so its stderr is at EOF.
                let stderr: Vec<String> = self.stderr.iter().collect();
                panic!(
                    "provider process exited ({status}); dashboard failure must leave the \
                     provider running: {stderr:?}"
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            self.child.try_wait().expect("try_wait").is_none(),
            "provider process exited; dashboard failure must leave the provider running"
        );
    }

    /// Stops a provider that `assert_provider_serving` saw serving and
    /// returns its stderr read to EOF. Every dashboard line precedes the bind,
    /// so the stream is final: a line's presence or absence is a real result,
    /// not a timing window.
    fn stop_and_collect_stderr(mut self) -> Vec<String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stderr.iter().collect()
    }
}

/// The line of a stderr collected to EOF that contains `needle`.
fn line_containing<'a>(stderr: &'a [String], needle: &str) -> &'a str {
    stderr
        .iter()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no stderr line contains `{needle}`: {stderr:?}"))
}

fn assert_no_line_containing(stderr: &[String], needle: &str) {
    for line in stderr {
        assert!(
            !line.contains(needle),
            "unexpected line containing `{needle}`: {line}"
        );
    }
}

impl Drop for ServeProbe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn serve_without_dashboard_flags_prints_no_dashboard_line_and_serves() {
    let mut probe = ServeProbe::spawn(&[]);
    probe.assert_provider_serving();
    assert_no_line_containing(&probe.stop_and_collect_stderr(), "gaze dashboard");
}

#[test]
fn owner_raw_flag_without_dashboard_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&[
        "--dashboard-capture-owner-raw",
        "--dashboard-acknowledge-owner-raw-risk",
    ]);
    probe.assert_provider_serving();
    let stderr = probe.stop_and_collect_stderr();
    assert!(line_containing(&stderr, "gaze dashboard disabled:").contains("--dashboard"));
}

#[test]
fn owner_raw_capture_without_acknowledgement_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-capture-owner-raw"]);
    probe.assert_provider_serving();
    line_containing(&probe.stop_and_collect_stderr(), "gaze dashboard disabled:");
}

#[test]
fn owner_restored_acknowledgement_without_capture_disables_dashboard_only() {
    let mut probe =
        ServeProbe::spawn(&["--dashboard", "--dashboard-acknowledge-owner-restored-risk"]);
    probe.assert_provider_serving();
    line_containing(&probe.stop_and_collect_stderr(), "gaze dashboard disabled:");
}

#[test]
fn stdio_pairing_descriptor_disables_dashboard_only() {
    for fd in ["0", "1", "2"] {
        let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-pairing-fd", fd]);
        probe.assert_provider_serving();
        let stderr = probe.stop_and_collect_stderr();
        assert!(line_containing(&stderr, "gaze dashboard disabled:").contains("descriptor"));
    }
}

#[test]
fn non_loopback_dashboard_bind_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-bind", "192.0.2.7:0"]);
    probe.assert_provider_serving();
    let stderr = probe.stop_and_collect_stderr();
    assert!(line_containing(&stderr, "gaze dashboard disabled:").contains("loopback"));
}

#[test]
fn loopback_dashboard_bind_with_port_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-bind", "127.0.0.1:8080"]);
    probe.assert_provider_serving();
    line_containing(&probe.stop_and_collect_stderr(), "gaze dashboard disabled:");
}

#[test]
fn retention_over_crate_ceiling_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-max-events", "1025"]);
    probe.assert_provider_serving();
    let stderr = probe.stop_and_collect_stderr();
    assert!(line_containing(&stderr, "gaze dashboard disabled:").contains("ceiling"));
}

#[test]
fn invalid_dashboard_ttl_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-ttl", "soon"]);
    probe.assert_provider_serving();
    line_containing(&probe.stop_and_collect_stderr(), "gaze dashboard disabled:");
}

/// A pairing descriptor that passes flag validation but is not open in this
/// process must fail closed at the delivery step before any child, token, or
/// sink exists, and the provider must keep serving. This is the
/// deterministic activation-failure probe: unlike the controlling-terminal
/// path it does not depend on whether the test runner has a TTY.
#[test]
fn dashboard_with_unopened_pairing_descriptor_disables_and_provider_continues() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-pairing-fd", "27"]);
    probe.assert_provider_serving();
    let stderr = probe.stop_and_collect_stderr();
    assert!(line_containing(&stderr, "gaze dashboard disabled:").contains("descriptor"));
    assert_no_line_containing(&stderr, "gaze dashboard active");
}

/// The hidden child mode is data-free on handle failure: one sanitized closed
/// error line, nonzero exit, and no token or payload bytes.
#[test]
fn child_mode_without_inherited_handles_exits_sanitized() {
    let output = gaze_binary()
        .args([
            "proxy",
            "_dashboard-child",
            "--bind-addr",
            "127.0.0.1",
            "--ttl-secs",
            "30",
            "--max-events",
            "4",
            "--max-bytes",
            "65536",
        ])
        .env_remove("GAZE_DASHBOARD_CONTROL_SOCKET_V1")
        .env_remove("GAZE_DASHBOARD_INSPECTION_SOCKET_V1")
        .stdin(Stdio::null())
        .output()
        .expect("run child mode");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("dashboard child:"));
    assert!(!stderr.contains("GazeDashboardV1"));
    assert!(output.stdout.is_empty());
}

#[test]
fn child_mode_is_hidden_from_help() {
    let output = gaze_binary()
        .args(["proxy", "--help"])
        .output()
        .expect("proxy help");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(!help.contains("_dashboard-child"));
}

/// `gaze proxy start` exposes the relayed dashboard flag surface; `restart`
/// intentionally exposes none (dashboard activation is per-invocation and
/// never persisted into the daemon config).
#[test]
fn start_exposes_dashboard_flags_and_restart_does_not() {
    let start_help = gaze_binary()
        .args(["proxy", "start", "--help"])
        .output()
        .expect("start help");
    assert!(start_help.status.success());
    let start_text = String::from_utf8_lossy(&start_help.stdout);
    for flag in [
        "--dashboard",
        "--dashboard-capture-owner-raw",
        "--dashboard-acknowledge-owner-raw-risk",
        "--dashboard-capture-owner-restored",
        "--dashboard-acknowledge-owner-restored-risk",
        "--dashboard-bind",
        "--dashboard-ttl",
        "--dashboard-max-events",
        "--dashboard-max-bytes",
        "--dashboard-pairing-fd",
    ] {
        assert!(start_text.contains(flag), "start help must list {flag}");
    }

    let restart_help = gaze_binary()
        .args(["proxy", "restart", "--help"])
        .output()
        .expect("restart help");
    assert!(restart_help.status.success());
    let restart_text = String::from_utf8_lossy(&restart_help.stdout);
    assert!(!restart_text.contains("--dashboard"));
}

#[test]
fn serve_exposes_dashboard_flags() {
    let output = gaze_binary()
        .args(["proxy", "serve", "--help"])
        .output()
        .expect("serve help");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for flag in ["--dashboard", "--dashboard-bind", "--dashboard-pairing-fd"] {
        assert!(help.contains(flag), "serve help must list {flag}");
    }
}
