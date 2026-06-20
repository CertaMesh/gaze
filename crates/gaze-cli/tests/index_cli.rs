#![cfg(feature = "index")]

use std::fs;

use assert_cmd::Command;

const DOMAIN: &str = "local_owner/support_notes/v1";

#[test]
fn index_ingest_then_search_returns_tokenized_hits_without_raw_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
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

    let ingest = Command::cargo_bin("gaze")
        .expect("gaze bin")
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

    let search = Command::cargo_bin("gaze")
        .expect("gaze bin")
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
