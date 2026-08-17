use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
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
    assert!(responses.iter().all(|value| value["manifest"]
        .as_array()
        .is_some_and(|spans| !spans.is_empty())));

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
