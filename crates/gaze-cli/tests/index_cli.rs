#![cfg(feature = "index")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serial_test::file_serial;

const DOMAIN: &str = "local_owner/support_notes/v1";
const TEST_INDEX_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const WRONG_INDEX_KEY: &str = "202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f";

fn test_subprocess_timeout_ms() -> String {
    let seconds = std::env::var("GAZE_TEST_SUBPROCESS_TIMEOUT_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("test subprocess timeout must be an integer")
        })
        .unwrap_or(60);
    assert!(seconds > 0, "test subprocess timeout must be positive");
    seconds.saturating_mul(1_000).to_string()
}

#[test]
#[file_serial(gaze_subprocess)]
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
        .arg("ingest")
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

    let search = gaze_index_command(&fake_kiji)
        .args(["search", "alice@example.invalid"])
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
#[file_serial(gaze_subprocess)]
fn index_ingest_redacts_safety_net_only_prose_by_default_without_raw_persistence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("safety-net-only.md"),
        "\
Support summary mentions Dr. Schmidt after triage.
",
    )
    .expect("write safety-net-only");

    let ingest = gaze_index_command(&fake_kiji)
        .arg("ingest")
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

    let search = gaze_index_command(&fake_kiji)
        .args(["search", "Dr. Schmidt"])
        .args(["--class", "name", "--domain", DOMAIN, "--index-path"])
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
    assert!(stdout.contains(":Name_"));
    assert!(
        !stdout.contains("Dr. Schmidt"),
        "search stdout leaked raw fixture value: {stdout}"
    );

    let sealed = fs::read(index.join("index.json")).expect("read sealed index");
    assert!(sealed.starts_with(b"GAZEIDX1"));
    assert!(
        !bytes_contain(&sealed, b"Dr. Schmidt"),
        "sealed index bytes contain raw fixture value"
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn index_ingest_redacts_residual_suspects_by_default_without_raw_persistence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_residual_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("residual.md"),
        "\
Email: alice@example.invalid
Support summary mentions Dr. Schmidt marker after triage.
",
    )
    .expect("write residual");

    let ingest = gaze_index_command(&fake_kiji)
        .arg("ingest")
        .arg(&corpus)
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index ingest");
    assert!(
        ingest.status.success(),
        "default residual ingest failed: stderr={}",
        String::from_utf8_lossy(&ingest.stderr)
    );

    let search = gaze_index_command(&fake_kiji)
        .args(["search", "alice@example.invalid"])
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
    assert!(stdout.contains(":Email_"));
    for raw in ["alice@example.invalid", "Dr. Schmidt"] {
        assert!(
            !stdout.contains(raw),
            "search stdout leaked raw fixture value {raw}: {stdout}"
        );
    }

    let sealed = fs::read(index.join("index.json")).expect("read sealed index");
    assert!(sealed.starts_with(b"GAZEIDX1"));
    for raw in [
        b"alice@example.invalid".as_slice(),
        b"Dr. Schmidt".as_slice(),
    ] {
        assert!(
            !bytes_contain(&sealed, raw),
            "sealed index bytes contain raw fixture value"
        );
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn index_ingest_strict_residual_mode_fails_closed_with_reason() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_residual_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("strict.md"),
        "\
Support summary mentions Dr. Schmidt marker after triage.
",
    )
    .expect("write strict");

    let ingest = gaze_index_command(&fake_kiji)
        .arg("ingest")
        .arg(&corpus)
        .args([
            "--on-residual",
            "strict",
            "--domain",
            DOMAIN,
            "--index-path",
        ])
        .arg(&index)
        .output()
        .expect("run index ingest");

    assert!(
        !ingest.status.success(),
        "strict residual ingest unexpectedly succeeded"
    );
    let stderr = String::from_utf8_lossy(&ingest.stderr);
    assert!(stderr.contains("ResidualSuspect"), "stderr={stderr}");
    assert!(
        stderr.contains("index safety net failed closed"),
        "stderr={stderr}"
    );
    assert!(
        !stderr.contains("index operation failed closed"),
        "stderr swallowed the real reason: {stderr}"
    );
    assert!(
        !stderr.contains("Dr. Schmidt"),
        "stderr leaked raw fixture value: {stderr}"
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn index_search_wrong_key_surfaces_decrypt_reason() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("alpha.md"), "Email: alice@example.invalid\n").expect("write alpha");

    let ingest = gaze_index_command(&fake_kiji)
        .arg("ingest")
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

    let search = gaze_index_command_with_key(&fake_kiji, WRONG_INDEX_KEY)
        .args(["search", "alice@example.invalid"])
        .args(["--class", "email", "--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search");

    assert!(
        !search.status.success(),
        "wrong-key search unexpectedly succeeded"
    );
    let stderr = String::from_utf8_lossy(&search.stderr);
    assert!(stderr.contains("decrypt"), "stderr={stderr}");
    assert!(stderr.contains("aead"), "stderr={stderr}");
    assert!(
        !stderr.contains("index operation failed closed"),
        "stderr swallowed the real reason: {stderr}"
    );
    assert!(
        !stderr.contains("alice@example.invalid"),
        "stderr leaked raw fixture value: {stderr}"
    );
}

#[test]
#[file_serial(gaze_subprocess)]
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
        .arg("ingest")
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
        .args(["search", "alice@example.invalid"])
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
#[file_serial(gaze_subprocess)]
fn index_ingest_fails_closed_without_kiji_model_or_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("alpha.md"), "Email: alice@example.invalid\n").expect("write alpha");

    let ingest = Command::cargo_bin("gaze")
        .expect("gaze bin")
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
#[file_serial(gaze_subprocess)]
fn index_search_without_class_finds_organization_and_custom_class_entities() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("data.md"),
        "Organization: Globex GmbH\nCustomer ID: 90210\n",
    )
    .expect("write data");

    let ingest = gaze_index_command(&fake_kiji)
        .arg("ingest")
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
    let ingest_stdout = String::from_utf8_lossy(&ingest.stdout).to_string();
    assert!(
        ingest_stdout.contains("entities: 2"),
        "expected organization + custom entities, got: {ingest_stdout}"
    );

    // Armed `--class` searches confirm both entities are actually indexed.
    let org_armed = gaze_index_command(&fake_kiji)
        .args(["search", "Globex GmbH"])
        .args(["--class", "org", "--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search --class org");
    assert!(org_armed.status.success(), "org armed search failed");
    let org_armed_stdout = String::from_utf8_lossy(&org_armed.stdout).to_string();
    assert!(
        org_armed_stdout.contains("doc: doc:"),
        "--class org must find indexed org: {org_armed_stdout}"
    );
    assert!(org_armed_stdout.contains(":Organization_"));

    let custom_armed = gaze_index_command(&fake_kiji)
        .args(["search", "90210"])
        .args([
            "--class",
            "custom:customer_id",
            "--domain",
            DOMAIN,
            "--index-path",
        ])
        .arg(&index)
        .output()
        .expect("run index search --class custom:customer_id");
    assert!(custom_armed.status.success(), "custom armed search failed");
    let custom_armed_stdout = String::from_utf8_lossy(&custom_armed.stdout).to_string();
    assert!(
        custom_armed_stdout.contains("doc: doc:"),
        "--class custom must find indexed custom: {custom_armed_stdout}"
    );
    assert!(custom_armed_stdout.contains(":Custom:customer_id_"));

    // The bug: no `--class` must also reach both indexed classes.
    let org_default = gaze_index_command(&fake_kiji)
        .args(["search", "Globex GmbH"])
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search no --class (org)");
    assert!(org_default.status.success(), "org default search failed");
    let org_default_stdout = String::from_utf8_lossy(&org_default.stdout).to_string();
    assert!(
        org_default_stdout.contains("doc: doc:"),
        "BUG[org]: no --class search missed indexed org: {org_default_stdout}"
    );
    assert!(org_default_stdout.contains(":Organization_"));
    assert!(
        !org_default_stdout.contains("no hits"),
        "no --class search for indexed org must not say no hits: {org_default_stdout}"
    );

    let custom_default = gaze_index_command(&fake_kiji)
        .args(["search", "90210"])
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search no --class (custom)");
    assert!(
        custom_default.status.success(),
        "custom default search failed"
    );
    let custom_default_stdout = String::from_utf8_lossy(&custom_default.stdout).to_string();
    assert!(
        custom_default_stdout.contains("doc: doc:"),
        "BUG[custom]: no --class search missed indexed custom: {custom_default_stdout}"
    );
    assert!(custom_default_stdout.contains(":Custom:customer_id_"));
    assert!(
        !custom_default_stdout.contains("no hits"),
        "no --class search for indexed custom must not say no hits: {custom_default_stdout}"
    );

    // Raw PII never leaks on the default path; the owner-side footer still prints.
    for stdout in [&org_default_stdout, &custom_default_stdout] {
        for raw in ["Globex GmbH", "90210"] {
            assert!(
                !stdout.contains(raw),
                "default search leaked raw fixture value {raw}: {stdout}"
            );
        }
        assert!(
            stdout.contains("raw PII never shown (owner-side only)"),
            "default search must show owner-side footer: {stdout}"
        );
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn index_search_without_class_still_finds_name_email_and_reports_no_hits_when_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("people.md"),
        "\
Name: Dr. Schmidt
Email: alice@example.invalid
Organization: Globex GmbH
",
    )
    .expect("write people");

    let ingest = gaze_index_command(&fake_kiji)
        .arg("ingest")
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

    // Name entity reachable without --class (regression guard for the old Name default).
    let name_default = gaze_index_command(&fake_kiji)
        .args(["search", "Dr. Schmidt"])
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search no --class (name)");
    assert!(name_default.status.success(), "name default search failed");
    let name_default_stdout = String::from_utf8_lossy(&name_default.stdout).to_string();
    assert!(
        name_default_stdout.contains("doc: doc:"),
        "no --class search for indexed name must hit: {name_default_stdout}"
    );
    assert!(name_default_stdout.contains(":Name_"));
    assert!(
        !name_default_stdout.contains("no hits"),
        "no --class search for indexed name must not say no hits: {name_default_stdout}"
    );

    // Email entity reachable without --class (regression guard for the old Email heuristic).
    let email_default = gaze_index_command(&fake_kiji)
        .args(["search", "alice@example.invalid"])
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search no --class (email)");
    assert!(
        email_default.status.success(),
        "email default search failed"
    );
    let email_default_stdout = String::from_utf8_lossy(&email_default.stdout).to_string();
    assert!(
        email_default_stdout.contains("doc: doc:"),
        "no --class search for indexed email must hit: {email_default_stdout}"
    );
    assert!(email_default_stdout.contains(":Email_"));
    assert!(
        !email_default_stdout.contains("no hits"),
        "no --class search for indexed email must not say no hits: {email_default_stdout}"
    );

    // A value absent from the index under every class still reports `no hits`.
    let miss_default = gaze_index_command(&fake_kiji)
        .args(["search", "nobody-nowhere-cafebabe"])
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search no --class (absent)");
    assert!(
        miss_default.status.success(),
        "absent default search failed"
    );
    let miss_default_stdout = String::from_utf8_lossy(&miss_default.stdout).to_string();
    assert!(
        miss_default_stdout.contains("no hits"),
        "absent value must still report no hits: {miss_default_stdout}"
    );

    // No raw PII leaks; owner-side footer present on every default search.
    for stdout in [
        &name_default_stdout,
        &email_default_stdout,
        &miss_default_stdout,
    ] {
        for raw in ["Dr. Schmidt", "alice@example.invalid", "Globex GmbH"] {
            assert!(
                !stdout.contains(raw),
                "default search leaked raw fixture value {raw}: {stdout}"
            );
        }
        assert!(
            stdout.contains("raw PII never shown (owner-side only)"),
            "default search must show owner-side footer: {stdout}"
        );
    }
}

fn gaze_index_command(fake_kiji: &Path) -> Command {
    gaze_index_command_with_key(fake_kiji, TEST_INDEX_KEY)
}

fn gaze_index_command_with_key(fake_kiji: &Path, index_key: &str) -> Command {
    let mut command = Command::cargo_bin("gaze").expect("gaze bin");
    command
        .args([
            "index",
            &format!("--safety-net-timeout-ms={}", test_subprocess_timeout_ms()),
        ])
        .env("GAZE_KIJI_DISTILBERT_COMMAND", fake_kiji)
        .env_remove("GAZE_KIJI_DISTILBERT_MODEL_DIR")
        .env("GAZE_INDEX_KEY", index_key);
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

fn write_residual_fake_kiji(temp: &tempfile::TempDir) -> PathBuf {
    let script = temp.path().join("fake-kiji-residual.py");
    fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json
import sys

text = sys.stdin.read()
spans = []

target = "Dr. Schmidt"
index = text.find(target)
if index >= 0:
    start = len(text[:index].encode("utf-8"))
    end = start + len(target.encode("utf-8"))
    spans.append({"label": "PER", "start": start, "end": end, "score": 0.99})

name_index = text.find(":Name_")
token_index = text.rfind("<", 0, name_index)
marker_index = text.find(" marker", name_index)
if name_index >= 0 and token_index >= 0 and marker_index >= 0:
    start = len(text[:token_index].encode("utf-8"))
    end = len(text[:marker_index + len(" marker")].encode("utf-8"))
    spans.append({"label": "PER", "start": start, "end": end, "score": 0.99})

print(json.dumps(spans))
"#,
    )
    .expect("write residual fake kiji");
    let mut permissions = fs::metadata(&script)
        .expect("residual fake kiji metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("chmod residual fake kiji");
    script
}

#[test]
#[file_serial(gaze_subprocess)]
fn index_search_explicit_disallowed_class_returns_policy_denial_not_no_hits() {
    // Regression: the new `allows_class` guard must NOT apply when `--class` is
    // explicitly supplied.  Previously the bridge returned DenyReason::ClassNotAllowed
    // (non-zero exit, PolicyConfig in stderr).  The guard silently turned that into a
    // successful "no hits", hiding the authorization failure.
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let fake_kiji = write_fake_kiji(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    // Built-in classes are always allowed; this corpus introduces no custom class.
    fs::write(
        corpus.join("email-only.md"),
        "Email: alice@example.invalid\n",
    )
    .expect("write email-only");

    let ingest = gaze_index_command(&fake_kiji)
        .arg("ingest")
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

    // An unconfigured custom class must reach bridge authorization and be denied.
    let search = gaze_index_command(&fake_kiji)
        .args(["search", "alice@example.invalid"])
        .args([
            "--class",
            "custom:unconfigured",
            "--domain",
            DOMAIN,
            "--index-path",
        ])
        .arg(&index)
        .output()
        .expect("run index search");

    // Must fail with a non-zero exit (policy denial), not succeed with "no hits".
    assert!(
        !search.status.success(),
        "explicit disallowed class must return non-zero exit, got success with stdout={}",
        String::from_utf8_lossy(&search.stdout)
    );
    let stderr = String::from_utf8_lossy(&search.stderr);
    assert!(
        stderr.contains("PolicyConfig") || stderr.contains("ClassNotAllowed"),
        "stderr must contain policy denial reason, got: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&search.stdout);
    assert!(
        !stdout.contains("no hits"),
        "explicit disallowed class must not produce 'no hits' success: {stdout}"
    );
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
