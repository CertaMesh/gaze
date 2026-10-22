use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};
use tempfile::tempdir;

struct Daemon {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Daemon {
    fn start(policy: &std::path::Path) -> Self {
        let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
            .args(["daemon", "--policy", policy.to_str().unwrap()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn request(&mut self, value: Value) -> Value {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        assert!(self.stdout.read_line(&mut line).unwrap() > 0);
        serde_json::from_str(&line).unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn policy() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
schema_version = "0.1.0"
[session]
scope = "conversation"
[policy.rulepacks]
bundled = ["core"]
[[policy.custom_recognizers]]
kind = "regex"
name = "synthetic_email"
pattern = '[A-Za-z0-9._%+-]+@example[.]invalid'
class = "email"
safety_tier = "safe_default"
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
fn versioned_session_round_trips_two_calls_compaction_and_tool_json() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    assert_eq!(
        daemon.request(
            json!({"version":1,"op":"clean","session_id":"run-a","text":"alice@example.invalid"})
        )["error"],
        "SessionMissing"
    );
    assert_eq!(
        daemon.request(json!({"version":1,"op":"open","session_id":"run-a"}))["op"],
        "open"
    );
    assert_eq!(
        daemon.request(json!({"version":1,"op":"open","session_id":"run-a"}))["error"],
        "SessionExists"
    );
    assert_eq!(
        daemon.request(json!({"version":2,"op":"restore","session_id":"run-a","text":"x"}))
            ["error"],
        "ProtocolVersion"
    );

    let request = json!({"messages":[{"role":"user","content":[{"text":"Email alice@example.invalid"}]}],"tools":[{"arguments":{"recipient":"alice@example.invalid"}}]});
    let first = daemon
        .request(json!({"version":1,"op":"clean","session_id":"run-a","text":request.to_string()}));
    let cleaned: Value = serde_json::from_str(first["clean_text"].as_str().unwrap()).unwrap();
    let token = cleaned["tools"][0]["arguments"]["recipient"]
        .as_str()
        .unwrap();
    assert_ne!(token, "alice@example.invalid");
    assert!(!first["clean_text"]
        .as_str()
        .unwrap()
        .contains("alice@example.invalid"));

    let model = json!({"content":format!("Done for {token}"),"tool_call":{"arguments":{"recipient":token}}});
    let restored = daemon
        .request(json!({"version":1,"op":"restore","session_id":"run-a","text":model.to_string()}));
    let restored_json: Value = serde_json::from_str(restored["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        restored_json["tool_call"]["arguments"]["recipient"],
        "alice@example.invalid"
    );

    let second = daemon
        .request(json!({"version":1,"op":"clean","session_id":"run-a","text":request.to_string()}));
    let second_json: Value = serde_json::from_str(second["clean_text"].as_str().unwrap()).unwrap();
    assert_eq!(second_json["tools"][0]["arguments"]["recipient"], token);

    let compact = daemon.request(json!({"version":1,"op":"clean","session_id":"run-a","text":"Summarize alice@example.invalid"}));
    assert!(compact["clean_text"].as_str().unwrap().contains(token));
    let summary = daemon.request(
        json!({"version":1,"op":"restore","session_id":"run-a","text":format!("Summary: {token}")}),
    );
    assert_eq!(summary["text"], "Summary: alice@example.invalid");

    assert_eq!(
        daemon.request(
            json!({"version":1,"op":"restore","session_id":"run-a","text":"<deadbeef:Email_999>"})
        )["error"],
        "UnknownToken"
    );
    assert_eq!(
        daemon.request(json!({"version":1,"op":"close","session_id":"run-a"}))["op"],
        "close"
    );
    assert_eq!(
        daemon.request(json!({"version":1,"op":"restore","session_id":"run-a","text":token}))
            ["error"],
        "SessionMissing"
    );
}

#[test]
fn interrupted_session_cannot_restore_after_restart() {
    let (_dir, policy) = policy();
    let token = {
        let mut daemon = Daemon::start(&policy);
        daemon.request(json!({"version":1,"op":"open","session_id":"run-a"}));
        let clean = daemon.request(
            json!({"version":1,"op":"clean","session_id":"run-a","text":"alice@example.invalid"}),
        );
        clean["clean_text"].as_str().unwrap().to_owned()
    };
    let mut restarted = Daemon::start(&policy);
    assert_eq!(
        restarted.request(json!({"version":1,"op":"restore","session_id":"run-a","text":token}))
            ["error"],
        "SessionMissing"
    );
}

#[test]
fn restore_rejects_tokens_from_another_live_session_without_partial_output() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    for session_id in ["run-a", "run-b"] {
        assert_eq!(
            daemon.request(json!({"version":1,"op":"open","session_id":session_id}))["op"],
            "open"
        );
    }
    let a = "alice@example.invalid";
    let b = "bob@example.invalid";
    let a_token = daemon.request(json!({"version":1,"op":"clean","session_id":"run-a","text":a}))
        ["clean_text"]
        .as_str()
        .unwrap()
        .to_owned();
    let b_token = daemon.request(json!({"version":1,"op":"clean","session_id":"run-b","text":b}))
        ["clean_text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(a_token, a);
    assert_ne!(b_token, b);
    assert_ne!(a_token, b_token);

    for (session_id, own_token, foreign_token, original) in [
        ("run-a", &a_token, &b_token, a),
        ("run-b", &b_token, &a_token, b),
    ] {
        let own = daemon
            .request(json!({"version":1,"op":"restore","session_id":session_id,"text":own_token}));
        assert_eq!(own["text"], original);
        for text in [
            foreign_token.to_string(),
            format!("{own_token} {foreign_token}"),
        ] {
            let rejected = daemon
                .request(json!({"version":1,"op":"restore","session_id":session_id,"text":text}));
            assert_eq!(rejected["version"], 1);
            assert_eq!(rejected["error"], "UnknownToken");
            assert!(
                rejected.get("text").is_none(),
                "restore returned output: {rejected}"
            );
        }
    }
}

#[test]
fn invalid_v1_request_shapes_return_versioned_protocol_errors() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    for request in [
        json!({"version":1,"op":"restore","session_id":"","text":"x"}),
        json!({"version":1,"op":"restore","session_id":"run-a","text":42}),
        json!({"version":1,"op":42,"session_id":"run-a","text":"x"}),
    ] {
        let response = daemon.request(request);
        assert_eq!(response["version"], 1);
        assert!(response["session_id"].is_null());
        assert_eq!(response["error"], "ProtocolInvalid");
        assert!(response.get("text").is_none());
    }
}

#[test]
fn rejected_requests_never_echo_fictional_pii_session_ids() {
    let (dir, policy) = policy();
    let audit_db = dir.path().join("audit.db");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "daemon",
            "--policy",
            policy.to_str().unwrap(),
            "--audit-db",
            audit_db.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let legacy_id = "alice@example.invalid";
    let v1_id = "bob@example.invalid";
    let cases = [
        (
            json!({"session_id":legacy_id,"text":42}),
            None,
            "JsonMalformed",
        ),
        (json!({"session_id":legacy_id}), None, "ProtocolInvalid"),
        (
            json!({"version":1,"op":"restore","session_id":v1_id,"text":42}),
            Some(1),
            "ProtocolInvalid",
        ),
        (
            json!({"version":1,"op":42,"session_id":v1_id,"text":"x"}),
            Some(1),
            "ProtocolInvalid",
        ),
        (
            json!({"version":1,"op":"unknown","session_id":v1_id}),
            Some(1),
            "ProtocolInvalid",
        ),
        (
            json!({"version":1,"op":"clean","session_id":v1_id,"text":"x"}),
            Some(1),
            "SessionMissing",
        ),
        (
            json!({"version":1,"op":"restore","session_id":v1_id,"text":"x"}),
            Some(1),
            "SessionMissing",
        ),
        (
            json!({"version":1,"op":"close","session_id":v1_id}),
            Some(1),
            "SessionMissing",
        ),
        (
            json!({"version":2,"op":"restore","session_id":v1_id,"text":"x"}),
            Some(2),
            "ProtocolVersion",
        ),
    ];
    {
        let mut stdin = child.stdin.take().unwrap();
        for (request, _, _) in &cases {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let frames = stdout.lines().collect::<Vec<_>>();
    assert_eq!(frames.len(), cases.len());
    for (frame, (_, version, error)) in frames.iter().zip(cases) {
        let response: Value = serde_json::from_str(frame).unwrap();
        assert_eq!(response["error"], error);
        assert!(response["session_id"].is_null());
        match version {
            Some(version) => assert_eq!(response["version"], version),
            None => assert!(response.get("version").is_none()),
        }
        assert!(!frame.contains(legacy_id) && !frame.contains(v1_id));
        assert!(response.get("text").is_none());
        assert!(response.get("clean_text").is_none());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains(legacy_id) && !stderr.contains(v1_id));
    let audit = rusqlite::Connection::open(&audit_db).unwrap();
    let rows: i64 = audit
        .query_row("SELECT count(*) FROM redaction_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0, "rejected requests must not create audit rows");
    let audit_bytes = fs::read(&audit_db).unwrap();
    for id in [legacy_id, v1_id] {
        assert!(!audit_bytes
            .windows(id.len())
            .any(|window| window == id.as_bytes()));
    }
}
