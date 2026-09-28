use std::fs;

use assert_cmd::Command;
use serde_json::json;

fn corpus(text: &str, spans: serde_json::Value) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().expect("temp corpus");
    let row = json!({"text": text, "spans": spans});
    fs::write(file.path(), format!("{row}\n")).expect("write corpus");
    file
}

#[test]
fn eval_human_golden_and_json_metrics() {
    let pii = "alice@example.invalid";
    let text = format!("Contact {pii}.");
    let file = corpus(
        &text,
        json!([{"start": 8, "end": 8 + pii.len(), "label": "email"}]),
    );
    let human = Command::cargo_bin("gaze")
        .expect("gaze binary")
        .args(["eval", file.path().to_str().unwrap()])
        .output()
        .expect("run eval");
    assert!(human.status.success(), "{:?}", human.stderr);
    let expected = format!(
        "Documents: 1 (leak rate: 0.00%; zero leak: 100.00%)\nUTF-8 bytes: PII {0} | protected {0} | leaked 0 | false positive 0\nByte precision 1.0000 | recall 1.0000 | F1 1.0000\nClass\tGold bytes\tCovered\tLeaked\tEntities\tFully covered\nemail\t{0}\t{0}\t0\t1\t1\n",
        pii.len()
    );
    assert_eq!(String::from_utf8_lossy(&human.stdout), expected);

    let machine = Command::cargo_bin("gaze")
        .expect("gaze binary")
        .args(["eval", file.path().to_str().unwrap(), "--json"])
        .output()
        .expect("run eval json");
    assert!(machine.status.success(), "{:?}", machine.stderr);
    let report: serde_json::Value = serde_json::from_slice(&machine.stdout).unwrap();
    assert_eq!(report["leaked_bytes"], 0);
    assert_eq!(report["document_leak_rate"], 0.0);
    assert_eq!(report["false_positive_bytes"], 0);
    assert_eq!(report["per_class"]["email"]["fully_covered"], 1);
    for output in [
        &human.stdout,
        &human.stderr,
        &machine.stdout,
        &machine.stderr,
    ] {
        assert!(!String::from_utf8_lossy(output).contains(pii));
    }
}

#[test]
fn eval_schema_error_never_echoes_text() {
    let secret = "alice@example.invalid";
    let file = corpus(secret, json!([{"start": 1, "end": 200, "label": "email"}]));
    let output = Command::cargo_bin("gaze")
        .expect("gaze binary")
        .args(["eval", file.path().to_str().unwrap(), "--json"])
        .output()
        .expect("run eval");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "{\"error\":\"EvalSchema\",\"exit\":1}\n"
    );
}

#[test]
fn eval_label_map_groups_gold_by_canonical_class() {
    let pii = "alice@example.invalid";
    let text = format!("Reach {pii}");
    let file = corpus(
        &text,
        json!([{"start": 6, "end": 6 + pii.len(), "label": "EMAIL_ADDRESS"}]),
    );
    let map = tempfile::NamedTempFile::new().unwrap();
    fs::write(map.path(), "{\"EMAIL_ADDRESS\":\"email\"}").unwrap();
    let output = Command::cargo_bin("gaze")
        .expect("gaze binary")
        .args([
            "eval",
            file.path().to_str().unwrap(),
            "--label-map",
            map.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run eval");
    assert!(output.status.success(), "{:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["pii_bytes"], pii.len());
    assert_eq!(report["per_class"]["email"]["leaked_bytes"], 0);
}

#[test]
fn eval_counts_protected_unannotated_bytes_as_false_positives() {
    let pii = "alice@example.invalid";
    let file = corpus(pii, json!([]));
    let output = Command::cargo_bin("gaze")
        .expect("gaze binary")
        .args(["eval", file.path().to_str().unwrap(), "--json"])
        .output()
        .expect("run eval");
    assert!(output.status.success(), "{:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["false_positive_bytes"], pii.len());
    assert_eq!(report["document_leak_rate"], 0.0);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(pii));
}
