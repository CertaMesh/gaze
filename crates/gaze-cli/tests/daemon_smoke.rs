use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use serial_test::file_serial;
use tempfile::tempdir;

const PARITY_INPUT: &str = "id ES-TEST-123456 track Sonnenlied";

fn write_cross_verb_parity_policy() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let rulepack_path = dir.path().join("cross-verb.toml");
    fs::write(
        &rulepack_path,
        r#"
schema_version = "0.1.0"
rulepack_id = "cross-verb"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "es.test_id"
class = "custom:es_test_id"
enabled = true
safety_tier = "locale_gated"
locales = ["es-ES"]

[recognizers.match]
kind = "regex"
pattern = '''ES-TEST-[0-9]{6}'''

[[recognizers]]
id = "dictionary.synthetic_catalog"
class = "custom:catalog_title"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "dictionary"
terms = ["Sonnenlied"]
"#,
    )
    .unwrap();
    let policy_path = dir.path().join("policy.toml");
    fs::write(
        &policy_path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = ["core-extended"]
paths = ["{}"]

[[rule]]
kind = "class"
class = "custom:es_test_id"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:catalog_title"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
            rulepack_path.display()
        ),
    )
    .unwrap();
    (dir, policy_path)
}

fn parity_classes(text: &str) -> BTreeSet<&'static str> {
    [
        ("Custom:catalog_title", "Custom:catalog_title_"),
        ("Custom:es_test_id", "Custom:es_test_id_"),
    ]
    .into_iter()
    .filter_map(|(class, marker)| text.contains(marker).then_some(class))
    .collect()
}

fn unused_local_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut expected_len = None;
    loop {
        let read = stream.read(&mut chunk).unwrap();
        assert!(read > 0, "HTTP peer closed before request completed");
        bytes.extend_from_slice(&chunk[..read]);
        if expected_len.is_none() {
            if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                expected_len = Some(header_end + 4 + content_length);
            }
        }
        if expected_len.is_some_and(|length| bytes.len() >= length) {
            return bytes;
        }
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn direct_proxy_body(policy: &std::path::Path) -> String {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let (capture_tx, capture_rx) = mpsc::sync_channel(1);
    let upstream_thread = thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let request = read_http_request(&mut stream);
        capture_tx.send(request).unwrap();
        let body = r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"text","text":"synthetic-safe"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });

    let proxy_addr = unused_local_addr();
    let upstream_url = format!("http://{upstream_addr}");
    let child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "proxy",
            "serve",
            "--bind",
            &proxy_addr.to_string(),
            "--upstream-anthropic",
            &upstream_url,
            "--policy",
            policy.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _child = ChildGuard(child);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(proxy_addr).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "proxy did not start at {proxy_addr}"
        );
        thread::sleep(Duration::from_millis(20));
    }

    let request_body = json!({
        "model": "claude-test",
        "max_tokens": 32,
        "messages": [{"role": "user", "content": PARITY_INPUT}],
        "stream": false
    })
    .to_string();
    let mut stream = TcpStream::connect(proxy_addr).unwrap();
    write!(
        stream,
        "POST /v1/messages HTTP/1.1\r\nhost: {proxy_addr}\r\nx-api-key: synthetic-key\r\nanthropic-version: 2023-06-01\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        request_body.len(),
        request_body
    )
    .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    assert!(
        response.starts_with(b"HTTP/1.1 200"),
        "proxy response was not 200"
    );

    let captured = capture_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    upstream_thread.join().unwrap();
    let body_start = captured
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap()
        + 4;
    String::from_utf8(captured[body_start..].to_vec()).unwrap()
}

#[test]
fn clean_daemon_and_direct_proxy_share_auto_activated_locales_and_dictionaries() {
    let (_dir, policy) = write_cross_verb_parity_policy();
    let expected = BTreeSet::from(["Custom:catalog_title", "Custom:es_test_id"]);

    let mut clean = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args(["clean", "--policy", policy.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    clean
        .stdin
        .as_mut()
        .unwrap()
        .write_all(PARITY_INPUT.as_bytes())
        .unwrap();
    drop(clean.stdin.take());
    let clean = clean.wait_with_output().unwrap();
    assert!(
        clean.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&clean.stderr)
    );
    let clean: Value = serde_json::from_slice(&clean.stdout).unwrap();

    let mut daemon = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "daemon",
            "--policy",
            policy.to_str().unwrap(),
            "--idle-timeout",
            "30",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        daemon.stdin.as_mut().unwrap(),
        "{}",
        json!({"session_id": "parity-session", "text": PARITY_INPUT})
    )
    .unwrap();
    drop(daemon.stdin.take());
    let daemon = daemon.wait_with_output().unwrap();
    assert!(
        daemon.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&daemon.stderr)
    );
    let daemon: Value = serde_json::from_slice(&daemon.stdout).unwrap();

    let proxy = direct_proxy_body(&policy);
    assert_eq!(
        parity_classes(clean["clean_text"].as_str().unwrap()),
        expected
    );
    assert_eq!(
        parity_classes(daemon["clean_text"].as_str().unwrap()),
        expected
    );
    assert_eq!(parity_classes(&proxy), expected);
    assert!(!proxy.contains("ES-TEST-123456"));
    assert!(!proxy.contains("Sonnenlied"));
}

fn write_policy() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'alice@example[.]invalid'
class = "email"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();
    (dir, path)
}

/// Drives `count` JSONL requests through one `gaze daemon` process, alternating
/// between two session ids so per-session manifest isolation is exercised.
///
/// Returns the wall-clock duration next to the output instead of asserting on
/// it: throughput is a performance property, and only the opt-in throughput
/// test below looks at the duration (solo #2981).
fn run_daemon_batch(
    policy: &std::path::Path,
    audit_db: &std::path::Path,
    count: usize,
) -> (Duration, Output) {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "daemon",
            "--policy",
            policy.to_str().unwrap(),
            "--audit-db",
            audit_db.to_str().unwrap(),
            "--idle-timeout",
            "30",
            "--session-cap",
            "1000",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let started = Instant::now();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for idx in 0..count {
            let session_id = if idx % 2 == 0 {
                "session-a"
            } else {
                "session-b"
            };
            let request = json!({
                "session_id": session_id,
                "text": format!("Contact alice@example.invalid for ticket {idx}")
            });
            writeln!(stdin, "{request}").unwrap();
        }
    }

    let output = child.wait_with_output().unwrap();
    (started.elapsed(), output)
}

/// Budget for the opt-in throughput test, mirroring the
/// `GAZE_TEST_SUBPROCESS_TIMEOUT_SECS` helper #430 introduced: generous by
/// default, tightenable when a run deliberately hunts a perf regression.
fn daemon_throughput_budget() -> Duration {
    let seconds = std::env::var("GAZE_TEST_DAEMON_THROUGHPUT_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("daemon throughput budget must be an integer")
        })
        .unwrap_or(60);
    assert!(seconds > 0, "daemon throughput budget must be positive");
    Duration::from_secs(seconds)
}

// What this guarantees: every one of the 100 JSONL requests is answered, each
// answer is tokenized and carries a non-empty manifest, `session-a` and
// `session-b` never share a token (per-session manifest isolation), and every
// audit row is attributed to the daemon stage. How *fast* the daemon got there
// is not one of those properties - see the opt-in throughput test below.
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_processes_jsonl_and_isolates_sessions() {
    let (_dir, policy) = write_policy();
    let audit_dir = tempdir().unwrap();
    let audit_db = audit_dir.path().join("audit.db");
    let (_elapsed, output) = run_daemon_batch(&policy, &audit_db, 100);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let responses = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 100);
    assert!(responses
        .iter()
        .all(|value| value["clean_text"].as_str().unwrap().contains("<")));
    assert!(responses.iter().all(|value| {
        value["manifest"]
            .as_array()
            .is_some_and(|spans| !spans.is_empty())
    }));

    let token_a = responses
        .iter()
        .find(|value| value["session_id"] == "session-a")
        .unwrap()["tokens"][0]["token"]
        .as_str()
        .unwrap()
        .to_string();
    let token_b = responses
        .iter()
        .find(|value| value["session_id"] == "session-b")
        .unwrap()["tokens"][0]["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(token_a, token_b, "session manifests must not bleed");

    let conn = rusqlite::Connection::open(audit_db).unwrap();
    let mut stmt = conn
        .prepare("SELECT provenance_stage FROM redaction_log ORDER BY created_at")
        .unwrap();
    let stages = stmt
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!stages.is_empty());
    assert!(stages
        .iter()
        .all(|stage| stage.as_deref() == Some("daemon")));
}

// Throughput used to be asserted inside the correctness test above with a hard
// `elapsed < 10s`. A busy runner measured 10.845s and took main's `xtask gates`
// run down with it (solo #2981) - the same fixed-budget-loses-its-race class as
// solo #2404 / #2916, which #430 fixed for subprocess timeouts. A stopwatch
// proves nothing about correctness, so it lives here instead: `#[ignore]` keeps
// it out of every CI invocation (`cargo test` never passes `--include-ignored`
// in this repo) while it stays runnable on demand with
// `cargo test -p gaze-cli --all-features --test daemon_smoke -- --ignored`.
#[test]
#[ignore = "throughput budget, not a correctness property; run explicitly with --ignored"]
#[file_serial(gaze_subprocess)]
fn daemon_throughput_stays_within_opt_in_budget() {
    let (_dir, policy) = write_policy();
    let audit_dir = tempdir().unwrap();
    let (elapsed, output) = run_daemon_batch(&policy, &audit_dir.path().join("audit.db"), 100);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answered = String::from_utf8(output.stdout).unwrap().lines().count();
    assert_eq!(answered, 100, "throughput run must answer every request");

    let budget = daemon_throughput_budget();
    assert!(
        elapsed < budget,
        "100 daemon requests took {elapsed:?} against a {budget:?} budget; \
         override with GAZE_TEST_DAEMON_THROUGHPUT_SECS"
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn daemon_malformed_json_fails_closed_and_continues() {
    let (_dir, policy) = write_policy();
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "daemon",
            "--policy",
            policy.to_str().unwrap(),
            "--idle-timeout",
            "30",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, "{{not-json").unwrap();
        writeln!(
            stdin,
            "{}",
            json!({"session_id": "session-a", "text": "alice@example.invalid"})
        )
        .unwrap();
    }

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let responses = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["error"], "JsonMalformed");
    assert_eq!(responses[1]["session_id"], "session-a");
    assert!(responses[1]["clean_text"].as_str().unwrap().contains("<"));
}

// S10-F2 (audit 7201) drift gate for the daemon path: the auto-activate locale
// set is derived from the loaded rulepacks (same source of truth as
// `gaze clean`). An adopter path pack with a document-basis `locale_gated`
// es-ES recognizer activates under `core-extended` with no policy locale.
fn write_policy_with_es_locale_gated_path_rulepack() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let rulepack_path = dir.path().join("es-locale-gated.toml");
    fs::write(
        &rulepack_path,
        r#"
schema_version = "0.1.0"
rulepack_id = "es-locale-gated"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "es.test_id"
class = "custom:es_test_id"
enabled = true
safety_tier = "locale_gated"
locales = ["es-ES"]

[recognizers.match]
kind = "regex"
pattern = '''ES-TEST-[0-9]{6}'''
"#,
    )
    .unwrap();
    let policy_path = dir.path().join("policy.toml");
    fs::write(
        &policy_path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = ["core-extended"]
paths = ["{}"]

[[rule]]
kind = "class"
class = "custom:es_test_id"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
            rulepack_path.display()
        ),
    )
    .unwrap();
    (dir, policy_path)
}

#[test]
fn daemon_auto_activate_derives_locale_gated_locales_from_loaded_rulepacks() {
    let (_dir, policy) = write_policy_with_es_locale_gated_path_rulepack();
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "daemon",
            "--policy",
            policy.to_str().unwrap(),
            "--idle-timeout",
            "30",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            json!({"session_id": "session-es", "text": "id ES-TEST-123456"})
        )
        .unwrap();
    }

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let responses = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 1, "{stdout}");
    let clean = responses[0]["clean_text"].as_str().unwrap();
    assert!(
        clean.contains(":Custom:es_test_id_"),
        "es-ES locale-gated recognizer must auto-activate in the daemon: {clean}"
    );
}

// solo todo #2965. `gaze proxy start` spawns a detached child, and nothing here
// drove traffic through that child before this test — `proxy_dashboard.rs` only
// asserted the `proxy start --help` flag surface. The daemonized path is the one
// adopters run in production, so it has to resolve the same pipeline as
// `gaze clean`; whatever it silently drops is policy the chokepoint ignores.

/// A `gaze` invocation whose daemon state (pidfile, config, logs) is redirected
/// into `home`, so a test daemon can never collide with the developer's real
/// `gaze proxy`. `XDG_*` is cleared because `dirs` prefers those over `HOME`.
fn gaze_with_home(home: &Path) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("gaze"));
    command
        .env("HOME", home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .env_remove("XDG_CACHE_HOME");
    command
}

/// Finds `name` anywhere under `root`. The daemon's state layout is
/// platform-specific, and the test only needs to prove that state landed inside
/// the redirected `HOME`; a walk stays correct if the mapping ever changes.
fn find_under(root: &Path, name: &str) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().is_some_and(|found| found == name) {
                return Some(path);
            }
        }
    }
    None
}

/// Daemon log tail for assertion messages, so a failure reports what the
/// detached child did instead of only that it never answered.
fn daemon_log_tail(home: &Path) -> String {
    ["proxy.log", "proxy-stderr.log"]
        .into_iter()
        .filter_map(|name| {
            let text = fs::read_to_string(find_under(home, name)?).ok()?;
            (!text.trim().is_empty()).then(|| format!("\n--- {name} ---\n{text}"))
        })
        .collect()
}

/// Stops the detached daemon however the test ended, so a failed assertion
/// cannot leave a proxy running on the developer's machine.
struct DaemonGuard {
    home: PathBuf,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = gaze_with_home(&self.home)
            .args(["proxy", "stop", "--force", "--timeout", "5s"])
            .output();
        // Backstop for a `stop` that never reached the child: the pidfile is
        // written before the daemon serves anything.
        if let Some(pid) = find_under(&self.home, "proxy.pid")
            .and_then(|pidfile| fs::read_to_string(pidfile).ok())
            .and_then(|text| text.lines().next()?.trim().parse::<u32>().ok())
        {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).output();
        }
    }
}

/// Drives one request through the *daemonized* proxy (`gaze proxy start`, the
/// path that spawns a detached child) and returns the body the loopback
/// upstream actually received.
fn daemonized_proxy_body(policy: &Path) -> String {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let (capture_tx, capture_rx) = mpsc::sync_channel(1);
    // Deliberately not joined: a daemon that ignores the configured upstream
    // never connects, and this thread would then block `accept` forever.
    thread::spawn(move || {
        let Ok((mut stream, _)) = upstream.accept() else {
            return;
        };
        let request = read_http_request(&mut stream);
        let _ = capture_tx.send(request);
        let body = r#"{"id":"msg_1","type":"message","role":"assistant","model":"claude-test","content":[{"type":"text","text":"synthetic-safe"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
    });

    let home = tempdir().unwrap();
    let _guard = DaemonGuard {
        home: home.path().to_path_buf(),
    };
    let proxy_addr = unused_local_addr();
    let upstream_url = format!("http://{upstream_addr}");
    let start = gaze_with_home(home.path())
        .args([
            "proxy",
            "start",
            "--bind",
            &proxy_addr.to_string(),
            "--upstream-anthropic",
            &upstream_url,
            "--policy",
            policy.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "proxy start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );
    assert!(
        find_under(home.path(), "proxy.pid").is_some(),
        "daemon state escaped the redirected HOME"
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if TcpStream::connect(proxy_addr).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "daemonized proxy never bound {proxy_addr}{}",
            daemon_log_tail(home.path())
        );
        thread::sleep(Duration::from_millis(20));
    }

    let request_body = json!({
        "model": "claude-test",
        "max_tokens": 32,
        "messages": [{"role": "user", "content": PARITY_INPUT}],
        "stream": false
    })
    .to_string();
    let mut stream = TcpStream::connect(proxy_addr).unwrap();
    write!(
        stream,
        "POST /v1/messages HTTP/1.1\r\nhost: {proxy_addr}\r\nx-api-key: synthetic-test-key\r\nanthropic-version: 2023-06-01\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        request_body.len(),
        request_body
    )
    .unwrap();
    // A daemon that dropped `--upstream-anthropic` reaches for the real API, so
    // this read must time out rather than hang the suite.
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);

    let captured = capture_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_else(|_| {
            panic!(
                "the loopback upstream captured no request: the daemonized proxy ignored the \
                 configured --upstream-anthropic{}",
                daemon_log_tail(home.path())
            )
        });
    assert!(
        response.starts_with(b"HTTP/1.1 200"),
        "proxy response was not 200: {}",
        String::from_utf8_lossy(&response)
    );
    let body_start = captured
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap()
        + 4;
    String::from_utf8(captured[body_start..].to_vec()).unwrap()
}

/// Token classes `gaze clean` produces under `policy` — the reference every
/// other verb has to match.
fn clean_classes(policy: &Path) -> BTreeSet<&'static str> {
    let mut clean = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args(["clean", "--policy", policy.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    clean
        .stdin
        .as_mut()
        .unwrap()
        .write_all(PARITY_INPUT.as_bytes())
        .unwrap();
    drop(clean.stdin.take());
    let clean = clean.wait_with_output().unwrap();
    assert!(
        clean.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&clean.stderr)
    );
    let clean: Value = serde_json::from_slice(&clean.stdout).unwrap();
    parity_classes(clean["clean_text"].as_str().unwrap())
}

#[test]
#[file_serial(gaze_subprocess)]
fn daemonized_proxy_start_honours_policy_recognizers_and_dictionaries() {
    let (_dir, policy) = write_cross_verb_parity_policy();
    let clean = clean_classes(&policy);
    assert_eq!(
        clean,
        BTreeSet::from(["Custom:catalog_title", "Custom:es_test_id"]),
        "reference verb stopped tokenizing the policy classes"
    );

    let proxy = daemonized_proxy_body(&policy);

    assert_eq!(
        parity_classes(&proxy),
        clean,
        "`gaze proxy start` must tokenize the same classes as `gaze clean` under one policy: {proxy}"
    );
    assert!(
        !proxy.contains("ES-TEST-123456"),
        "raw policy-detected id reached the upstream: {proxy}"
    );
    assert!(
        !proxy.contains("Sonnenlied"),
        "raw dictionary term reached the upstream: {proxy}"
    );
}

/// The deterministic half of the same contract: if the policy the adopter named
/// cannot be loaded, the daemon must refuse to come up. A `start` that reports
/// success here is a chokepoint serving with the bundled `core` pipeline while
/// the adopter believes their policy is enforced.
#[test]
#[file_serial(gaze_subprocess)]
fn daemonized_proxy_start_fails_closed_when_the_policy_cannot_be_loaded() {
    let home = tempdir().unwrap();
    let _guard = DaemonGuard {
        home: home.path().to_path_buf(),
    };
    let bind = unused_local_addr();
    let missing = home.path().join("absent-policy.toml");

    let start = gaze_with_home(home.path())
        .args([
            "proxy",
            "start",
            "--bind",
            &bind.to_string(),
            "--policy",
            missing.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        !start.status.success(),
        "`proxy start` reported success with an unloadable policy: stdout={} stderr={}",
        String::from_utf8_lossy(&start.stdout),
        String::from_utf8_lossy(&start.stderr)
    );
    assert!(
        TcpStream::connect(bind).is_err(),
        "a policy-less daemon is serving at {bind}{}",
        daemon_log_tail(home.path())
    );
    assert!(
        find_under(home.path(), "proxy.pid").is_none(),
        "a failed start must not leave the pidfile that bricks the next one"
    );
}

// ---------------------------------------------------------------------------
// solo todo #3004: `gaze daemon` can run the locale-aware safety-net registry.
//
// Before #3004 `daemon.rs::clean_options` hardcoded `safety_net_registry: false`
// and `safety_net_add: &[]`, and clap rejected the flags outright, so under an
// identical policy the daemon chokepoint could only ever run a *single*
// safety-net backend. `gaze::Policy` has no safety-net section, so there was no
// second route to the stronger configuration either.
//
// These tests assert behaviour, not parsing: a flag that were accepted and then
// ignored would be strictly worse than the honest rejection it replaces.
// ---------------------------------------------------------------------------

/// The Pass-3 safety-net subprocess budget, mirroring `safety_net_cli.rs`.
fn safety_net_timeout_ms() -> String {
    let seconds = std::env::var("GAZE_TEST_SUBPROCESS_TIMEOUT_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("test subprocess timeout must be an integer")
        })
        .unwrap_or(60);
    assert!(seconds > 0, "test subprocess timeout must be positive");
    seconds.saturating_mul(1_000).to_string()
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}

/// A policy that tokenizes nothing in the fixtures below, so the daemon's
/// `clean_text` is byte-identical to the request text and any difference is
/// attributable to the Pass-3 safety net rather than to Pass-1 detection.
fn write_preserve_default_policy() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 86400

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();
    (dir, path)
}

/// Drives one JSONL request through a `gaze daemon` started with `extra_args`
/// and returns the single parsed response line.
fn daemon_request(policy: &Path, extra_args: &[String], text: &str) -> (Value, Output) {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args(["daemon", "--policy", policy.to_str().unwrap()])
        .args(extra_args)
        .args(["--idle-timeout", "30"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        let request = json!({ "session_id": "session-3004", "text": text });
        writeln!(stdin, "{request}").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .next()
        .unwrap_or_else(|| {
            panic!(
                "daemon produced no response line; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        })
        .to_string();
    let value: Value = serde_json::from_str(&line)
        .unwrap_or_else(|err| panic!("daemon response is not JSON ({err}): {line}"));
    (value, output)
}

/// An `opf` stand-in that reports nothing: a backend with no coverage for the
/// text it is handed.
fn write_blind_opf(dir: &Path) -> PathBuf {
    let path = dir.join("blind-opf");
    write_executable(&path, "#!/bin/sh\ncat >/dev/null\nprintf '[]\\n'\n");
    path
}

fn opf_checkpoint(dir: &Path) -> PathBuf {
    let path = dir.join("opf-checkpoint");
    fs::create_dir(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    path
}

/// Both registry entries are live inside one daemon process, and the daemon
/// dispatches between them by locale.
///
/// The en-US request reaches the OPF entry (which answers, so the request
/// succeeds); the de-DE request reaches the Kiji entry (whose model is a
/// placeholder, so it fails closed). One backend could not produce both
/// outcomes, which is what makes this a multi-backend proof rather than a
/// parsing one.
#[cfg(all(feature = "safety-net-openai", feature = "safety-net-kiji"))]
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_safety_net_registry_dispatches_between_two_backends_by_locale() {
    let dir = tempdir().unwrap();
    let opf = write_blind_opf(dir.path());
    let checkpoint = opf_checkpoint(dir.path());
    let kiji = dir.path().join("kiji");
    write_executable(&kiji, "#!/bin/sh\nexit 91\n");
    let model_dir = dir.path().join("kiji-distilbert");
    fs::create_dir(&model_dir).unwrap();
    for artifact in ["SHA256SUMS", "labels.json", "model.onnx", "tokenizer.json"] {
        fs::write(model_dir.join(artifact), b"placeholder").unwrap();
    }
    let (_policy_dir, policy) = write_preserve_default_policy();

    let registry_args = |locale: &str| -> Vec<String> {
        vec![
            format!("--locale={locale}"),
            "--safety-net-registry".to_string(),
            "--safety-net-add=openai-filter".to_string(),
            "--safety-net-add=kiji-distilbert".to_string(),
            format!("--safety-net-timeout-ms={}", safety_net_timeout_ms()),
            format!("--opf-command={}", opf.display()),
            format!("--opf-checkpoint={}", checkpoint.display()),
            "--opf-locales=en-US,en-GB".to_string(),
            format!("--kiji-distilbert-command={}", kiji.display()),
            format!("--kiji-distilbert-model-dir={}", model_dir.display()),
            "--kiji-distilbert-locales=de-DE,de-AT".to_string(),
        ]
    };

    let (english, output) = daemon_request(&policy, &registry_args("en-US"), "hello there");
    assert_eq!(
        english["clean_text"],
        "hello there",
        "en-US must route to the OPF entry and answer: response={english}, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        english.get("error").is_none(),
        "en-US must not fail closed: {english}"
    );

    let (german, _output) = daemon_request(&policy, &registry_args("de-DE"), "hallo zusammen");
    assert_eq!(
        german["error"], "ModelUnavailable",
        "de-DE must route to the Kiji entry and fail closed on its placeholder model: {german}"
    );
    assert!(
        german.get("clean_text").is_none(),
        "a failed-closed request must not answer with text: {german}"
    );
}

/// The axis-1 contrast: same daemon, same policy, same document, same `opf`
/// binary — the only difference is the capability #3004 adds.
///
/// A single backend has no locale gate: it scans every locale with one model,
/// reports nothing for the locale it does not cover, and the document ships.
/// The registry resolves by locale and refuses to hand on a document no
/// registered backend covers.
#[cfg(feature = "safety-net-openai")]
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_single_backend_ships_what_the_registry_refuses() {
    let dir = tempdir().unwrap();
    let opf = write_blind_opf(dir.path());
    let checkpoint = opf_checkpoint(dir.path());
    let (_policy_dir, policy) = write_preserve_default_policy();
    let german = "Rueckfragen an Hanna Weber";

    // What `gaze daemon` could do before #3004: one backend, no locale gate.
    // `--safety-net-backend` only selects; `--safety-net` is what activates.
    let single_backend = vec![
        "--locale=de-DE".to_string(),
        "--safety-net=openai-filter".to_string(),
        "--safety-net-backend=openai-filter".to_string(),
        format!("--safety-net-timeout-ms={}", safety_net_timeout_ms()),
        format!("--openai-filter-command={}", opf.display()),
        format!("--openai-filter-checkpoint={}", checkpoint.display()),
    ];
    let (shipped, output) = daemon_request(&policy, &single_backend, german);
    assert_eq!(
        shipped["clean_text"],
        german,
        "the single-backend daemon ships the de-DE document unflagged: response={shipped}, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    // What #3004 makes reachable: locale-keyed dispatch that fails closed.
    let registry = vec![
        "--locale=de-DE".to_string(),
        "--safety-net-registry".to_string(),
        "--safety-net-add=openai-filter".to_string(),
        format!("--safety-net-timeout-ms={}", safety_net_timeout_ms()),
        format!("--opf-command={}", opf.display()),
        format!("--opf-checkpoint={}", checkpoint.display()),
        "--opf-locales=en-US".to_string(),
    ];
    let (refused, _output) = daemon_request(&policy, &registry, german);
    assert_eq!(
        refused["error"], "Unavailable",
        "the registry must refuse a locale no registered backend covers: {refused}"
    );
    assert!(
        refused.get("clean_text").is_none(),
        "a refused request must not answer with text: {refused}"
    );
}

/// The registry's findings reach the daemon's enforcement path.
///
/// Under the default `resolve` mode a residual-PII suspect is substituted out
/// of the answer. Without the registry flags the identical daemon returns the
/// same name verbatim, so this pins that the flags are acted on rather than
/// accepted and dropped.
#[cfg(feature = "safety-net-openai")]
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_safety_net_registry_finding_is_enforced_not_merely_parsed() {
    let dir = tempdir().unwrap();
    let checkpoint = opf_checkpoint(dir.path());
    let text = "Freundliche Gruesse Hanna Weber";
    let name = "Hanna Weber";
    let start = text.find(name).expect("fixture contains the name");
    let end = start + name.len();
    let opf = dir.path().join("reporting-opf");
    write_executable(
        &opf,
        &format!(
            "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' \
             '[{{\"label\":\"private_person\",\"start\":{start},\"end\":{end},\"score\":0.99}}]'\n"
        ),
    );
    let (_policy_dir, policy) = write_preserve_default_policy();

    let registry = vec![
        "--locale=de-DE".to_string(),
        "--safety-net-registry".to_string(),
        "--safety-net-add=openai-filter".to_string(),
        format!("--safety-net-timeout-ms={}", safety_net_timeout_ms()),
        format!("--opf-command={}", opf.display()),
        format!("--opf-checkpoint={}", checkpoint.display()),
        "--opf-locales=de-DE".to_string(),
    ];
    let (caught, output) = daemon_request(&policy, &registry, text);
    let clean_text = caught["clean_text"].as_str().unwrap_or_else(|| {
        panic!(
            "registry run must answer; response={caught}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(
        !clean_text.contains(name),
        "the registry reported the residual name, so it must not survive into the answer: {clean_text}"
    );

    // Same daemon, same document, registry flags withheld.
    let (missed, output) = daemon_request(&policy, &["--locale=de-DE".to_string()], text);
    assert_eq!(
        missed["clean_text"],
        text,
        "without the registry the identical daemon ships the name verbatim: response={missed}, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The rulepack overrides #3004 added actually reach the daemon's detection
/// surface: the plumbing already ran through `clean_overrides_from_options`,
/// only the flags were missing, so the hardcoded empty override silently pinned
/// the daemon to whatever the policy declared.
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_rulepack_override_changes_the_detection_surface() {
    let (_dir, policy) = write_policy_with_es_locale_gated_path_rulepack();
    let text = "id ES-TEST-123456";

    // Baseline: the policy's path rulepack tokenizes the ES id.
    let (baseline, output) = daemon_request(&policy, &["--locale=es-ES".to_string()], text);
    let baseline_text = baseline["clean_text"].as_str().unwrap_or_else(|| {
        panic!(
            "baseline run must answer; response={baseline}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(
        baseline_text.contains("Custom:es_test_id_"),
        "the policy's path rulepack must tokenize the ES id: {baseline_text}"
    );

    // `--rulepack-path` replaces `policy.rulepacks.paths` wholesale, so pointing
    // it at a rulepack without the ES recognizer removes that class from the
    // detection surface. Before #3004 the daemon had no way to express this.
    let override_dir = tempdir().unwrap();
    let unrelated = override_dir.path().join("unrelated.toml");
    fs::write(
        &unrelated,
        concat!(
            "schema_version = \"0.1.0\"\n",
            "rulepack_id = \"unrelated\"\n",
            "rulepack_version = \"0.1.0\"\n",
            "default_locales = [\"global\"]\n",
            "\n",
            "[[recognizers]]\n",
            "id = \"unrelated.marker\"\n",
            "class = \"custom:unrelated_marker\"\n",
            "enabled = true\n",
            "locales = [\"global\"]\n",
            "\n",
            "[recognizers.match]\n",
            "kind = \"regex\"\n",
            "pattern = \"ZZ-UNRELATED-[0-9]{4}\"\n",
        ),
    )
    .unwrap();

    let (overridden, output) = daemon_request(
        &policy,
        &[
            "--locale=es-ES".to_string(),
            format!("--rulepack-path={}", unrelated.display()),
        ],
        text,
    );
    let overridden_text = overridden["clean_text"].as_str().unwrap_or_else(|| {
        panic!(
            "override run must answer; response={overridden}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(
        overridden_text, text,
        "--rulepack-path replaced the policy's rulepack, so the ES id is no longer tokenized: {overridden_text}"
    );
}

// ---------------------------------------------------------------------------
// Eviction audit-write failure surfacing.
//
// `Daemon::log_eviction` used `let _ = self.logger.log_eviction(...)`, so a
// SQLite audit-write failure during background eviction was silently dropped
// with no signal on any channel: stderr was empty, stdout emitted nothing
// (eviction has no response), and `tracing::warn!` is a runtime no-op because
// the CLI installs no tracing subscriber. The fix surfaces the error via
// `eprintln!` (the same channel as the panic hook and `CliError::emit_stderr`)
// with sanitized JSON.
//
// To induce the `RedactionLogError::Sqlite` the tests below set the immutable
// flag (`chattr +i`) on the audit DB's *parent directory* after the first
// successful row is written. `SqliteLogger` uses SQLite's default rollback
// journal mode, so each write transaction must create a `<db>-journal` file in
// that directory. An immutable directory blocks that creation — even for root
// — so the daemon's next INSERT fails with `SQLITE_CANTOPEN`. The existing rows
// and the table itself stay intact, so the tests can still query the DB to
// verify the row was *not* written. If `chattr` is unavailable or unsupported
// on the filesystem, the fallback drops `redaction_log` via a separate
// connection, which makes the daemon's next `INSERT INTO redaction_log` fail
// with `SQLITE_ERROR`/no-such-table.
// ---------------------------------------------------------------------------

/// How the audit DB was broken. Determined at runtime by [`break_audit_db`].
enum AuditBreak {
    /// `chattr +i` was set on the parent directory. The guard removes it.
    Chattr(PathBuf),
    /// `redaction_log` was dropped via a separate connection. No undo needed.
    Dropped,
}

impl Drop for AuditBreak {
    fn drop(&mut self) {
        if let AuditBreak::Chattr(dir) = self {
            let _ = Command::new("chattr").arg("-i").arg(dir).output();
        }
    }
}

/// Makes the daemon's next audit write fail with `RedactionLogError::Sqlite`.
///
/// Tries `chattr +i` on the parent directory first (works even when the test
/// runs as root, and preserves the existing table/rows for post-test
/// queries). Falls back to `DROP TABLE redaction_log` via a separate SQLite
/// connection if `chattr` is unavailable or the filesystem rejects the
/// immutable flag.
fn break_audit_db(audit_db: &Path, audit_dir: &Path) -> AuditBreak {
    let chattr_ok = Command::new("chattr")
        .arg("+i")
        .arg(audit_dir)
        .output()
        .is_ok_and(|o| o.status.success());
    if chattr_ok {
        return AuditBreak::Chattr(audit_dir.to_path_buf());
    }
    let conn = rusqlite::Connection::open(audit_db).expect("open audit db for DROP TABLE fallback");
    conn.execute_batch("DROP TABLE IF EXISTS redaction_log;")
        .expect("DROP TABLE redaction_log fallback");
    AuditBreak::Dropped
}

/// Counts `daemon.session_eviction` rows in `redaction_log`, returning `0`
/// when the table itself is gone (DROP TABLE fallback) rather than panicking.
fn eviction_row_count(audit_db: &Path) -> i64 {
    let conn = rusqlite::Connection::open(audit_db).expect("open audit db for post-eviction query");
    match conn.query_row(
        "SELECT count(*) FROM redaction_log WHERE source = 'daemon.session_eviction'",
        [],
        |row| row.get(0),
    ) {
        Ok(count) => count,
        Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("no such table") => 0,
        Err(err) => panic!("unexpected error querying audit db: {err}"),
    }
}

/// Polls `redaction_log` until at least `min_count` rows are present or the
/// deadline is exceeded.  Returns the final row count.  This replaces a fixed
/// `thread::sleep` before breaking the audit DB so the test is not sensitive
/// to how long the daemon needs to commit its SQLite write.
fn wait_for_redaction_rows(audit_db: &Path, min_count: i64, timeout: Duration) -> i64 {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(conn) = rusqlite::Connection::open(audit_db) {
            if let Ok(count) = conn.query_row("SELECT count(*) FROM redaction_log", [], |row| {
                row.get::<_, i64>(0)
            }) {
                if count >= min_count {
                    return count;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {min_count} row(s) in redaction_log"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Spawns `gaze daemon` and returns the child plus background stdout/stderr
/// reader threads. The threads collect all lines so the test can keep stdin
/// open (to let the session age out for idle eviction) and close it when
/// ready. The child is wrapped in `ChildGuard` so a panicked test does not
/// leak a daemon process.
fn spawn_daemon_collect(
    args: &[&str],
) -> (
    ChildGuard,
    thread::JoinHandle<Vec<String>>,
    thread::JoinHandle<Vec<String>>,
) {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let stdout_thread = thread::spawn(move || {
        std::io::BufReader::new(stdout)
            .lines()
            .map(|l| l.unwrap())
            .collect::<Vec<_>>()
    });
    let stderr_thread = thread::spawn(move || {
        std::io::BufReader::new(stderr)
            .lines()
            .map(|l| l.unwrap())
            .collect::<Vec<_>>()
    });
    (ChildGuard(child), stdout_thread, stderr_thread)
}

/// T1 — Idle-eviction audit-write failure surfaces on stderr.
#[cfg(unix)]
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_idle_eviction_audit_failure_surfaces_on_stderr() {
    let (_policy_dir, policy) = write_policy();
    let audit_dir = tempdir().unwrap();
    let audit_db = audit_dir.path().join("audit.db");

    let (mut guard, stdout_thread, stderr_thread) = spawn_daemon_collect(&[
        "daemon",
        "--policy",
        policy.to_str().unwrap(),
        "--audit-db",
        audit_db.to_str().unwrap(),
        "--session-idle-timeout",
        "1",
        "--idle-timeout",
        "30",
    ]);

    // One request — audit write succeeds, redaction row appears.
    writeln!(
        guard.0.stdin.as_mut().unwrap(),
        "{}",
        json!({"session_id":"sess1","text":"alice@example.invalid"})
    )
    .unwrap();
    guard.0.stdin.as_mut().unwrap().flush().unwrap();

    // Poll until the audit row lands instead of sleeping a fixed interval;
    // this prevents the test from being sensitive to daemon startup latency.
    wait_for_redaction_rows(&audit_db, 1, Duration::from_secs(5));

    // Break the audit DB so the next SQLite write fails with
    // `RedactionLogError::Sqlite`.
    let _breaker = break_audit_db(&audit_db, audit_dir.path());

    // Wait for idle eviction (session-idle-timeout is 1s).
    thread::sleep(Duration::from_millis(2000));

    drop(guard.0.stdin.take());
    let status = guard.0.wait().unwrap();
    let stdout_lines = stdout_thread.join().unwrap();
    let stderr_lines = stderr_thread.join().unwrap();
    let stderr_text = stderr_lines.join("\n");

    assert!(
        status.success(),
        "daemon should exit cleanly after eviction, stderr={stderr_text}"
    );
    assert!(
        stderr_text.contains(r#""error":"AuditWriteFailed""#),
        "stderr must surface AuditWriteFailed: {stderr_text}"
    );
    assert!(
        stderr_text.contains(r#""reason":"idle_timeout""#),
        "stderr must show idle_timeout reason: {stderr_text}"
    );
    // session_id in the error line is the generated audit UUID, not the raw caller ID.
    let failure_line = stderr_lines
        .iter()
        .find(|line| line.contains("AuditWriteFailed"))
        .expect("AuditWriteFailed line must be present");
    let parsed: serde_json::Value = serde_json::from_str(failure_line)
        .expect("AuditWriteFailed line must be valid JSON");
    let audit_id = parsed["session_id"].as_str().expect("session_id must be a string");
    assert_eq!(audit_id.len(), 36, "session_id must be a UUID: {audit_id}");
    assert_ne!(audit_id, "sess1", "raw caller ID must not appear in stderr");
    assert!(
        stderr_text.contains(r#""detail""#),
        "stderr must carry a detail field: {stderr_text}"
    );
    // detail must be a safe error code, not an arbitrary error message.
    let detail = parsed["detail"].as_str().expect("detail must be a string");
    assert!(
        detail == "Sqlite" || detail == "Backend",
        "detail must be a safe error code, got: {detail}"
    );
    // Raw caller ID must not appear in stderr.
    assert!(
        !stderr_text.contains("sess1"),
        "raw caller session ID must not appear in stderr: {stderr_text}"
    );
    // Eviction emits no JSONL response.
    assert_eq!(
        stdout_lines.len(),
        1,
        "stdout should be exactly the one clean response, got: {stdout_lines:?}"
    );
    // The eviction audit row is still lost (log_eviction cannot fail-closed),
    // but the loss is now detectable via stderr.
    assert_eq!(
        eviction_row_count(&audit_db),
        0,
        "eviction audit row should be absent (write failed)"
    );
}

/// T2 — LRU-eviction audit-write failure surfaces on stderr.
#[cfg(unix)]
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_lru_eviction_audit_failure_surfaces_on_stderr() {
    let (_policy_dir, policy) = write_policy();
    let audit_dir = tempdir().unwrap();
    let audit_db = audit_dir.path().join("audit.db");

    let (mut guard, stdout_thread, stderr_thread) = spawn_daemon_collect(&[
        "daemon",
        "--policy",
        policy.to_str().unwrap(),
        "--audit-db",
        audit_db.to_str().unwrap(),
        "--session-cap",
        "1",
        "--idle-timeout",
        "30",
    ]);

    // First request creates session "a" — audit write succeeds.
    writeln!(
        guard.0.stdin.as_mut().unwrap(),
        "{}",
        json!({"session_id":"a","text":"alice@example.invalid"})
    )
    .unwrap();
    guard.0.stdin.as_mut().unwrap().flush().unwrap();

    // Poll until the audit row lands before breaking the DB.
    // This guards against a timing race where the daemon hasn't yet committed
    // the SQLite write when chattr +i is applied to the directory.
    wait_for_redaction_rows(&audit_db, 1, Duration::from_secs(5));

    // Break the audit DB.
    let _breaker = break_audit_db(&audit_db, audit_dir.path());

    // Second request creates session "b", evicting "a" via LRU. The eviction
    // audit write fails (surfaced on stderr); the pipeline call for "b" also
    // fails (surfaced as a Pipeline error on stdout).
    writeln!(
        guard.0.stdin.as_mut().unwrap(),
        "{}",
        json!({"session_id":"b","text":"alice@example.invalid"})
    )
    .unwrap();
    guard.0.stdin.as_mut().unwrap().flush().unwrap();
    thread::sleep(Duration::from_millis(500));

    drop(guard.0.stdin.take());
    let status = guard.0.wait().unwrap();
    let stdout_lines = stdout_thread.join().unwrap();
    let stderr_lines = stderr_thread.join().unwrap();
    let stderr_text = stderr_lines.join("\n");

    assert!(status.success(), "stderr={stderr_text}");

    // stderr: LRU eviction audit failure for "a".
    assert!(
        stderr_text.contains(r#""error":"AuditWriteFailed""#),
        "stderr must surface AuditWriteFailed: {stderr_text}"
    );
    assert!(
        stderr_text.contains(r#""reason":"lru""#),
        "stderr must show lru reason: {stderr_text}"
    );
    // session_id in the error line is the generated audit UUID, not the raw caller ID.
    let failure_line = stderr_lines
        .iter()
        .find(|line| line.contains("AuditWriteFailed"))
        .expect("AuditWriteFailed line must be present");
    let parsed_lru: serde_json::Value = serde_json::from_str(failure_line)
        .expect("AuditWriteFailed line must be valid JSON");
    let audit_id_lru = parsed_lru["session_id"].as_str().expect("session_id must be a string");
    assert_eq!(audit_id_lru.len(), 36, "session_id must be a UUID: {audit_id_lru}");
    assert_ne!(audit_id_lru, "a", "raw caller ID must not appear in stderr");
    // Raw caller IDs must not appear in stderr.
    assert!(
        !stderr_text.contains(r#""session_id":"a""#),
        "raw caller session ID must not appear in stderr: {stderr_text}"
    );
    // stdout: first response is Clean, second is Pipeline error (audit DB
    // broken, pipeline redaction-log write fails).
    assert_eq!(
        stdout_lines.len(),
        2,
        "stdout should have two responses, got: {stdout_lines:?}"
    );
    let resp0: Value = serde_json::from_str(&stdout_lines[0]).unwrap();
    assert!(
        resp0.get("clean_text").is_some(),
        "first response should be clean: {resp0}"
    );
    let resp1: Value = serde_json::from_str(&stdout_lines[1]).unwrap();
    assert_eq!(
        resp1["error"], "Pipeline",
        "second response should be Pipeline error: {resp1}"
    );
    assert!(
        resp1.get("clean_text").is_none(),
        "Pipeline error must not carry clean_text: {resp1}"
    );
    // No eviction audit row in the DB.
    assert_eq!(
        eviction_row_count(&audit_db),
        0,
        "eviction audit row should be absent (write failed)"
    );
}

/// T3 — Happy-path eviction writes the audit row and emits no error.
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_eviction_writes_audit_row_when_db_healthy() {
    let (_policy_dir, policy) = write_policy();
    let audit_dir = tempdir().unwrap();
    let audit_db = audit_dir.path().join("audit.db");

    let (mut guard, stdout_thread, stderr_thread) = spawn_daemon_collect(&[
        "daemon",
        "--policy",
        policy.to_str().unwrap(),
        "--audit-db",
        audit_db.to_str().unwrap(),
        "--session-idle-timeout",
        "1",
        "--idle-timeout",
        "30",
    ]);

    writeln!(
        guard.0.stdin.as_mut().unwrap(),
        "{}",
        json!({"session_id":"sess1","text":"alice@example.invalid"})
    )
    .unwrap();
    guard.0.stdin.as_mut().unwrap().flush().unwrap();
    thread::sleep(Duration::from_millis(500));

    // Wait for idle eviction (session-idle-timeout is 1s, audit DB is healthy).
    thread::sleep(Duration::from_millis(2000));

    drop(guard.0.stdin.take());
    let status = guard.0.wait().unwrap();
    let stdout_lines = stdout_thread.join().unwrap();
    let stderr_lines = stderr_thread.join().unwrap();
    let stderr_text = stderr_lines.join("\n");

    assert!(status.success(), "stderr={stderr_text}");
    assert!(
        !stderr_text.contains("AuditWriteFailed"),
        "stderr should not contain AuditWriteFailed when DB is healthy: {stderr_text}"
    );
    assert_eq!(
        stdout_lines.len(),
        1,
        "stdout should have exactly one clean response"
    );
    // The eviction audit row should be present with correct metadata.
    let conn = rusqlite::Connection::open(&audit_db).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT source, provenance_stage, class, action, session_id \
             FROM redaction_log WHERE source = 'daemon.session_eviction'",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 1, "exactly one eviction audit row: {rows:?}");
    let (source, stage, class, action, session_id) = &rows[0];
    assert_eq!(source, "daemon.session_eviction");
    assert_eq!(stage.as_deref(), Some("daemon"));
    assert_eq!(class, "custom:daemon_session");
    assert_eq!(action, "preserve");
    assert!(
        session_id.is_some(),
        "eviction row should carry the audit session id"
    );
}

/// T4 — Stderr sanitization: hostile session_id round-trips as valid JSON.
#[cfg(unix)]
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_audit_failure_stderr_survives_hostile_session_id() {
    let (_policy_dir, policy) = write_policy();
    let audit_dir = tempdir().unwrap();
    let audit_db = audit_dir.path().join("audit.db");

    let hostile = r#"a"b\c{d}e"#;

    let (mut guard, stdout_thread, stderr_thread) = spawn_daemon_collect(&[
        "daemon",
        "--policy",
        policy.to_str().unwrap(),
        "--audit-db",
        audit_db.to_str().unwrap(),
        "--session-idle-timeout",
        "1",
        "--idle-timeout",
        "30",
    ]);

    // A request with a session_id containing characters that would break
    // naive {} interpolation: double quote, backslash, braces.
    writeln!(
        guard.0.stdin.as_mut().unwrap(),
        "{}",
        json!({"session_id": hostile, "text": "alice@example.invalid"})
    )
    .unwrap();
    guard.0.stdin.as_mut().unwrap().flush().unwrap();
    thread::sleep(Duration::from_millis(500));

    // Break the audit DB.
    let _breaker = break_audit_db(&audit_db, audit_dir.path());

    // Wait for idle eviction of the hostile-id session.
    thread::sleep(Duration::from_millis(2000));

    drop(guard.0.stdin.take());
    let status = guard.0.wait().unwrap();
    let stdout_lines = stdout_thread.join().unwrap();
    let stderr_lines = stderr_thread.join().unwrap();

    assert!(status.success());
    assert_eq!(stdout_lines.len(), 1, "one clean response only");

    let stderr_text = stderr_lines.join("\n");
    assert!(
        stderr_text.contains("AuditWriteFailed"),
        "stderr should contain AuditWriteFailed: {stderr_text}"
    );
    // The line must be valid JSON. session_id is the generated audit UUID,
    // not the raw caller-supplied hostile ID; hostile characters must not appear.
    let failure_line = stderr_lines
        .iter()
        .find(|line| line.contains("AuditWriteFailed"))
        .unwrap();
    let parsed: Value = serde_json::from_str(failure_line).unwrap_or_else(|err| {
        panic!("AuditWriteFailed line must be valid JSON ({err}): {failure_line}")
    });
    let audit_id_t4 = parsed["session_id"].as_str().expect("session_id must be a string");
    assert_eq!(audit_id_t4.len(), 36, "session_id must be a UUID: {audit_id_t4}");
    // The hostile raw caller ID must not appear in any form in stderr.
    assert!(
        !stderr_text.contains(r#"a"b"#),
        "hostile caller ID must not appear in stderr: {stderr_text}"
    );
    assert_eq!(parsed["reason"], "idle_timeout");
    // detail is a safe error code, not an arbitrary error message.
    let detail_t4 = parsed["detail"].as_str().expect("detail must be a string");
    assert!(
        detail_t4 == "Sqlite" || detail_t4 == "Backend",
        "detail must be a safe error code, got: {detail_t4}"
    );
}

/// T5 — No-audit-db path does not emit AuditWriteFailed.
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_eviction_without_audit_db_produces_no_error() {
    let (_policy_dir, policy) = write_policy();

    let (mut guard, stdout_thread, stderr_thread) = spawn_daemon_collect(&[
        "daemon",
        "--policy",
        policy.to_str().unwrap(),
        "--session-idle-timeout",
        "1",
        "--idle-timeout",
        "30",
    ]);

    writeln!(
        guard.0.stdin.as_mut().unwrap(),
        "{}",
        json!({"session_id":"sess1","text":"alice@example.invalid"})
    )
    .unwrap();
    guard.0.stdin.as_mut().unwrap().flush().unwrap();
    thread::sleep(Duration::from_millis(500));

    // Wait for idle eviction — no audit DB, so log_eviction's Ok(()) guard
    // means no error and no AuditWriteFailed stderr line.
    thread::sleep(Duration::from_millis(2000));

    drop(guard.0.stdin.take());
    let status = guard.0.wait().unwrap();
    let stdout_lines = stdout_thread.join().unwrap();
    let stderr_lines = stderr_thread.join().unwrap();
    let stderr_text = stderr_lines.join("\n");

    assert!(status.success(), "stderr={stderr_text}");
    assert!(
        stderr_text.is_empty(),
        "stderr should be empty without --audit-db: {stderr_text}"
    );
    assert_eq!(
        stdout_lines.len(),
        1,
        "stdout should have exactly one clean response"
    );
}

/// T6 — Request-path audit failure surfaces on stdout (not stderr).
///
/// `clean_request` maps the RedactionLogError to a typed `Pipeline` error on
/// the JSONL stdout stream. The eviction `AuditWriteFailed` line uses stderr.
/// This test pins that the two surfacing channels remain distinct and that
/// only the eviction path changed.
#[cfg(unix)]
#[test]
#[file_serial(gaze_subprocess)]
fn daemon_request_path_audit_failure_surfaces_on_stdout_not_stderr() {
    let (_policy_dir, policy) = write_policy();
    let audit_dir = tempdir().unwrap();
    let audit_db = audit_dir.path().join("audit.db");

    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "daemon",
            "--policy",
            policy.to_str().unwrap(),
            "--audit-db",
            audit_db.to_str().unwrap(),
            "--idle-timeout",
            "30",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // First request succeeds (audit DB is writable).
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            json!({"session_id":"sess1","text":"alice@example.invalid"})
        )
        .unwrap();
        stdin.flush().unwrap();
    }

    // Poll until the first request's audit row has landed before breaking the
    // DB, so we don't race with the daemon's SQLite write.
    wait_for_redaction_rows(&audit_db, 1, Duration::from_secs(5));

    // Break the audit DB.
    let _breaker = break_audit_db(&audit_db, audit_dir.path());

    // Second request — the pipeline's redaction-log write fails, surfaced as a
    // typed Pipeline error on stdout (not AuditWriteFailed on stderr).
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            json!({"session_id":"sess2","text":"alice@example.invalid"})
        )
        .unwrap();
        stdin.flush().unwrap();
    }
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let responses: Vec<Value> = stdout
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(responses.len(), 2, "two responses: {stdout}");
    assert!(
        responses[0].get("clean_text").is_some(),
        "first response should be clean: {}",
        responses[0]
    );
    assert_eq!(
        responses[1]["error"], "Pipeline",
        "second response should be Pipeline error: {}",
        responses[1]
    );
    assert!(
        responses[1].get("clean_text").is_none(),
        "Pipeline error must not carry clean_text: {}",
        responses[1]
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("AuditWriteFailed"),
        "no eviction occurred, so no AuditWriteFailed on stderr: {stderr}"
    );
}
