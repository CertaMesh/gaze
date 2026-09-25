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
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn gaze_binary() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin("gaze"))
}

/// Drives `gaze proxy serve` without asserting how fast it starts (user ruling
/// 2026-09-16). The CLI prints every dashboard decision line before the
/// provider binds, so a provider that accepts connections has already
/// written all of them; that makes both waits below deadline-free.
struct ServeProbe {
    child: Child,
    stderr: std::sync::mpsc::Receiver<String>,
    addr: SocketAddr,
}

impl ServeProbe {
    fn spawn(dashboard_flags: &[&str]) -> Self {
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
                    Ok(0) | Err(_) => break,
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
        }
    }

    /// Blocks until a stderr line contains `needle`. No deadline: the line
    /// arrives or the provider exits and closes stderr, which fails the wait.
    fn wait_for_line(&mut self, needle: &str) -> String {
        loop {
            match self.stderr.recv() {
                Ok(line) if line.contains(needle) => return line,
                Ok(_) => continue,
                Err(_) => panic!("stderr closed before `{needle}` appeared"),
            }
        }
    }

    /// Proves the provider is serving: waits until it accepts a connection,
    /// failing as soon as it exits instead of after a fixed sleep.
    fn assert_provider_serving(&mut self) {
        while TcpStream::connect(self.addr).is_err() {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                panic!(
                    "provider process exited ({status}); dashboard failure must leave the \
                     provider running"
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            self.child.try_wait().expect("try_wait").is_none(),
            "provider process exited; dashboard failure must leave the provider running"
        );
    }

    /// Stops a provider that `assert_provider_serving` saw listening, then
    /// reads its stderr to EOF. Every dashboard line precedes the bind, so
    /// the full stream is final and absence is a real result, not a timing
    /// window.
    fn stop_and_assert_no_line_containing(mut self, needle: &str) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for line in self.stderr.iter() {
            assert!(
                !line.contains(needle),
                "unexpected line containing `{needle}`: {line}"
            );
        }
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
    probe.stop_and_assert_no_line_containing("gaze dashboard");
}

#[test]
fn owner_raw_flag_without_dashboard_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&[
        "--dashboard-capture-owner-raw",
        "--dashboard-acknowledge-owner-raw-risk",
    ]);
    let line = probe.wait_for_line("gaze dashboard disabled:");
    assert!(line.contains("--dashboard"));
    probe.assert_provider_serving();
}

#[test]
fn owner_raw_capture_without_acknowledgement_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-capture-owner-raw"]);
    probe.wait_for_line("gaze dashboard disabled:");
    probe.assert_provider_serving();
}

#[test]
fn owner_restored_acknowledgement_without_capture_disables_dashboard_only() {
    let mut probe =
        ServeProbe::spawn(&["--dashboard", "--dashboard-acknowledge-owner-restored-risk"]);
    probe.wait_for_line("gaze dashboard disabled:");
    probe.assert_provider_serving();
}

#[test]
fn stdio_pairing_descriptor_disables_dashboard_only() {
    for fd in ["0", "1", "2"] {
        let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-pairing-fd", fd]);
        let line = probe.wait_for_line("gaze dashboard disabled:");
        assert!(line.contains("descriptor"));
        probe.assert_provider_serving();
    }
}

#[test]
fn non_loopback_dashboard_bind_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-bind", "192.0.2.7:0"]);
    let line = probe.wait_for_line("gaze dashboard disabled:");
    assert!(line.contains("loopback"));
    probe.assert_provider_serving();
}

#[test]
fn loopback_dashboard_bind_with_port_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-bind", "127.0.0.1:8080"]);
    probe.wait_for_line("gaze dashboard disabled:");
    probe.assert_provider_serving();
}

#[test]
fn retention_over_crate_ceiling_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-max-events", "1025"]);
    let line = probe.wait_for_line("gaze dashboard disabled:");
    assert!(line.contains("ceiling"));
    probe.assert_provider_serving();
}

#[test]
fn invalid_dashboard_ttl_disables_dashboard_only() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-ttl", "soon"]);
    probe.wait_for_line("gaze dashboard disabled:");
    probe.assert_provider_serving();
}

/// A pairing descriptor that passes flag validation but is not open in this
/// process must fail closed at the delivery step before any child, token, or
/// sink exists, and the provider must keep serving. This is the
/// deterministic activation-failure probe: unlike the controlling-terminal
/// path it does not depend on whether the test runner has a TTY.
#[test]
fn dashboard_with_unopened_pairing_descriptor_disables_and_provider_continues() {
    let mut probe = ServeProbe::spawn(&["--dashboard", "--dashboard-pairing-fd", "27"]);
    let line = probe.wait_for_line("gaze dashboard disabled:");
    assert!(line.contains("descriptor"));
    probe.assert_provider_serving();
    probe.stop_and_assert_no_line_containing("gaze dashboard active");
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
