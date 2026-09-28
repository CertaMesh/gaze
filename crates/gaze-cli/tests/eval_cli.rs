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
    let first = json!({"text":"abc","spans":[]});
    let second = fs::read_to_string(file.path()).unwrap();
    fs::write(file.path(), format!("{first}\n{second}")).unwrap();
    let output = Command::cargo_bin("gaze")
        .expect("gaze binary")
        .args(["eval", file.path().to_str().unwrap(), "--json"])
        .output()
        .expect("run eval");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "{\"error\":\"EvalSchema\",\"exit\":1,\"line\":2}\n"
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

#[test]
fn eval_counts_every_span_of_a_repeated_value() {
    let pii = "alice@example.invalid";
    let text = format!("{pii} and {pii}");
    let second = pii.len() + " and ".len();
    let file = corpus(
        &text,
        json!([
            {"start": 0, "end": pii.len(), "label": "email"},
            {"start": second, "end": second + pii.len(), "label": "email"}
        ]),
    );
    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args(["eval", file.path().to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["pii_bytes"], 2 * pii.len());
    assert_eq!(report["true_positive_bytes"], 2 * pii.len());
    assert_eq!(report["per_class"]["email"]["fully_covered"], 2);
}

#[test]
fn eval_and_clean_protect_the_same_context_dictionary_bytes() {
    let term = "context-song-123";
    let text = format!("play {term}");
    let file = corpus(
        &text,
        json!([{"start": 5, "end": 5 + term.len(), "label": "custom:song"}]),
    );
    let context = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        context.path(),
        json!({
            "dictionaries": {"songs": {"terms": [term], "case_sensitive": true}},
            "class_map": {"songs": "custom:song"},
            "fields": {}
        })
        .to_string(),
    )
    .unwrap();
    let context_path = context.path().to_str().unwrap();
    let clean = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--context-json", context_path])
        .write_stdin(text.as_bytes().to_vec())
        .output()
        .unwrap();
    assert!(clean.status.success(), "{:?}", clean.stderr);
    let clean: serde_json::Value = serde_json::from_slice(&clean.stdout).unwrap();
    let protected_bytes: usize = clean["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["raw"].as_str().unwrap().len())
        .sum();
    let eval = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "eval",
            file.path().to_str().unwrap(),
            "--context-json",
            context_path,
            "--json",
        ])
        .output()
        .unwrap();
    assert!(eval.status.success(), "{:?}", eval.stderr);
    let eval: serde_json::Value = serde_json::from_slice(&eval.stdout).unwrap();
    assert_eq!(protected_bytes, term.len());
    assert_eq!(eval["predicted_bytes"], protected_bytes);
    assert_eq!(eval["leaked_bytes"], 0);
}

#[test]
fn adopter_files_in_wrong_slots_never_echo_their_contents() {
    let secret = "alice@example.invalid";
    let wrong_file = corpus(secret, json!([]));
    let path = wrong_file.path().to_str().unwrap();
    let valid_corpus = corpus("abc", json!([]));
    let corpus_path = valid_corpus.path().to_str().unwrap();
    let cases: &[(&[&str], &str)] = &[
        (&["clean", "--policy", path], "policy TOML parse error"),
        (
            &["clean", "--context-json", path],
            "context JSON parse error",
        ),
        (
            &["clean", "--rulepack-path", path],
            "rulepack TOML parse error",
        ),
        (
            &["eval", corpus_path, "--policy", path],
            "policy TOML parse error",
        ),
        (
            &["eval", corpus_path, "--context-json", path],
            "context JSON parse error",
        ),
        (
            &["eval", corpus_path, "--rulepack-path", path],
            "rulepack TOML parse error",
        ),
        (&["eval", corpus_path, "--label-map", path], "EvalSchema"),
        (&["daemon", "--policy", path], "policy TOML parse error"),
    ];
    for (args, expected) in cases {
        let output = Command::cargo_bin("gaze")
            .unwrap()
            .args(*args)
            .write_stdin(b"safe input".to_vec())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(expected), "{args:?}: {stderr}");
        if expected.contains("parse error") {
            assert!(stderr.contains("line "), "{args:?}: {stderr}");
            assert!(stderr.contains("column "), "{args:?}: {stderr}");
        }
        assert!(!stderr.contains(secret), "{args:?}: {stderr}");
        assert!(!stderr.contains("\"text\""), "{args:?}: {stderr}");
    }
}

#[test]
fn semantic_file_errors_redact_adopter_values() {
    let secret = "alice@example.invalid";
    let corpus = corpus("abc", json!([]));
    let policy = tempfile::NamedTempFile::new().unwrap();
    fs::write(policy.path(), format!("schema_version = \"{secret}\"\n")).unwrap();
    let context = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        context.path(),
        json!({"dictionaries": {}, "class_map": {"test": secret}, "fields": {}}).to_string(),
    )
    .unwrap();
    let labels = tempfile::NamedTempFile::new().unwrap();
    fs::write(labels.path(), json!({"TEST": secret}).to_string()).unwrap();
    let cases: &[&[&str]] = &[
        &["clean", "--policy", policy.path().to_str().unwrap()],
        &["clean", "--context-json", context.path().to_str().unwrap()],
        &[
            "eval",
            corpus.path().to_str().unwrap(),
            "--policy",
            policy.path().to_str().unwrap(),
        ],
        &[
            "eval",
            corpus.path().to_str().unwrap(),
            "--context-json",
            context.path().to_str().unwrap(),
        ],
        &[
            "eval",
            corpus.path().to_str().unwrap(),
            "--label-map",
            labels.path().to_str().unwrap(),
        ],
    ];
    for args in cases {
        let output = Command::cargo_bin("gaze")
            .unwrap()
            .args(*args)
            .write_stdin(b"safe input".to_vec())
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains(secret),
            "{args:?}"
        );
    }
}

#[cfg(feature = "proxy")]
#[test]
fn proxy_policy_parse_error_never_echoes_file_contents() {
    let secret = "alice@example.invalid";
    let wrong_file = corpus(secret, json!([]));
    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "proxy",
            "serve",
            "--policy",
            wrong_file.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("policy TOML parse error"));
    assert!(!stderr.contains(secret));
}

#[cfg(feature = "safety-net-nym")]
#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR pointing at the pinned bundle"]
fn eval_and_clean_protect_the_same_nym_policy_bytes() {
    let model_dir = std::env::var("GAZE_NYM_MODEL_DIR").unwrap();
    let text = "Das Fahrzeug mit dem Kennzeichen M-AB 1234 wurde abgeschleppt.";
    let plate = "M-AB 1234";
    let start = text.find(plate).unwrap();
    let file = corpus(
        text,
        json!([{"start": start, "end": start + plate.len(), "label": "custom:license_plate"}]),
    );
    let policy = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        policy.path(),
        "[session]\nscope = \"conversation\"\n[policy.rulepacks]\nbundled = [\"core\"]\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n[safety_net]\nbackend = \"nym\"\n",
    )
    .unwrap();
    let policy_path = policy.path().to_str().unwrap();
    let args = ["--policy", policy_path, "--nym-model-dir", &model_dir];
    let clean = Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .write_stdin(text.as_bytes().to_vec())
        .output()
        .unwrap();
    assert!(clean.status.success(), "{:?}", clean.stderr);
    let clean: serde_json::Value = serde_json::from_slice(&clean.stdout).unwrap();
    let protected_bytes: usize = clean["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["raw"].as_str().unwrap().len())
        .sum();
    let eval = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "eval",
            file.path().to_str().unwrap(),
            "--policy",
            policy_path,
            "--nym-model-dir",
            &model_dir,
            "--json",
        ])
        .output()
        .unwrap();
    assert!(eval.status.success(), "{:?}", eval.stderr);
    let eval: serde_json::Value = serde_json::from_slice(&eval.stdout).unwrap();
    assert!(protected_bytes > 0);
    assert_eq!(eval["predicted_bytes"], protected_bytes);
}
