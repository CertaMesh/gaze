//! CLI integration tests via assert_cmd.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::json;

fn ghostwriter() -> Command {
    Command::cargo_bin("ghostwriter").expect("binary built")
}

#[test]
fn sanitize_replaces_customer_name() {
    let req = json!({
        "text": "Hi Markus Mueller, please reply",
        "context": { "customer_name": "Markus Mueller" }
    })
    .to_string();

    let assert = ghostwriter()
        .arg("sanitize")
        .write_stdin(req)
        .assert()
        .success();

    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let resp: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(resp["clean_text"], "Hi <CUSTOMER_NAME>, please reply");
    assert!(resp["session_blob"].as_str().unwrap().len() > 0);
    assert_eq!(resp["metadata"]["placeholders"][0], "<CUSTOMER_NAME>");
}

#[test]
fn sanitize_then_restore_roundtrip_via_cli() {
    let req = json!({
        "text": "Markus Mueller wrote from mueller@icloud.com",
        "context": {
            "customer_name": "Markus Mueller",
            "customer_email": "mueller@icloud.com"
        }
    })
    .to_string();

    let assert = ghostwriter()
        .arg("sanitize")
        .write_stdin(req)
        .assert()
        .success();
    let sanitize_out: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let blob = sanitize_out["session_blob"].as_str().unwrap().to_string();

    let draft = "Hello <CUSTOMER_NAME>, we received your note at <CUSTOMER_EMAIL>.";
    let restore_req = json!({ "text": draft, "session_blob": blob }).to_string();

    let assert = ghostwriter()
        .arg("restore")
        .write_stdin(restore_req)
        .assert()
        .success();
    let restore_out: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(
        restore_out["restored_text"],
        "Hello Markus Mueller, we received your note at mueller@icloud.com."
    );
}

#[test]
fn invalid_json_stdin_exits_nonzero() {
    ghostwriter()
        .arg("sanitize")
        .write_stdin("not json at all")
        .assert()
        .failure()
        .stderr(predicate::str::contains("parsing SanitizeRequest JSON"));
}

#[test]
fn version_flag_prints_version() {
    ghostwriter()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("ghostwriter"));
}
