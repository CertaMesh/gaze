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
