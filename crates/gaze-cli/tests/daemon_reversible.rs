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
        self.request_raw(&value.to_string())
    }

    fn request_raw(&mut self, value: &str) -> Value {
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
kind = "class"
class = "name"
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
        json!({"version":1,"op":"restore","session_id":"run-a"}),
        json!({"version":1,"op":"restore","session_id":"run-a","text":null}),
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
        (json!({"session_id":legacy_id}), None, "JsonMalformed"),
        (
            json!({"session_id":legacy_id,"text":null}),
            None,
            "JsonMalformed",
        ),
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
            None => {
                assert_eq!(
                    *frame,
                    "{\"session_id\":null,\"error\":\"JsonMalformed\",\"detail\":\"malformed JSON line\"}"
                );
            }
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

#[test]
fn context_protects_this_turn_and_matched_values_propagate_without_retaining_unused_values() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    daemon.request(json!({"version":1,"op":"open","session_id":"context-run"}));
    let name = "Zelmora Quentrix";
    let unused = "Velmora Xantrix";
    let text = format!("Handle {name} now.");
    let before =
        daemon.request(json!({"version":1,"op":"clean","session_id":"context-run","text":text}));
    assert_eq!(
        before["clean_text"], text,
        "fixture must evade ordinary rules"
    );
    let protected = daemon.request(json!({"version":1,"op":"clean","session_id":"context-run","text":text,
        "context":{"record":{"full_name":name,"last_name":unused},"fields":{"private_marker":"context-only-marker"}}}));
    let clean = protected["clean_text"].as_str().unwrap();
    assert!(!clean.contains(name), "context value leaked");
    assert_eq!(protected["manifest"].as_array().unwrap().len(), 1);
    assert!(protected.get("context").is_none());
    assert!(protected.get("session_blob").is_none());
    for raw in [name, unused, "private_marker", "context-only-marker"] {
        assert!(!protected.to_string().contains(raw));
    }
    assert_eq!(
        daemon.request(json!({"version":1,"op":"restore","session_id":"context-run","text":clean}))
            ["text"],
        text
    );
    let later =
        daemon.request(json!({"version":1,"op":"clean","session_id":"context-run","text":text}));
    assert_eq!(
        later["clean_text"], clean,
        "caller-asserted rule evidence must propagate"
    );
    let unobserved =
        daemon.request(json!({"version":1,"op":"clean","session_id":"context-run","text":unused}));
    assert_eq!(
        unobserved["clean_text"], unused,
        "unused context must not persist"
    );
    let other = daemon.request(json!({"session_id":"other-run","text":text}));
    assert_eq!(other["clean_text"], text, "context must not cross sessions");
    let legacy = daemon.request(
        json!({"session_id":"legacy-context","text":name,"context":{"record":{"full_name":name}}}),
    );
    assert!(!legacy["clean_text"].as_str().unwrap().contains(name));
}

#[test]
fn invalid_context_fails_closed_without_creating_or_changing_sessions() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    let name = "Zelmora Quentrix";
    for context in [
        json!(null),
        json!(42),
        json!([]),
        json!("private-context-value"),
        json!({"record":{"private_field_name":name}}),
        json!({"record":{"full_name":"X".repeat(257)}}),
        json!({"fields":{"private_field_name":"X".repeat(4 * 1024 * 1024)}}),
    ] {
        let response = daemon.request(json!({"session_id":"invalid-context","text":"alice@example.invalid","context":context}));
        assert_eq!(response["error"], "PolicyConfig");
        assert!(response["session_id"].is_null());
        assert!(response.get("clean_text").is_none());
        assert!(response.get("tokens").is_none());
        for private in [name, "private_field_name", "private-context-value"] {
            assert!(!response.to_string().contains(private));
        }
        assert_eq!(
            daemon.request(json!({"version":1,"op":"open","session_id":"invalid-context"}))["op"],
            "open"
        );
        let unchanged = daemon
            .request(json!({"version":1,"op":"clean","session_id":"invalid-context","text":name}));
        assert_eq!(unchanged["clean_text"], name);
        assert_eq!(unchanged["tokens"], json!([]));
        daemon.request(json!({"version":1,"op":"close","session_id":"invalid-context"}));
    }
    daemon.request(json!({"version":1,"op":"open","session_id":"existing"}));
    let original = daemon.request(
        json!({"version":1,"op":"clean","session_id":"existing","text":"alice@example.invalid"}),
    );
    let rejected = daemon.request(json!({"version":1,"op":"clean","session_id":"existing","text":"bob@example.invalid","context":{"record":{"full_name":42}}}));
    assert_eq!(rejected["error"], "PolicyConfig");
    let after = daemon
        .request(json!({"version":1,"op":"clean","session_id":"existing","text":"ordinary text"}));
    assert_eq!(after["tokens"], original["tokens"]);
}

#[test]
fn context_envelope_never_reaches_protocol_diagnostics_or_audit() {
    let (dir, policy) = policy();
    let audit_db = dir.path().join("context-audit.db");
    let name = "Zelmora Quentrix";
    let marker = "private-envelope-marker";
    let requests = [
        json!({"session_id":"safe-run","text":name,"context":{"record":{"full_name":name},"fields":{"private_field_name":marker}}}),
        json!({"session_id":"safe-run","text":"ignored text","context":{"record":{"private_field_name":marker}}}),
    ];
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args([
            "daemon",
            "--policy",
            policy.to_str().unwrap(),
            "--audit-db",
            audit_db.to_str().unwrap(),
        ])
        .env("RUST_LOG", "trace")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let responses: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 2);
    assert_ne!(responses[0]["clean_text"], name);
    assert_eq!(responses[1]["error"], "PolicyConfig");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let audit = fs::read(&audit_db).unwrap();
    for private in [name, marker, "private_field_name", "full_name"] {
        assert!(!stdout.contains(private));
        assert!(!stderr.contains(private));
        assert!(!audit
            .windows(private.len())
            .any(|bytes| bytes == private.as_bytes()));
    }
    let audit = rusqlite::Connection::open(audit_db).unwrap();
    let rows: i64 = audit
        .query_row("SELECT count(*) FROM redaction_log", [], |row| row.get(0))
        .unwrap();
    assert!(
        rows > 0,
        "successful context protection must retain normal audit metadata"
    );
}

#[test]
fn context_preserves_raw_duplicate_and_encoded_size_checks() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    for raw_context in [
        r#"{"record":{"full_name":"Zelmora Quentrix","full_name":"Velmora Xantrix"}}"#.to_string(),
        format!(
            "{{{}\"record\":{{\"full_name\":\"Zelmora Quentrix\"}}}}",
            " ".repeat(4 * 1024 * 1024)
        ),
    ] {
        let response = daemon.request_raw(&format!(
            r#"{{"session_id":"raw-context","text":"Zelmora Quentrix","context":{raw_context}}}"#
        ));
        assert_eq!(response["error"], "PolicyConfig");
        assert!(response.get("clean_text").is_none());
    }
}

fn clean_with_context(policy: &std::path::Path, raw_context: &str) -> std::process::Output {
    let dir = tempdir().unwrap();
    let context = dir.path().join("context.json");
    fs::write(&context, raw_context).unwrap();
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .arg("clean")
        .arg("--policy")
        .arg(policy)
        .arg("--context-json")
        .arg(context)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Zelmora Quentrix")
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn context_positional_arrays_report_object_migration_without_echoing_values() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    for count in 0..=6 {
        let mut fields = vec!["{}"; count];
        if count >= 4 {
            fields[3] = r#"{"full_name":"Zelmora Quentrix"}"#;
        }
        let context = format!("[{}]", fields.join(","));
        let output = clean_with_context(&policy, &context);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"], "PolicyConfig");
        let detail = error["detail"].as_str().unwrap();
        assert!(detail.contains("use a JSON object"), "{error}");
        assert!(detail.contains(r#"{"record":{"full_name":"..."}}"#));
        assert!(!detail.contains("Zelmora Quentrix"));
        let response = daemon.request_raw(&format!(
            r#"{{"session_id":"array-context","text":"Zelmora Quentrix","context":{context}}}"#
        ));
        assert_eq!(response["error"], error["error"]);
        assert_eq!(response["detail"], error["detail"]);
        assert!(response.get("clean_text").is_none());
    }
    let output = clean_with_context(&policy, r#"{"record":{"full_name":"Zelmora Quentrix"}}"#);
    assert!(output.status.success(), "{:?}", output.stderr);
    let cleaned: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!cleaned["clean_text"]
        .as_str()
        .unwrap()
        .contains("Zelmora Quentrix"));
}

fn nested_fields_context(depth: usize) -> String {
    format!(
        r#"{{"fields":{{"private":{}0{}}}}}"#,
        "[".repeat(depth),
        "]".repeat(depth)
    )
}

#[test]
fn context_depth_accepted_by_clean_is_accepted_by_daemon() {
    let (_dir, policy) = policy();
    let context = nested_fields_context(125);
    let output = clean_with_context(&policy, &context);
    assert!(output.status.success(), "{:?}", output.stderr);
    let mut daemon = Daemon::start(&policy);
    daemon.request(json!({"version":1,"op":"open","session_id":"depth"}));
    for versioned in [true, false] {
        let prefix = if versioned {
            r#""version":1,"op":"clean","#
        } else {
            ""
        };
        let response = daemon.request_raw(&format!(
            r#"{{{prefix}"session_id":"depth","text":"Zelmora Quentrix","context":{context}}}"#
        ));
        assert_eq!(response["clean_text"], "Zelmora Quentrix", "{response}");
        if versioned {
            assert_eq!(response["version"], 1);
        }
    }
}

#[test]
fn context_depth_rejected_by_clean_has_same_versioned_policy_error_in_daemon() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    daemon.request(json!({"version":1,"op":"open","session_id":"depth"}));
    for depth in [126, 127, 256] {
        let context = nested_fields_context(depth);
        let output = clean_with_context(&policy, &context);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"], "PolicyConfig");
        let response = daemon.request_raw(&format!(
            r#"{{"version":1,"op":"clean","session_id":"depth","text":"Zelmora Quentrix","context":{context}}}"#
        ));
        assert_eq!(response["version"], 1, "{response}");
        assert_eq!(response["error"], error["error"]);
        assert_eq!(response["detail"], error["detail"]);
        assert!(response.get("clean_text").is_none());
    }
    let malformed = daemon
        .request_raw(r#"{"version":1,"op":"clean","session_id":"depth","context":{"fields":[}}"#);
    assert_eq!(malformed["error"], "JsonMalformed");
    let response =
        daemon.request(json!({"version":1,"op":"clean","session_id":"depth","text":"ordinary"}));
    assert_eq!(response["clean_text"], "ordinary");
}

#[test]
fn context_is_rejected_on_non_clean_operations() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    daemon.request(json!({"version":1,"op":"open","session_id":"context-ops"}));
    for op in ["open", "restore", "close"] {
        let mut frame = json!({"version":1,"op":op,"session_id":"context-ops","context":{}});
        if op == "restore" {
            frame["text"] = json!("ordinary text");
        }
        assert_eq!(daemon.request(frame)["error"], "ProtocolInvalid");
    }
    assert_eq!(
        daemon.request(json!({"version":1,"op":"close","session_id":"context-ops"}))["op"],
        "close"
    );
}

#[test]
fn context_reuses_clean_dictionary_schema_and_refuses_nonreversible_record_policies() {
    let (dir, policy) = policy();
    let name = "Zelmora Quentrix";
    let mut daemon = Daemon::start(&policy);
    let response = daemon.request(json!({"session_id":"dictionary-context","text":name,
        "context":{"dictionaries":{"owner_names":{"terms":[name],"case_sensitive":true}},
        "class_map":{"owner_names":"name"}}}));
    assert!(!response["clean_text"].as_str().unwrap().contains(name));
    assert_eq!(response["tokens"][0]["class"], "Name");
    let preserve = dir.path().join("preserve.toml");
    fs::write(
        &preserve,
        fs::read_to_string(&policy).unwrap().replace(
            "class = \"name\"\naction = \"tokenize\"",
            "class = \"name\"\naction = \"preserve\"",
        ),
    )
    .unwrap();
    let mut daemon = Daemon::start(&preserve);
    let response = daemon.request(json!({"session_id":"refused-record","text":name,
        "context":{"record":{"full_name":name}}}));
    assert_eq!(response["error"], "PolicyConfig");
    assert!(response.get("clean_text").is_none());
    assert_eq!(
        daemon.request(json!({"version":1,"op":"open","session_id":"refused-record"}))["op"],
        "open"
    );
}

#[test]
fn context_requests_keep_resident_ner_and_its_learned_evidence_tier() {
    let (_dir, policy) = policy();
    fs::write(
        &policy,
        format!(
            "{}\n[ner]\nmodel_dir = \"__gaze_test_fixed_ner\"\n",
            fs::read_to_string(&policy).unwrap()
        ),
    )
    .unwrap();
    let mut daemon = Daemon::start(&policy);
    let first = daemon.request(json!({"session_id":"ner-context","text":"Alice Example"}));
    assert!(!first["clean_text"]
        .as_str()
        .unwrap()
        .contains("Alice Example"));
    let contextual = daemon.request(
        json!({"session_id":"ner-context","text":"Alice Example and Zelmora Quentrix",
        "context":{"record":{"full_name":"Zelmora Quentrix"}}}),
    );
    let cleaned = contextual["clean_text"].as_str().unwrap();
    assert!(!cleaned.contains("Alice Example"));
    assert!(!cleaned.contains("Zelmora Quentrix"));
    let later = daemon
        .request(json!({"session_id":"ner-context","text":"ALICE EXAMPLE and ZELMORA QUENTRIX"}));
    let cleaned = later["clean_text"].as_str().unwrap();
    assert!(
        cleaned.contains("ALICE EXAMPLE"),
        "learned evidence must not propagate"
    );
    assert!(
        !cleaned.contains("ZELMORA QUENTRIX"),
        "caller-known rule evidence must propagate"
    );
}

#[test]
fn padded_record_ids_keep_repeat_protection_on_second_daemon_turn() {
    let dir = tempdir().unwrap();
    let policy = dir.path().join("policy.toml");
    fs::write(&policy, "[session]\nscope = \"conversation\"\n[policy.rulepacks]\nbundled = [\"core\"]\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n").unwrap();
    let mut daemon = Daemon::start(&policy);
    for (index, padding) in [" ".repeat(32), "\u{00a0}".repeat(16), "\\t".repeat(32)]
        .into_iter()
        .enumerate()
    {
        let session = format!("padded-record-{index}");
        let opened = daemon.request(json!({"version":1,"op":"open","session_id":session}));
        assert_eq!(opened["op"], "open", "{opened}");
        let text = format!(r#"{{"customerId":"SYN731284{padding}"}}"#);
        let first =
            daemon.request(json!({"version":1,"op":"clean","session_id":session,"text":text}));
        assert!(
            !first["clean_text"].as_str().unwrap().contains("SYN731284"),
            "{first}"
        );
        let next = daemon.request(json!({"version":1,"op":"clean","session_id":session,"text":"Follow-up about SYN731284"}));
        assert!(
            !next["clean_text"].as_str().unwrap().contains("SYN731284"),
            "{next}"
        );
    }
}

#[test]
fn mixed_record_ids_keep_json_structure_on_second_daemon_turn() {
    let dir = tempdir().unwrap();
    let policy = dir.path().join("policy.toml");
    fs::write(&policy, "[session]\nscope = \"conversation\"\n[policy.rulepacks]\nbundled = [\"core\"]\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n").unwrap();
    let mut daemon = Daemon::start(&policy);
    let mut checked = 0;
    for (index, (seed, text, encoded)) in [
        (
            "customerId: 731284",
            r#"{"731284":true,"total":731284,"copy":"731284"}"#,
            false,
        ),
        (
            "Customer ID = 731284",
            r#"{\"731284\":true,\"total\":731284,\"copy\":\"731284\"}"#,
            true,
        ),
        (
            r#"{"customerId":"SYN\u0037\u0033\u0031\u0032\u0038\u0034"}"#,
            r#"{"copy":"SYN731284"}"#,
            false,
        ),
        (
            r#"{"customerId":"SYN731284"}"#,
            r#"{"copy":"SYN\u0037\u0033\u0031\u0032\u0038\u0034"}"#,
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let session = format!("mixed-record-{index}");
        let opened = daemon.request(json!({"version":1,"op":"open","session_id":session}));
        assert_eq!(opened["op"], "open", "{opened}");
        let first =
            daemon.request(json!({"version":1,"op":"clean","session_id":session,"text":seed}));
        assert!(first["clean_text"].is_string(), "{first}");
        let input = format!("log: {text}");
        let next =
            daemon.request(json!({"version":1,"op":"clean","session_id":session,"text":input}));
        let clean = next["clean_text"].as_str().unwrap();
        let body = clean.strip_prefix("log: ").unwrap();
        let decoded = if encoded {
            serde_json::from_str::<String>(&format!("\"{body}\"")).unwrap()
        } else {
            body.to_owned()
        };
        let parsed: Value = serde_json::from_str(&decoded).unwrap();
        if index < 2 {
            assert_eq!(parsed["731284"], true);
            assert_eq!(parsed["total"], 731284);
            assert_ne!(parsed["copy"], "731284");
        } else {
            assert!(
                !clean.contains("SYN731284") && !clean.contains(r"SYN\u0037"),
                "{clean}"
            );
        }
        let restored =
            daemon.request(json!({"version":1,"op":"restore","session_id":session,"text":clean}));
        assert_eq!(restored["text"], input, "{restored}");
        checked += 1;
    }
    assert_eq!(checked, 4);
}
