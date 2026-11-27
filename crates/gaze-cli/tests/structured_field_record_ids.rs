//! Whole-document CLI/daemon controls for the proxy's leaf-context known failures.
#[allow(dead_code)]
#[path = "../../gaze-proxy/tests/support/structured_record_ids.rs"]
mod record_ids;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

fn policy() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("ids.toml"), record_ids::RULEPACK).unwrap();
    let policy = dir.path().join("policy.toml");
    fs::write(
        &policy,
        format!(
            r#"
schema_version = "0.1.0"
[session]
scope = "conversation"
[policy.rulepacks]
bundled = ["core"]
paths = [{}]
[[rule]]
kind = "class"
class = "custom:probe_record_id"
action = "tokenize"
[[rule]]
kind = "default"
action = "tokenize"
"#,
            serde_json::to_string(&dir.path().join("ids.toml")).unwrap()
        ),
    )
    .unwrap();
    (dir, policy)
}

struct Daemon {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}
impl Daemon {
    fn start(policy: &std::path::Path) -> Self {
        let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
            .args(["daemon", "--policy"])
            .arg(policy)
            .env("GAZE_DISABLE_OS_KEYSTORE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
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

#[test]
fn daemon_whole_json_protects_labelled_strings_and_refuses_labelled_numbers() {
    let (_dir, policy) = policy();
    let mut daemon = Daemon::start(&policy);
    for case in record_ids::cases() {
        assert!(
            daemon.request(json!({"version":1,"op":"open","session_id":"probe"}))["op"] == "open",
            "daemon open: expected open at /op"
        );
        let source = case.document().to_string();
        let response =
            daemon.request(json!({"version":1,"op":"clean","session_id":"probe","text":source}));
        if case.numeric {
            // A labelled JSON number has no reversible rendering: typed refusal, no output.
            assert_eq!(
                response["error"], "Untransformable",
                "daemon {case:?}: {response}"
            );
            assert_eq!(response["reason"], "json_number_no_text_only_path");
            assert!(response.get("clean_text").is_none());
            assert!(
                !response.to_string().contains("731284"),
                "value-free refusal"
            );
            assert!(
                daemon.request(json!({"version":1,"op":"close","session_id":"probe"}))["op"]
                    == "close",
                "daemon close: expected close at /op"
            );
            continue;
        }
        let clean = response["clean_text"].as_str().unwrap_or_else(|| {
            panic!("daemon/whole-json {case:?}: expected string at /clean_text")
        });
        record_ids::check(&case, "daemon/whole-json", clean, false);
        serde_json::from_str::<Value>(clean).expect("labelled strings stay JSON");
        let restored =
            daemon.request(json!({"version":1,"op":"restore","session_id":"probe","text":clean}));
        assert!(
            restored["text"] == source,
            "daemon/whole-json {case:?}: restore mismatch at /text"
        );
        assert!(
            daemon.request(json!({"version":1,"op":"close","session_id":"probe"}))["op"] == "close",
            "daemon close: expected close at /op"
        );
    }
    let response = daemon.request(json!({"session_id":"unlabelled","text":"{\"count\":731284}"}));
    assert!(
        response["clean_text"] == "{\"count\":731284}",
        "daemon unlabelled control: mismatch at /clean_text"
    );
}

#[test]
fn clean_whole_json_protects_labelled_strings_and_refuses_labelled_numbers() {
    let (_dir, policy) = policy();
    for case in record_ids::cases() {
        let source = case.document().to_string();
        if case.numeric {
            let output = run_clean(&policy, &source);
            assert_eq!(output.status.code(), Some(8), "clean {case:?}: exit 8");
            assert!(
                output.stdout.is_empty(),
                "clean {case:?}: no output on refusal"
            );
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                stderr.contains(r#""reason":"json_number_no_text_only_path""#),
                "clean {case:?}: typed reason expected, got {stderr}"
            );
            assert!(!stderr.contains("731284"), "value-free refusal");
            continue;
        }
        let clean = clean(&policy, &source);
        record_ids::check(&case, "clean/whole-json", &clean, false);
        serde_json::from_str::<Value>(&clean).expect("labelled strings stay JSON");
    }
    assert!(
        clean(&policy, "{\"count\":731284}") == "{\"count\":731284}",
        "clean unlabelled control: mismatch at /clean_text"
    );
}

fn run_clean(policy: &std::path::Path, source: &str) -> std::process::Output {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("gaze"))
        .args(["clean", "--policy"])
        .arg(policy)
        .env("GAZE_DISABLE_OS_KEYSTORE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn clean(policy: &std::path::Path, source: &str) -> String {
    let output = run_clean(policy, source);
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    response["clean_text"].as_str().unwrap().to_owned()
}
