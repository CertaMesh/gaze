#![cfg(feature = "index")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

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
    let index_env = index_ner(&temp);
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

    let ingest = gaze_index_command(&index_env)
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

    let search = gaze_index_command(&index_env)
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
fn index_ingest_tokenizes_ner_only_prose_without_raw_persistence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let index_env = index_ner(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("safety-net-only.md"),
        "\
Support summary mentions Dr. Schmidt after triage.
",
    )
    .expect("write safety-net-only");

    let ingest = gaze_index_command(&index_env)
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

    let search = gaze_index_command(&index_env)
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
    let index_env = index_ner_with_residual_opf(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("residual.md"),
        "\
Email: alice@example.invalid
Support summary mentions Dr. Schmidt marker after triage.
",
    )
    .expect("write residual");

    let ingest = gaze_index_command(&index_env)
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

    let search = gaze_index_command(&index_env)
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
    let index_env = index_ner_with_residual_opf(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("strict.md"),
        "\
Support summary mentions Dr. Schmidt marker after triage.
",
    )
    .expect("write strict");

    let ingest = gaze_index_command(&index_env)
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
    let index_env = index_ner(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("alpha.md"), "Email: alice@example.invalid\n").expect("write alpha");

    let ingest = gaze_index_command(&index_env)
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

    let search = gaze_index_command_with_key(&index_env, WRONG_INDEX_KEY)
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
    let index_env = index_ner(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("prose.md"),
        "\
Support summary: Alice Mueller from Globex GmbH wrote from alice@example.invalid about onboarding. Follow up next week.
",
    )
    .expect("write prose");

    let ingest = gaze_index_command(&index_env)
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

    let search = gaze_index_command(&index_env)
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

/// Ingest runs the same `core` floor as a policy-less `gaze clean`, so the structured
/// identifiers NER never sees are tokenized before they reach the snippet that search prints.
/// The fake output net reports nothing, so this pins the deterministic floor, not a net.
#[test]
#[file_serial(gaze_subprocess)]
fn index_ingest_tokenizes_core_identifiers_so_search_never_shows_them_raw() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let index_env = index_ner(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("identifiers.md"),
        "\
Alice Mueller wrote from alice@example.invalid about her refund.
Card 4111 1111 1111 1111 was charged twice.
IBAN AT61 1904 3002 3457 3201 is the refund target.
Her router is at ip 10.1.2.3 and she can be reached on +43 1 234 5678.
",
    )
    .expect("write identifiers");

    let ingest = gaze_index_command(&index_env)
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

    let search = gaze_index_command(&index_env)
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
    assert!(stdout.contains("doc: doc:"), "expected a hit: {stdout}");
    for token in [
        ":Custom:credit_card_",
        ":Custom:iban_",
        ":Custom:ip_address_",
        ":Custom:phone_",
    ] {
        assert!(
            stdout.contains(token),
            "search stdout lacks a {token} token: {stdout}"
        );
    }
    for raw in [
        "alice@example.invalid",
        "Alice Mueller",
        "4111 1111 1111 1111",
        "AT61 1904 3002 3457 3201",
        "10.1.2.3",
        "+43 1 234 5678",
    ] {
        assert!(
            !stdout.contains(raw),
            "search stdout leaked raw fixture value {raw}: {stdout}"
        );
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn index_ingest_fails_closed_without_ner_model() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("alpha.md"), "Email: alice@example.invalid\n").expect("write alpha");

    let ingest = Command::cargo_bin("gaze")
        .expect("gaze bin")
        .env_remove("GAZE_NER_MODEL_DIR")
        .env("GAZE_INDEX_KEY", TEST_INDEX_KEY)
        .args(["index", "ingest"])
        .arg(&corpus)
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index ingest");

    assert_eq!(
        ingest.status.code(),
        Some(2),
        "ingest must fail closed without an NER model: stderr={}",
        String::from_utf8_lossy(&ingest.stderr)
    );
    let stderr = String::from_utf8_lossy(&ingest.stderr);
    assert!(stderr.contains("IndexNerModelMissing"), "stderr={stderr}");
    assert!(stderr.contains("GAZE_NER_MODEL_DIR"), "stderr={stderr}");
    assert!(!index.exists(), "a refused ingest must not create an index");
}

/// A bundle that is internally consistent but is not the pinned Davlan bundle is refused before
/// any model file loads.
#[test]
#[file_serial(gaze_subprocess)]
fn index_ingest_refuses_an_unpinned_ner_bundle() {
    use sha2::{Digest, Sha256};

    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("alpha.md"), "Email: alice@example.invalid\n").expect("write alpha");
    let ner_dir = temp.path().join("davlan-mbert-ner-hrl");
    fs::create_dir_all(&ner_dir).expect("ner dir");
    fs::set_permissions(&ner_dir, fs::Permissions::from_mode(0o700)).expect("chmod ner dir");
    let mut sums = String::new();
    for name in [
        "model.onnx",
        "tokenizer.json",
        "tokenizer_config.json",
        "config.json",
        "special_tokens_map.json",
        "vocab.txt",
        "labels.json",
    ] {
        let body = format!("not the pinned {name}");
        fs::write(ner_dir.join(name), &body).expect("write artifact");
        fs::set_permissions(ner_dir.join(name), fs::Permissions::from_mode(0o600))
            .expect("chmod artifact");
        sums.push_str(&format!("{:x}  {name}\n", Sha256::digest(body.as_bytes())));
    }
    fs::write(ner_dir.join("SHA256SUMS"), sums).expect("write sums");
    fs::set_permissions(
        ner_dir.join("SHA256SUMS"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("chmod sums");

    let ingest = Command::cargo_bin("gaze")
        .expect("gaze bin")
        .env_remove("GAZE_NER_MODEL_DIR")
        .env("GAZE_INDEX_KEY", TEST_INDEX_KEY)
        .args(["index", "ingest"])
        .arg(&corpus)
        .arg("--ner-model-dir")
        .arg(&ner_dir)
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index ingest");

    assert_eq!(
        ingest.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&ingest.stderr)
    );
    let stderr = String::from_utf8_lossy(&ingest.stderr);
    assert!(stderr.contains("IndexNerModelMissing"), "stderr={stderr}");
    assert!(
        stderr.contains("pinned NER bundle verification failed"),
        "stderr={stderr}"
    );
    assert!(!index.exists(), "a refused ingest must not create an index");
}

#[test]
#[file_serial(gaze_subprocess)]
fn index_search_without_class_finds_organization_and_custom_class_entities() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let index_env = index_ner(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(
        corpus.join("data.md"),
        "Organization: Globex GmbH\nCustomer ID: 90210\n",
    )
    .expect("write data");

    let ingest = gaze_index_command(&index_env)
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
    let org_armed = gaze_index_command(&index_env)
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

    let custom_armed = gaze_index_command(&index_env)
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
    let org_default = gaze_index_command(&index_env)
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

    let custom_default = gaze_index_command(&index_env)
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
    let index_env = index_ner(&temp);
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

    let ingest = gaze_index_command(&index_env)
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
    let name_default = gaze_index_command(&index_env)
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
    let email_default = gaze_index_command(&index_env)
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
    let miss_default = gaze_index_command(&index_env)
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

/// What `gaze index` runs against in a test: the test-support NER double (in place of the pinned
/// Davlan bundle) and a fake OPF net (search always needs an output net).
struct IndexEnv {
    ner_dir: PathBuf,
    opf: PathBuf,
    checkpoint: PathBuf,
}

fn gaze_index_command(env: &IndexEnv) -> Command {
    gaze_index_command_with_key(env, TEST_INDEX_KEY)
}

fn gaze_index_command_with_key(env: &IndexEnv, index_key: &str) -> Command {
    let mut command = Command::cargo_bin("gaze").expect("gaze bin");
    command
        .args(["index", "--safety-net", "openai-filter", "--opf-command"])
        .arg(&env.opf)
        .arg("--opf-checkpoint")
        .arg(&env.checkpoint)
        .arg(format!(
            "--safety-net-timeout-ms={}",
            test_subprocess_timeout_ms()
        ))
        .env("GAZE_NER_MODEL_DIR", &env.ner_dir)
        .env_remove("GAZE_NYM_MODEL_DIR")
        .env_remove("GAZE_OPENAI_FILTER_OPF")
        .env_remove("OPF_CHECKPOINT")
        .env("GAZE_INDEX_KEY", index_key);
    command
}

/// NER double plus a fake OPF net that reports nothing. The double recognizes this directory
/// name and detects Dr. Schmidt, Prof. Weber, Alice Mueller, Globex GmbH and Initech AG.
fn index_ner(temp: &tempfile::TempDir) -> IndexEnv {
    index_env(
        temp,
        "print(json.dumps({\"text\": text, \"detected_spans\": []}))\n",
    )
}

/// NER double plus a two-pass fake OPF net. First pass: it flags the raw word `summary`, which
/// the resolver tokenizes. Next pass: it flags a surviving token plus ` marker`, a residual the
/// resolver may not act on again, so `--on-residual` decides.
fn index_ner_with_residual_opf(temp: &tempfile::TempDir) -> IndexEnv {
    index_env(
        temp,
        r#"spans = []
first = text.find("summary")
if first >= 0:
    spans.append({"label": "private_person", "start": first, "end": first + len("summary"), "score": 0.99})
else:
    name_index = text.find(":Name_")
    token_index = text.rfind("<", 0, name_index)
    marker_index = text.find(" marker", name_index)
    if name_index >= 0 and token_index >= 0 and marker_index >= 0:
        # OPF reports character offsets.
        spans.append({"label": "private_person", "start": token_index, "end": marker_index + len(" marker"), "score": 0.99})
print(json.dumps({"text": text, "detected_spans": spans}))
"#,
    )
}

fn index_env(temp: &tempfile::TempDir, body: &str) -> IndexEnv {
    let ner_dir = temp.path().join("__gaze_test_index_ner");
    fs::create_dir_all(&ner_dir).expect("ner dir");
    let opf = temp.path().join("fake-opf.py");
    fs::write(
        &opf,
        format!(
            "#!/usr/bin/env python3\nimport json\nimport sys\n\ntext = sys.stdin.read()\n{body}"
        ),
    )
    .expect("write fake opf");
    fs::set_permissions(&opf, fs::Permissions::from_mode(0o755)).expect("chmod fake opf");
    let checkpoint = temp.path().join("opf-checkpoint");
    fs::create_dir_all(&checkpoint).expect("checkpoint dir");
    fs::set_permissions(&checkpoint, fs::Permissions::from_mode(0o700)).expect("chmod checkpoint");
    IndexEnv {
        ner_dir,
        opf,
        checkpoint,
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn index_search_without_a_safety_net_fails_closed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let corpus = temp.path().join("corpus");
    let index = temp.path().join("owner-index");
    let index_env = index_ner(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    fs::write(corpus.join("alpha.md"), "Email: alice@example.invalid\n").expect("write alpha");
    let ingest = gaze_index_command(&index_env)
        .arg("ingest")
        .arg(&corpus)
        .args(["--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index ingest");
    assert!(
        ingest.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&ingest.stderr)
    );

    let search = Command::cargo_bin("gaze")
        .expect("gaze bin")
        .env("GAZE_INDEX_KEY", TEST_INDEX_KEY)
        .args(["index", "search", "alice@example.invalid"])
        .args(["--class", "email", "--domain", DOMAIN, "--index-path"])
        .arg(&index)
        .output()
        .expect("run index search");
    assert_eq!(
        search.status.code(),
        Some(3),
        "stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );
    assert!(
        search.stdout.is_empty(),
        "a refused search must not print hits"
    );
    let stderr = String::from_utf8_lossy(&search.stderr);
    assert!(stderr.contains("SafetyNetConfig"), "stderr={stderr}");
    assert!(stderr.contains("--safety-net"), "stderr={stderr}");
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
    let index_env = index_ner(&temp);
    fs::create_dir_all(&corpus).expect("corpus dir");
    // Built-in classes are always allowed; this corpus introduces no custom class.
    fs::write(
        corpus.join("email-only.md"),
        "Email: alice@example.invalid\n",
    )
    .expect("write email-only");

    let ingest = gaze_index_command(&index_env)
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
    let search = gaze_index_command(&index_env)
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
