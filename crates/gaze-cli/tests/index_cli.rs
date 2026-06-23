#![cfg(feature = "index")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

const DOMAIN: &str = "local_owner/support_notes/v1";
const INDEX_KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn index_ingest_then_search_returns_tokenized_hits_without_raw_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("alpha.md"),
        "\
Name: Dr. Schmidt
Email: alice@example.invalid
Organization: Globex GmbH
Case ID: CASE-001
Support note references a local-only index fixture.
",
    )
    .expect("write alpha");
    fs::write(
        corpus.join("beta.md"),
        "\
Name: Prof. Weber
Email: weber@example.invalid
Organization: Initech AG
Case ID: CASE-002
Second support note for search isolation.
",
    )
    .expect("write beta");

    let ingest = gaze_index_command(&fake_kiji)
        .args(["index", "ingest"])
        .arg(&corpus)
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index ingest");
    assert!(
        ingest.status.success(),
        "ingest failed: stderr={}",
        String::from_utf8_lossy(&ingest.stderr)
    );
    let index_bytes = fs::read(index.join("index.json")).expect("read encrypted index");
    assert!(index_bytes.starts_with(b"GAZEIDX1"));
    assert!(
        !index_bytes
            .windows(b"alice@example.invalid".len())
            .any(|window| window == b"alice@example.invalid"),
        "encrypted index contains raw fixture email"
    );

    let search = gaze_index_command(&fake_kiji)
        .args(["index", "search", "alice@example.invalid"])
        .args(["--class", "email", "--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search");
    assert!(
        search.status.success(),
        "search failed: stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );

    let stdout = String::from_utf8(search.stdout).expect("utf8 stdout");
    assert!(stdout.contains("doc: doc:"));
    assert!(stdout.contains(":Email_"));
    assert!(stdout.contains("raw PII never shown (owner-side only)"));

    for raw in [
        "Dr. Schmidt",
        "alice@example.invalid",
        "Globex GmbH",
        "CASE-001",
        "Prof. Weber",
        "weber@example.invalid",
        "Initech AG",
        "CASE-002",
    ] {
        assert!(
            !stdout.contains(raw),
            "search stdout leaked raw fixture value {raw}: {stdout}"
        );
    }
}

#[test]
fn realistic_prose_name_org_email_regression_never_returns_raw_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("prose.md"),
        "\
Support summary: Alice Mueller from Globex GmbH wrote from alice@example.invalid about onboarding. Follow up next week.
",
    )
    .expect("write prose");

    let ingest = gaze_index_command(&fake_kiji)
        .args(["index", "ingest"])
        .arg(&corpus)
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index ingest");
    assert!(
        ingest.status.success(),
        "ingest failed: stderr={}",
        String::from_utf8_lossy(&ingest.stderr)
    );
    let ingest_stdout = String::from_utf8(ingest.stdout).expect("utf8 ingest stdout");
    assert!(
        ingest_stdout.contains("entities: 3"),
        "expected name + org + email entities, got: {ingest_stdout}"
    );

    let search = gaze_index_command(&fake_kiji)
        .args(["index", "search", "alice@example.invalid"])
        .args(["--class", "email", "--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search");
    assert!(
        search.status.success(),
        "search failed: stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );

    let stdout = String::from_utf8(search.stdout).expect("utf8 stdout");
    assert!(stdout.contains("doc: doc:"));
    assert!(stdout.contains(":Email_"));
    assert!(stdout.contains(":Name_"));
    for raw in ["Alice Mueller", "Globex GmbH", "alice@example.invalid"] {
        assert!(
            !stdout.contains(raw),
            "realistic prose search stdout leaked raw fixture value {raw}: {stdout}"
        );
    }
}

#[test]
fn index_ingest_fails_closed_without_kiji_model_or_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("alpha.md"), "Email: alice@example.invalid\n").expect("write alpha");

    let ingest = Command::cargo_bin("gaze")
        .expect("gaze bin")
        .env("GAZE_INDEX_KEY", INDEX_KEY)
        .env_remove("GAZE_KIJI_DISTILBERT_COMMAND")
        .env_remove("GAZE_KIJI_DISTILBERT_MODEL_DIR")
        .args(["index", "ingest"])
        .arg(&corpus)
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index ingest");

    assert!(
        !ingest.status.success(),
        "ingest unexpectedly succeeded without Kiji backend"
    );
    let stderr = String::from_utf8_lossy(&ingest.stderr);
    assert!(stderr.contains("SafetyNetConfig"), "stderr={stderr}");
    assert!(
        stderr.contains("GAZE_KIJI_DISTILBERT_MODEL_DIR"),
        "stderr={stderr}"
    );
}

#[test]
fn index_ingest_surfaces_safety_net_failure_detail() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_failing_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("alpha.md"), "Email: alice@example.invalid\n").expect("write alpha");

    let ingest = gaze_index_command(&fake_kiji)
        .args(["index", "ingest"])
        .arg(&corpus)
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index ingest");

    assert!(
        !ingest.status.success(),
        "ingest unexpectedly succeeded with failing Kiji backend"
    );
    let stderr = String::from_utf8_lossy(&ingest.stderr);
    assert!(stderr.contains("SafetyNetConfig"), "stderr={stderr}");
    assert!(
        stderr.contains("exit status: 13"),
        "stderr omitted backend detail: {stderr}"
    );
    assert!(
        !stderr.contains("alice@example.invalid"),
        "stderr leaked raw fixture value: {stderr}"
    );
}

fn gaze_index_command(fake_kiji: &Path) -> Command {
    let mut command = Command::cargo_bin("gaze").expect("gaze bin");
    command
        .env("GAZE_KIJI_DISTILBERT_COMMAND", fake_kiji)
        .env("GAZE_INDEX_KEY", INDEX_KEY)
        .env_remove("GAZE_KIJI_DISTILBERT_MODEL_DIR");
    command
}

fn write_fake_kiji(temp: &tempfile::TempDir) -> PathBuf {
    let script = temp.path().join("fake-kiji.py");
    fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json
import sys

text = sys.stdin.read()
targets = [
    ("Dr. Schmidt", "PER"),
    ("Prof. Weber", "PER"),
    ("Alice Mueller", "PER"),
    ("Globex GmbH", "ORG"),
    ("Initech AG", "ORG"),
]

spans = []
for value, label in targets:
    cursor = 0
    while True:
        index = text.find(value, cursor)
        if index < 0:
            break
        start = len(text[:index].encode("utf-8"))
        end = start + len(value.encode("utf-8"))
        spans.append({"label": label, "start": start, "end": end, "score": 0.99})
        cursor = index + len(value)

print(json.dumps(spans))
"#,
    )
    .expect("write fake kiji");
    let mut permissions = fs::metadata(&script)
        .expect("fake kiji metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("chmod fake kiji");
    script
}

fn write_failing_fake_kiji(temp: &tempfile::TempDir) -> PathBuf {
    let script = temp.path().join("failing-fake-kiji.py");
    fs::write(
        &script,
        r#"#!/usr/bin/env python3
import sys

sys.stderr.write("fixture boot failure\n")
sys.exit(13)
"#,
    )
    .expect("write failing fake kiji");
    let mut permissions = fs::metadata(&script)
        .expect("failing fake kiji metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("chmod failing fake kiji");
    script
}
