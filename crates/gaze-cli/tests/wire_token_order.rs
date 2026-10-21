//! Deterministic order of the token lists `gaze daemon` and `gaze clean` emit.
//!
//! Both lists used to come straight out of the session's `HashMap`, so the same
//! input produced a differently ordered `tokens` / `entries` array from one
//! session (or one process) to the next. Every session gets a fresh hash seed,
//! so many sessions in one daemon, plus a few fresh processes, cover the seeds;
//! each response must be byte-identical once the per-session values (the
//! session id and the random session hex inside each token) are normalized.
//! No timing is involved.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use serial_test::file_serial;
use tempfile::tempdir;

/// Enough distinct values that two random orders essentially never agree.
const VALUE_COUNT: usize = 24;
const SESSIONS_PER_PROCESS: usize = 6;
const PROCESSES: usize = 3;

fn write_policy() -> (tempfile::TempDir, PathBuf) {
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
pattern = 'user[0-9]+@example[.]invalid'
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

/// `user1@example.invalid user2@example.invalid ...`: token numbers follow
/// first occurrence, so span order (`Email_2` before `Email_10`) differs from
/// plain string order of the tokens, and the test can tell the two apart.
fn first_text() -> String {
    (1..=VALUE_COUNT)
        .map(|idx| format!("user{idx}@example.invalid"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A follow-up in the same session: one new value, two earlier values out of
/// their original order. The other earlier tokens are not in this document.
fn second_text() -> String {
    format!(
        "user{}@example.invalid then user7@example.invalid and user3@example.invalid",
        VALUE_COUNT + 6
    )
}

fn session_hex_of(token: &str) -> &str {
    token
        .strip_prefix('<')
        .and_then(|rest| rest.get(..8))
        .unwrap_or_else(|| panic!("unexpected token shape: {token}"))
}

/// Replaces the per-session parts of a response line with fixed placeholders.
fn normalize(line: &str, session_id: &str, hex: &str) -> String {
    line.replace(
        &format!("\"session_id\":\"{session_id}\""),
        "\"session_id\":\"S\"",
    )
    .replace(hex, "HEX")
}

/// The documented order: first clean-text span start, tokens absent from this
/// document last; then token, class and family.
fn assert_span_then_token_order(response: &Value) {
    let clean_text = response["clean_text"].as_str().unwrap();
    let mut first_start = BTreeMap::<&str, u64>::new();
    for span in response["manifest"].as_array().unwrap() {
        let start = span["clean_span"]["start"].as_u64().unwrap();
        let end = span["clean_span"]["end"].as_u64().unwrap();
        let text = &clean_text[start as usize..end as usize];
        let slot = first_start.entry(text).or_insert(start);
        *slot = (*slot).min(start);
    }
    let keys = response["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|token| {
            let text = token["token"].as_str().unwrap();
            (
                first_start.get(text).copied().unwrap_or(u64::MAX),
                text.to_string(),
                token["class"].as_str().unwrap().to_string(),
                token["family"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "tokens must be in span-then-token order");
}

fn run_daemon(policy: &Path, requests: &[Value]) -> Vec<String> {
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
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
#[file_serial(gaze_subprocess)]
fn daemon_tokens_are_byte_identical_across_sessions_and_processes() {
    let (_dir, policy) = write_policy();
    let mut firsts = Vec::new();
    let mut seconds = Vec::new();
    for process in 0..PROCESSES {
        let mut requests = Vec::new();
        for session in 0..SESSIONS_PER_PROCESS {
            let session_id = format!("p{process}-s{session}");
            requests.push(json!({"session_id": session_id, "text": first_text()}));
            requests.push(json!({"session_id": session_id, "text": second_text()}));
        }
        let lines = run_daemon(&policy, &requests);
        assert_eq!(lines.len(), requests.len());
        for (request, line) in requests.iter().zip(&lines) {
            let response: Value = serde_json::from_str(line).unwrap();
            let session_id = request["session_id"].as_str().unwrap();
            assert_eq!(response["session_id"], session_id, "{line}");
            let tokens = response["tokens"].as_array().unwrap();
            assert!(tokens.len() >= VALUE_COUNT, "{line}");
            assert_span_then_token_order(&response);
            let hex = session_hex_of(tokens[0]["token"].as_str().unwrap()).to_string();
            let normalized = normalize(line, session_id, &hex);
            if request["text"] == first_text() {
                firsts.push(normalized);
            } else {
                seconds.push(normalized);
            }
        }
    }
    assert_eq!(firsts.len(), PROCESSES * SESSIONS_PER_PROCESS);
    for line in &firsts {
        assert_eq!(line, &firsts[0], "first response differs between runs");
    }
    for line in &seconds {
        assert_eq!(line, &seconds[0], "follow-up response differs between runs");
    }
}

fn run_clean(policy: &Path, text: &str) -> Value {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args(["clean", "--policy", policy.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
#[file_serial(gaze_subprocess)]
fn clean_entries_are_byte_identical_across_processes() {
    let (_dir, policy) = write_policy();
    let mut serialized = Vec::new();
    for _ in 0..(PROCESSES + 3) {
        let response = run_clean(&policy, &first_text());
        let clean_text = response["clean_text"].as_str().unwrap();
        let entries = response["entries"].as_array().unwrap();
        assert_eq!(entries.len(), VALUE_COUNT);
        // `clean` emits no manifest, and its session is fresh, so every token
        // occurs in this document: span order is order of first occurrence.
        let starts = entries
            .iter()
            .map(|entry| clean_text.find(entry["token"].as_str().unwrap()).unwrap())
            .collect::<Vec<_>>();
        assert!(
            starts.windows(2).all(|pair| pair[0] < pair[1]),
            "entries must follow first occurrence in clean_text: {starts:?}"
        );
        let hex = session_hex_of(entries[0]["token"].as_str().unwrap()).to_string();
        serialized.push(
            serde_json::to_string(&response["entries"])
                .unwrap()
                .replace(&hex, "HEX"),
        );
    }
    for line in &serialized {
        assert_eq!(line, &serialized[0], "clean entries differ between runs");
    }
}
