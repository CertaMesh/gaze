use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::tempdir;

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

#[test]
fn daemon_processes_jsonl_and_isolates_sessions() {
    let (_dir, policy) = write_policy();
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
        for idx in 0..100 {
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
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(10),
        "100 daemon requests took {elapsed:?}"
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

#[test]
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
