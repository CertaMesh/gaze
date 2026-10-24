//! CLI policy, class and exact-restore contract for synthetic person-linked IDs.
use assert_cmd::Command;
use serde_json::{json, Value};
use std::fs;
use tempfile::tempdir;

#[test]
fn labelled_record_ids_cli_round_trip_and_public_counterweights() {
    let dir = tempdir().unwrap();
    let policy = dir.path().join("policy.toml");
    fs::write(&policy, "[session]\nscope = \"persistent\"\nttl_secs = 86400\n[policy.rulepacks]\nbundled = [\"core\"]\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n").unwrap();
    let input = r#"{"customerId":"C-000123","personalnummer":"E-000123","member_id":"M-000123","order_id":"ORD-000123","record_id":"REC-000123","version":"1.2.3","amount":12.50,"customer_id":null,"employeeId":true,"contact_record_id":false}"#;
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--policy", policy.to_str().unwrap()])
        .write_stdin(input)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    let clean = reply["clean_text"].as_str().unwrap();
    let clean_json: Value =
        serde_json::from_str(clean).expect("JSON keywords retain valid scalar syntax");
    assert!(clean_json["customer_id"].is_null());
    assert_eq!(clean_json["employeeId"], true);
    assert_eq!(clean_json["contact_record_id"], false);
    for class in ["customer_id", "employee_id", "record_id"] {
        assert!(clean.contains(&format!(":Custom:{class}_")), "{clean}");
    }
    for value in ["C-000123", "E-000123", "M-000123"] {
        assert!(!clean.contains(&format!("\"{value}\"")), "{clean}");
    }
    for value in ["ORD-000123", "REC-000123", "1.2.3", "12.50"] {
        assert!(clean.contains(value), "{clean}");
    }
    let restored = Command::cargo_bin("gaze")
        .unwrap()
        .arg("restore")
        .write_stdin(json!({"session_blob":reply["session_blob"],"text":clean}).to_string())
        .output()
        .unwrap();
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let restored: Value = serde_json::from_slice(&restored.stdout).unwrap();
    assert_eq!(restored["text"].as_str().unwrap(), input);
}
