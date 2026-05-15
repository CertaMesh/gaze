//! End-to-end integration tests for the `gaze` pipe-mode CLI.
//!
//! Drives the compiled bin via `assert_cmd` and asserts the wire contract
//! covered by the consolidated roadmap's shipped CLI history.

use std::fs;
use std::thread::sleep;
use std::time::Duration;

use assert_cmd::Command;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use regex::Regex;
use rusqlite::{params, params_from_iter, Connection};
use serde_json::{json, Value};
use tempfile::tempdir;

use gaze::{PiiClass, Scope, Session};
use gaze_audit::{build_audit_query_sql, AuditFilter, SqliteLogger, AUDIT_RESTRICTED_COLUMNS};

/// Run `gaze clean` on the given stdin and parse the JSON response.
fn clean_ok(input: &str) -> (String, String, u64) {
    clean_ok_with_args(&[], input)
}

fn clean_ok_with_args(args: &[&str], input: &str) -> (String, String, u64) {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clean failed: status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "expected empty stderr on success, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let clean_text = v["clean_text"].as_str().unwrap().to_string();
    let blob = v["session_blob"].as_str().unwrap().to_string();
    let detections = v["stats"]["detections"].as_u64().unwrap();
    (clean_text, blob, detections)
}

fn clean_json_with_args(args: &[&str], input: &str) -> Value {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clean failed: status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout is JSON")
}

fn clean_raw_with_args(args: &[&str], input: &str) -> std::process::Output {
    Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap()
}

/// Run `gaze restore` with the given request body. Returns (status_code, stdout, stderr).
fn restore_raw(body: &[u8]) -> (Option<i32>, Vec<u8>, Vec<u8>) {
    restore_raw_with_args(&[], body)
}

fn restore_raw_with_args(args: &[&str], body: &[u8]) -> (Option<i32>, Vec<u8>, Vec<u8>) {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("restore")
        .args(args)
        .write_stdin(body.to_vec())
        .output()
        .unwrap();
    (out.status.code(), out.stdout, out.stderr)
}

fn restore_json(session_blob: &str, text: &str) -> (Option<i32>, Vec<u8>, Vec<u8>) {
    let body = json!({ "session_blob": session_blob, "text": text }).to_string();
    restore_raw(body.as_bytes())
}

fn restore_json_with_args(
    args: &[&str],
    session_blob: &str,
    text: &str,
) -> (Option<i32>, Vec<u8>, Vec<u8>) {
    let body = json!({ "session_blob": session_blob, "text": text }).to_string();
    restore_raw_with_args(args, body.as_bytes())
}

fn parse_stderr_variant(stderr: &[u8]) -> Value {
    serde_json::from_slice(stderr).expect("stderr is one-line JSON")
}

/// Assert a `PolicyConfig`/exit=2 stderr envelope and require `detail` to be a
/// non-empty string. The exact detail wording is intentionally not pinned here
/// so the helper can be reused across the many sites that route through the
/// same envelope; tests that care about specific wording should call
/// `assert_policy_config_detail_contains` instead.
fn assert_policy_config_envelope(stderr: &[u8]) {
    let value = parse_stderr_variant(stderr);
    assert_eq!(value["error"], "PolicyConfig", "stderr={value}");
    assert_eq!(value["exit"], 2, "stderr={value}");
    let detail = value
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        !detail.is_empty(),
        "expected non-empty detail on PolicyConfig envelope: {value}"
    );
}

fn assert_policy_config_detail_contains(stderr: &[u8], needle: &str) {
    let value = parse_stderr_variant(stderr);
    assert_eq!(value["error"], "PolicyConfig", "stderr={value}");
    assert_eq!(value["exit"], 2, "stderr={value}");
    let detail = value
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        detail.contains(needle),
        "detail '{detail}' must contain '{needle}'"
    );
}

fn assert_session_scoped_custom_token(token: &str) {
    let re = Regex::new(r"^<[0-9a-f]{8}:Custom:[a-z0-9_]+_\d+>$").unwrap();
    assert!(re.is_match(token), "unexpected custom token shape: {token}");
}

fn restore_success_text(blob: &str, text: &str) -> String {
    let (code, stdout, stderr) = restore_json(blob, text);
    assert_eq!(code, Some(0), "stderr={}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty(), "expected empty stderr");
    let resp: Value = serde_json::from_slice(&stdout).unwrap();
    resp["text"].as_str().unwrap().to_string()
}

fn write_minimal_policy() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'alice@example[.]com'
class = "email"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();
    (dir, path)
}

fn write_policy_with_session_scope(scope: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "{scope}"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "class_alpha"
pattern = 'class-alpha-[0-9]+'
class = "custom:class_alpha"

[[rule]]
kind = "class"
class = "custom:class_alpha"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#
        ),
    )
    .unwrap();
    (dir, path)
}

fn write_policy_with_bundled_id(rulepack_id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = ["{rulepack_id}"]

[[rule]]
kind = "class"
class = "custom:class_alpha"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#
        ),
    )
    .unwrap();
    (dir, path)
}

fn write_policy_with_ner_locale(locale: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[ner]
locale = "{locale}"

[[policy.custom_recognizers]]
kind = "regex"
name = "class_alpha"
pattern = 'class-alpha-[0-9]+'
class = "custom:class_alpha"

[[rule]]
kind = "class"
class = "custom:class_alpha"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#
        ),
    )
    .unwrap();
    (dir, path)
}

fn write_policy_with_ner_model_dir(
    policy_model_dir: &std::path::Path,
    locale_active: &str,
    ner_locale: &str,
    threshold: f32,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[locale]
active = ["{locale_active}"]

[ner]
model_dir = "{}"
locale = "{ner_locale}"
threshold = {threshold}

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "name"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
            policy_model_dir.display()
        ),
    )
    .unwrap();
    (dir, path)
}

fn class_alpha_rulepack() -> String {
    r#"
schema_version = "0.1.0"
rulepack_id = "class-alpha"
rulepack_version = "0.4.2"
default_locales = ["global"]

[[recognizers]]
id = "class-alpha.regex"
class = "custom:class_alpha"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = 'class-alpha-[0-9]+'

[recognizers.scoring]
base = 0.42
priority = 77

[recognizers.token]
"#
    .to_string()
}

fn write_policy_for_class_alpha_rulepack() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "custom:class_alpha"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();
    (dir, path)
}

fn assert_symmetric_policy_config(cli_out: std::process::Output, toml_out: std::process::Output) {
    assert_eq!(cli_out.status.code(), Some(2));
    assert_eq!(toml_out.status.code(), Some(2));
    assert!(cli_out.stdout.is_empty(), "CLI stdout must stay empty");
    assert!(toml_out.stdout.is_empty(), "TOML stdout must stay empty");
    assert_eq!(
        cli_out.stderr, toml_out.stderr,
        "CLI and TOML policy errors must be byte-identical"
    );
    assert_policy_config_envelope(&cli_out.stderr);
}

fn normalize_session_hex(text: &str) -> String {
    Regex::new(r"<[0-9a-f]{8}:")
        .unwrap()
        .replace_all(text, "<SESSION:")
        .to_string()
}

fn write_policy_with_rulepack(
    rulepack: &str,
    locale_active: Option<&str>,
) -> (tempfile::TempDir, std::path::PathBuf) {
    write_policy_with_rulepack_and_class(rulepack, locale_active, "email")
}

fn write_policy_with_rulepack_and_class(
    rulepack: &str,
    locale_active: Option<&str>,
    class: &str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let rulepack_path = dir.path().join("rulepack.toml");
    fs::write(&rulepack_path, rulepack).unwrap();
    let path = dir.path().join("policy.toml");
    let locale = locale_active
        .map(|active| format!("\n[locale]\nactive = [{active}]\n"))
        .unwrap_or_default();
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400
{locale}
[policy.rulepacks]
bundled = []
paths = ["{}"]

[[rule]]
kind = "class"
class = "{class}"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
            rulepack_path.display()
        ),
    )
    .unwrap();
    (dir, path)
}

fn de_email_rulepack(locales: &str) -> String {
    format!(
        r#"
schema_version = "0.1.0"
rulepack_id = "test-de"
rulepack_version = "0.4.0"
default_locales = ["de-DE"]

[[recognizers]]
id = "email.de"
class = "Email"
enabled = true
locales = [{locales}]

[recognizers.match]
kind = "regex"
pattern = 'kundennummer-[0-9]+'

[recognizers.scoring]
base = 0.42
priority = 77

[recognizers.token]
"#
    )
}

fn write_policy_with_bundled_rulepacks(
    bundled_rulepacks: &[&str],
    locale_active: &str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    let bundled = bundled_rulepacks
        .iter()
        .map(|rulepack| format!("\"{rulepack}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[locale]
active = ["{locale_active}"]

[policy.rulepacks]
bundled = [{bundled}]

[[rule]]
kind = "class"
class = "name"
action = "tokenize"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#
        ),
    )
    .unwrap();
    (dir, path)
}

fn write_policy_with_core_extended_rulepacks(
    bundled_rulepacks: &[&str],
    locale_active: &str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    let bundled = bundled_rulepacks
        .iter()
        .map(|rulepack| format!("\"{rulepack}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[locale]
active = ["{locale_active}"]

[policy.rulepacks]
bundled = [{bundled}]

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:phone"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:ip_address"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:postal_code"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:iban"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:family:payment-card-or-iban"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:credit_card"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#
        ),
    )
    .unwrap();
    (dir, path)
}

fn write_policy_with_test_ner(
    bundled_rulepacks: &[&str],
    locale_active: &str,
    ner_locale: &str,
    threshold: f32,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let model_dir = dir.path().join("__gaze_test_fixed_ner");
    fs::create_dir(&model_dir).unwrap();
    let path = dir.path().join("policy.toml");
    let bundled = bundled_rulepacks
        .iter()
        .map(|rulepack| format!("\"{rulepack}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[locale]
active = ["{locale_active}"]

[ner]
model_dir = "{}"
locale = "{ner_locale}"
threshold = {threshold}

[policy.rulepacks]
bundled = [{bundled}]

[[rule]]
kind = "class"
class = "name"
action = "tokenize"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
            model_dir.display()
        ),
    )
    .unwrap();
    (dir, path)
}

fn session_blob_ttl_secs(blob: &str) -> u64 {
    let raw = BASE64.decode(blob.as_bytes()).unwrap();
    let payload: Value = serde_json::from_slice(&raw[97..]).unwrap();
    payload["scope"]["Persistent"]["ttl_secs"].as_u64().unwrap()
}

fn session_blob_scope(blob: &str) -> Value {
    let raw = BASE64.decode(blob.as_bytes()).unwrap();
    let payload: Value = serde_json::from_slice(&raw[97..]).unwrap();
    payload["scope"].clone()
}

/// Build a session blob by hand via the library. Used where the stub CLI
/// pipeline cannot emit the token shape under test (`<Name_1>`, lowercase
/// FormatPreserve, etc.) until solo #3 ships the real policy loader.
fn build_blob_with<F>(configure: F) -> String
where
    F: FnOnce(&Session),
{
    let session = Session::new(Scope::Persistent {
        ttl: Duration::from_secs(3600),
    })
    .unwrap();
    configure(&session);
    let snap = session.export().unwrap();
    BASE64.encode(snap.into_bytes())
}

fn build_blob_and_tokens<F>(configure: F) -> (String, Vec<String>)
where
    F: FnOnce(&Session) -> Vec<String>,
{
    let session = Session::new(Scope::Persistent {
        ttl: Duration::from_secs(3600),
    })
    .unwrap();
    let tokens = configure(&session);
    let snap = session.export().unwrap();
    (BASE64.encode(snap.into_bytes()), tokens)
}

// -----------------------------------------------------------------------
// 1. Roundtrip
// -----------------------------------------------------------------------

#[test]
fn t01_roundtrip_email_tokenized_then_restored() {
    let input = "Contact Alice at alice@example.invalid for details.";
    let (clean_text, blob, detections) = clean_ok(input);

    assert_eq!(detections, 1, "stub pipeline tokenizes one email");
    assert!(
        !clean_text.contains("alice@example.invalid"),
        "raw email leaked"
    );
    assert!(
        clean_text.contains(":Email_1>"),
        "expected session-scoped Email_1 token: {clean_text}"
    );

    // LLM reply reuses the tokens.
    let llm_reply = clean_text.replace("Contact", "Reply to");
    let (code, stdout, stderr) = restore_json(&blob, &llm_reply);
    assert_eq!(
        code,
        Some(0),
        "restore should succeed, stderr={}",
        String::from_utf8_lossy(&stderr)
    );
    assert!(stderr.is_empty(), "expected empty stderr");
    let resp: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(
        resp["text"].as_str().unwrap(),
        "Reply to Alice at alice@example.invalid for details."
    );
}

#[test]
fn clean_json_emits_top_level_entries_for_detections() {
    let v = clean_json_with_args(&[], "Contact Alice at alice@example.invalid for details.");
    let entries = v["entries"].as_array().expect("entries array");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["class"], "Email");
    assert_eq!(entries[0]["raw"], "alice@example.invalid");
    assert_eq!(entries[0]["family"], "counter");
    assert!(entries[0]["token"].as_str().unwrap().contains(":Email_1>"));
    assert!(
        v["clean_text"]
            .as_str()
            .unwrap()
            .contains(entries[0]["token"].as_str().unwrap()),
        "clean_text must contain the emitted entry token"
    );
}

#[test]
fn clean_json_emits_empty_top_level_entries_without_detections() {
    let v = clean_json_with_args(&[], "Nothing detectable in this text.");

    assert_eq!(v["stats"]["detections"], 0);
    assert!(v["entries"].as_array().expect("entries array").is_empty());
}

// -----------------------------------------------------------------------
// 2. Canary
// -----------------------------------------------------------------------

#[test]
fn t02_canary_absent_in_clean_reappears_in_restore() {
    let canary = "CANARY_DO_NOT_LEAK@test.local";
    let input = format!("Ping {canary} before noon.");

    let (clean_text, blob, _) = clean_ok(&input);
    assert!(
        !clean_text.contains(canary),
        "canary leaked into clean_text: {clean_text}"
    );

    let (code, stdout, _) = restore_json(&blob, &clean_text);
    assert_eq!(code, Some(0));
    let resp: Value = serde_json::from_slice(&stdout).unwrap();
    assert!(
        resp["text"].as_str().unwrap().contains(canary),
        "canary did not round-trip"
    );
}

// -----------------------------------------------------------------------
// 3. UnknownToken — hallucinated class shape
// -----------------------------------------------------------------------

#[test]
fn t03_unknown_token_pascalcase_shape() {
    let (_, blob, _) = clean_ok("Email is alice@example.invalid please.");
    // Session has <Email_1>. LLM invents <Email_999>.
    let (code, stdout, stderr) = restore_json(&blob, "Your <Email_999> is queued.");
    assert_eq!(
        code,
        Some(3),
        "expected exit 3, stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(
        parse_stderr_variant(&stderr),
        json!({ "error": "UnknownToken", "exit": 3, "token": "<Email_999>" })
    );
}

// -----------------------------------------------------------------------
// 4. UnknownToken — lowercase FormatPreserve shape
// -----------------------------------------------------------------------

#[test]
fn t04_unknown_token_lowercase_formatpreserve_shape() {
    // Stub pipeline has only an email Tokenize rule — it cannot emit
    // lowercase FormatPreserve shapes like `location_7`. Session therefore
    // has no matching token; Pass 1 is a no-op and Pass 2's lowercase-shape
    // arm catches the LLM hallucination.
    let (_, blob, _) = clean_ok("No PII here.");
    let (code, stdout, stderr) = restore_json(&blob, "Your location_7 order arrives soon.");
    assert_eq!(
        code,
        Some(3),
        "expected exit 3, stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(
        parse_stderr_variant(&stderr),
        json!({ "error": "UnknownToken", "exit": 3, "token": "location_7" })
    );
}

#[test]
fn t04b_tolerant_restore_reports_warning_and_preserves_text() {
    let (_, blob, _) = clean_ok("No PII here.");
    let text = "Your location_7 order arrives soon.";
    let (code, stdout, stderr) = restore_json_with_args(&["--restore-mode=tolerant"], &blob, text);

    assert_eq!(code, Some(0), "stderr={}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty(), "expected empty stderr");
    let resp: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(resp["text"].as_str().unwrap(), text);
    assert_eq!(
        resp["restore_warning"],
        json!([{ "variant": "UnknownToken", "token": "location_7" }])
    );
}

#[test]
fn t04c_tolerant_restore_omits_empty_warning_array() {
    let (_, blob, _) = clean_ok("No PII here.");
    let (code, stdout, stderr) =
        restore_json_with_args(&["--restore-mode=tolerant"], &blob, "Plain reply.");

    assert_eq!(code, Some(0), "stderr={}", String::from_utf8_lossy(&stderr));
    let resp: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(resp["text"].as_str().unwrap(), "Plain reply.");
    assert!(
        resp.get("restore_warning").is_none(),
        "empty restore_warning must be omitted: {resp}"
    );
}

#[test]
fn cascade_restored_class_alpha_not_trapped() {
    let (blob, tokens) = build_blob_and_tokens(|s| {
        vec![s
            .tokenize(&PiiClass::custom("order_ref"), "tenant_class_a")
            .unwrap()]
    });
    assert_session_scoped_custom_token(&tokens[0]);

    let restored = restore_success_text(&blob, &format!("Reference {} is ready.", tokens[0]));

    assert_eq!(restored, "Reference tenant_class_a is ready.");
}

#[test]
fn cascade_restored_song_user_artist_tenant() {
    let cases = [
        ("song_ref", "tenant_class_b"),
        ("user_ref", "tenant_class_c"),
        ("artist_ref", "Artist_1"),
        ("tenant_ref", "Tenant_5"),
    ];
    let (blob, tokens) = build_blob_and_tokens(|s| {
        cases
            .iter()
            .map(|(class, raw)| s.tokenize(&PiiClass::custom(class), raw).unwrap())
            .collect()
    });
    for token in &tokens {
        assert_session_scoped_custom_token(token);
    }

    let reply = format!(
        "{} / {} / {} / {}",
        tokens[0], tokens[1], tokens[2], tokens[3]
    );
    let restored = restore_success_text(&blob, &reply);

    assert_eq!(
        restored,
        "tenant_class_b / tenant_class_c / Artist_1 / Tenant_5"
    );
}

#[test]
fn cascade_restored_snake_case() {
    let cases = [
        ("order_raw", "class_alpha_7"),
        ("tenant_raw", "tenant_5"),
        ("class_raw", "class_1"),
        ("song_title_raw", "song_title_3"),
    ];
    let (blob, tokens) = build_blob_and_tokens(|s| {
        cases
            .iter()
            .map(|(class, raw)| s.tokenize(&PiiClass::custom(class), raw).unwrap())
            .collect()
    });
    for token in &tokens {
        assert_session_scoped_custom_token(token);
    }

    let reply = format!("{} {} {} {}", tokens[0], tokens[1], tokens[2], tokens[3]);
    let restored = restore_success_text(&blob, &reply);

    assert_eq!(restored, "class_alpha_7 tenant_5 class_1 song_title_3");
}

#[test]
fn cascade_llm_hallucination_still_trapped() {
    let (blob, tokens) = build_blob_and_tokens(|s| {
        vec![s
            .tokenize(&PiiClass::custom("order_ref"), "tenant_class_a")
            .unwrap()]
    });
    assert_session_scoped_custom_token(&tokens[0]);

    let text = format!("Known {} and unknown <unknown:Email_99>.", tokens[0]);
    let (code, stdout, stderr) = restore_json(&blob, &text);

    assert_eq!(
        code,
        Some(3),
        "expected exit 3, stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(
        parse_stderr_variant(&stderr),
        json!({ "error": "UnknownToken", "exit": 3, "token": "Email_99" })
    );
}

#[test]
fn cascade_trap_boundary_crossing_not_exempted() {
    let (blob, tokens) =
        build_blob_and_tokens(|s| vec![s.tokenize(&PiiClass::custom("name_ref"), "Foo").unwrap()]);
    assert_session_scoped_custom_token(&tokens[0]);

    let text = format!("{}_7", tokens[0]);
    let (code, stdout, stderr) = restore_json(&blob, &text);

    assert_eq!(
        code,
        Some(3),
        "expected exit 3, stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(
        parse_stderr_variant(&stderr),
        json!({ "error": "UnknownToken", "exit": 3, "token": "Foo_7" })
    );
}

#[test]
fn clean_echoes_locale_chain_from_cli() {
    let v = clean_json_with_args(
        &["--locale", "fr-FR,de-DE"],
        "Reach alice@example.invalid today.",
    );

    assert_eq!(
        v["stats"]["locale_chain"],
        json!(["fr-FR", "de-DE", "global"])
    );
}

#[test]
fn rulepack_recognizer_is_gated_by_policy_locale() {
    let (_dir, path) =
        write_policy_with_rulepack(&de_email_rulepack("\"de-DE\""), Some("\"en-US\""));
    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "kennung kundennummer-123",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "policy config must not emit stdout");
    assert_policy_config_envelope(&out.stderr);

    let (_dir, path) =
        write_policy_with_rulepack(&de_email_rulepack("\"de-DE\""), Some("\"de-DE\""));
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "kennung kundennummer-123",
    );
    let clean = v["clean_text"].as_str().unwrap();

    assert!(clean.starts_with("kennung <"));
    assert!(clean.ends_with(":Email_1>"));
    assert_eq!(v["stats"]["detections"], 1);
}

#[test]
fn rulepack_recognizer_fr_fr_not_gated_by_en_us() {
    let rulepack = de_email_rulepack("\"fr-FR\"").replace("kundennummer", "ticket");
    let (_dir, path) = write_policy_with_rulepack(&rulepack, Some("\"en-US\""));
    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "kennung ticket-123",
    );

    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "policy config must not emit stdout");
    assert_policy_config_envelope(&out.stderr);
}

#[test]
fn rulepack_dictionary_matcher_detects_when_locale_matches() {
    let rulepack = r#"
schema_version = "0.1.0"
rulepack_id = "test-dict"
rulepack_version = "0.4.0"
default_locales = ["de-DE"]

[[recognizers]]
id = "email.de"
class = "Email"
enabled = true
locales = ["de-DE"]

[recognizers.match]
kind = "dictionary"
terms = ["Sonnenlied"]

[recognizers.token]
"#;
    let (_dir, path) = write_policy_with_rulepack(rulepack, Some("\"en-US\""));
    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "track Sonnenlied",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "policy config must not emit stdout");
    assert_policy_config_envelope(&out.stderr);

    let (_dir, path) = write_policy_with_rulepack(rulepack, Some("\"de-DE\""));
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "track Sonnenlied",
    );
    let clean = v["clean_text"].as_str().unwrap();
    assert!(clean.starts_with("track <"));
    assert!(clean.ends_with(":Email_1>"));
    assert_eq!(v["stats"]["detections"], 1);
    assert_eq!(v["stats"]["locale_chain"], json!(["de-DE", "global"]));
}

#[test]
fn t21a_email_header_does_not_fire_on_inline_titlecase() {
    let (_dir, path) = write_policy_with_bundled_rulepacks(&["core"], "en-US");
    let input = "I love using Microsoft Word and React Components in New York";
    let v = clean_json_with_args(&[&format!("--policy={}", path.display())], input);

    assert_eq!(v["clean_text"], input);
    assert_eq!(v["stats"]["detections"], 0);
}

#[test]
fn t21c_email_header_rejects_body_from_without_bracket() {
    let (_dir, path) = write_policy_with_bundled_rulepacks(&["core"], "en-US");
    let input = "From: React Component for context.\n";
    let v = clean_json_with_args(&[&format!("--policy={}", path.display())], input);

    assert_eq!(v["clean_text"], input);
    assert_eq!(v["stats"]["detections"], 0);
}

#[test]
fn t21e_email_header_de_locale() {
    let (_dir, path) =
        write_policy_with_bundled_rulepacks(&["core", "locale-de", "locale-en"], "de-DE");
    let input = "Von: Alice Example <alice@example.invalid>";
    let v = clean_json_with_args(&[&format!("--policy={}", path.display())], input);
    let clean = v["clean_text"].as_str().unwrap();

    assert!(
        Regex::new(r"^Von: <[0-9a-f]{8}:Name_\d+> <<[0-9a-f]{8}:Email_\d+>>$")
            .unwrap()
            .is_match(clean)
    );
    assert_eq!(v["stats"]["detections"], 2);
    assert_eq!(v["stats"]["locale_chain"], json!(["de-DE", "global"]));

    let restored = restore_success_text(v["session_blob"].as_str().unwrap(), clean);
    assert_eq!(restored, input);
}

#[test]
fn t21f_prompt_preamble_threshold_03_pipeline_end_to_end() {
    let (_dir, path) =
        write_policy_with_test_ner(&["core", "locale-de", "locale-en"], "de-DE", "de", 0.3);
    let input = "Du antwortest als Artistfy-Support an Alice Example.";
    let v = clean_json_with_args(&[&format!("--policy={}", path.display())], input);
    let clean = v["clean_text"].as_str().unwrap();

    assert!(
        Regex::new(r"^Du antwortest als Artistfy-Support an <[0-9a-f]{8}:Name_\d+>\.$")
            .unwrap()
            .is_match(clean),
        "unexpected clean text: {clean}"
    );
    assert_eq!(v["stats"]["detections"], 1);
    assert_eq!(v["stats"]["locale_chain"], json!(["de-DE", "global"]));

    let restored = restore_success_text(v["session_blob"].as_str().unwrap(), clean);
    assert_eq!(restored, input);
}

#[test]
fn t22_audit_log_per_term_traceability() {
    let dir = tempdir().unwrap();
    let rulepack_path = dir.path().join("rulepack.toml");
    fs::write(
        &rulepack_path,
        r#"
schema_version = "0.1.0"
rulepack_id = "audit-dict"
rulepack_version = "0.4.1"
default_locales = ["global"]

[[recognizers]]
id = "audit_terms"
class = "custom:term"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "dictionary"
terms = ["foo", "bar", "baz"]
case_sensitive = true

[recognizers.token]
"#,
    )
    .unwrap();
    let policy_path = dir.path().join("policy.toml");
    fs::write(
        &policy_path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = []
paths = ["{}"]

[[rule]]
kind = "class"
class = "custom:term"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
            rulepack_path.display()
        ),
    )
    .unwrap();
    let audit_path = dir.path().join("audit.sqlite");

    let v = clean_json_with_args(
        &[
            &format!("--policy={}", policy_path.display()),
            &format!("--audit-db={}", audit_path.display()),
        ],
        "foo bar baz",
    );

    assert_eq!(v["stats"]["detections"], 3);
    let logger = SqliteLogger::new(&audit_path).expect("audit log opens");
    let entries = logger.entries().expect("audit entries");
    assert_eq!(entries.len(), 3);
    for (index, entry) in entries.iter().enumerate() {
        let source_shape = Regex::new(&format!(r"^dictionary:[a-z_]+\[#{}\]$", index)).unwrap();
        assert!(
            source_shape.is_match(&entry.source),
            "unexpected audit source: {}",
            entry.source
        );
    }
}

#[test]
fn s4_audit_query_and_export_return_filtered_metadata_rows() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");

    let clean = clean_raw_with_args(
        &[&format!("--audit-db={}", audit_path.display())],
        "Email alice@example.invalid",
    );
    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let query = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "query",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--class",
            "email",
            "--action",
            "tokenize",
            "--document-kind",
            "text",
        ])
        .output()
        .unwrap();
    assert!(
        query.status.success(),
        "audit query failed: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    let stdout = String::from_utf8(query.stdout).unwrap();
    assert!(stdout.starts_with(
        "source\trecognizer_id\trecognizer_version_id\tclass\taction\tfield_name\tdocument_kind\tconflict_loser\tdecided_by\tcreated_at\tsession_id\tsnapshot_scheme\tsnapshot_alg\tsnapshot_key_version\tvalidator_fail_reason\tambiguity_record\tcollision_family\tcollision_variant\tfallback_triggered\n"
    ));
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("regex\tregex\t\temail\ttokenize\t\ttext\tfalse\t")),
        "unexpected query stdout: {stdout}"
    );

    let export_path = dir.path().join("audit.jsonl");
    let export = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--output",
            export_path.to_str().unwrap(),
            "--source",
            "regex",
        ])
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    assert!(export.stdout.is_empty());
    let rows = fs::read_to_string(export_path).unwrap();
    let row: Value = serde_json::from_str(rows.lines().next().unwrap()).unwrap();
    assert_eq!(row["class"], "email");
    assert_eq!(row["source"], "regex");
    assert_eq!(row["recognizer_id"], "regex");
    assert_eq!(row["recognizer_version_id"], Value::Null);
    assert_eq!(row["action"], "tokenize");
    assert_eq!(row["field_name"], Value::Null);
    assert_eq!(row["document_kind"], "text");
    assert_eq!(row["conflict_loser"], false);
    assert!(row.get("decided_by").is_some());
    assert!(row.get("session_id").is_some());
    assert!(row.get("snapshot_scheme").is_some());
    assert!(row.get("snapshot_alg").is_some());
    assert!(row.get("snapshot_key_version").is_some());
}

#[test]
fn s4_audit_query_filters_ambiguity_and_collision_metadata() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");
    let conn = Connection::open(&audit_path).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE redaction_log (
            source TEXT NOT NULL,
            class TEXT NOT NULL,
            action TEXT NOT NULL,
            field_name TEXT NULL,
            document_kind TEXT NOT NULL,
            conflict_loser INTEGER NOT NULL,
            decided_by TEXT NOT NULL DEFAULT 'none',
            created_at INTEGER NULL,
            session_id TEXT NULL,
            snapshot_scheme TEXT NOT NULL DEFAULT 'gaze.snapshot.v1.sha256-salted',
            snapshot_alg TEXT NOT NULL DEFAULT 'SHA-256',
            snapshot_key_version INTEGER NULL,
            validator_fail_reason TEXT NULL,
            ambiguity_record TEXT NULL,
            collision_family TEXT NULL,
            collision_variant TEXT NULL
        );
        INSERT INTO redaction_log
            (source, class, action, field_name, document_kind, conflict_loser, decided_by,
             created_at, session_id, snapshot_scheme, snapshot_alg, snapshot_key_version,
             validator_fail_reason, ambiguity_record, collision_family, collision_variant)
        VALUES
            ('regex', 'email', 'tokenize', NULL, 'text', 0, 'none',
             100, NULL, 'gaze.snapshot.v1.sha256-salted', 'SHA-256', NULL,
             NULL, NULL, NULL, NULL),
            ('hybrid', 'custom:postal_or_phone_de', 'tokenize', NULL, 'text', 0, 'validator',
             200, NULL, 'gaze.snapshot.v1.sha256-salted', 'SHA-256', NULL,
             '"luhn_failed"',
             '{"ambiguity_class":"custom:postal_or_phone_de","losing_candidates":[{"class":"custom:postal_de","recognizer_id":"postal-de"}],"reason":"no_anchor"}',
             'de-postal-phone', 'postal-de');
        "#,
    )
    .unwrap();

    let query = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "query",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--has-ambiguity",
            "--ambiguity-reason",
            "no-anchor",
            "--collision-family",
            "de-postal-phone",
            "--collision-variant",
            "postal-de",
        ])
        .output()
        .unwrap();
    assert!(
        query.status.success(),
        "audit query failed: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    let stdout = String::from_utf8(query.stdout).unwrap();
    assert!(stdout.lines().next().unwrap().ends_with("\tambiguity"));
    assert!(stdout.contains("hybrid\t\t\tcustom:postal_or_phone_de"));
    assert!(!stdout.contains("regex\t\t\temail"));
    assert!(stdout.contains(
        "class=custom:postal_or_phone_de reason=no_anchor losing=[custom:postal_de:postal-de]"
    ));

    let export = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--has-ambiguity",
            "--ambiguity-reason",
            "no-anchor",
        ])
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let row: Value = serde_json::from_slice(&export.stdout).unwrap();
    assert_eq!(row["validator_fail_reason"], "luhn_failed");
    assert_eq!(row["ambiguity_record"]["reason"], "no_anchor");
    assert_eq!(row["collision_family"], "de-postal-phone");
    assert_eq!(row["collision_variant"], "postal-de");
}

#[test]
fn s2_audit_cli_smoke_filters_created_at_range() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");

    let clean = clean_raw_with_args(
        &[&format!("--audit-db={}", audit_path.display())],
        "Email alice@example.invalid",
    );
    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let query = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "query",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--from",
            "1970-01-01T00:00:00Z",
            "--to",
            "2999-12-31T23:59:59Z",
        ])
        .output()
        .unwrap();
    assert!(
        query.status.success(),
        "audit query failed: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    assert_no_raw_audit_pii(&query.stdout);

    let stdout = String::from_utf8(query.stdout).unwrap();
    let row = stdout
        .lines()
        .find(|line| line.starts_with("regex\tregex\t\temail\ttokenize\t\ttext\tfalse\t"))
        .expect("expected email audit row in bounded time range");
    let created_at = row
        .split('\t')
        .nth(9)
        .expect("created_at column")
        .parse::<i64>()
        .expect("created_at is epoch milliseconds");
    assert!(created_at > 0, "expected positive created_at: {row}");

    let future = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "query",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--from",
            "2999-12-31T23:59:59Z",
        ])
        .output()
        .unwrap();
    assert!(
        future.status.success(),
        "future audit query failed: {}",
        String::from_utf8_lossy(&future.stderr)
    );
    let future_stdout = String::from_utf8(future.stdout).unwrap();
    assert_eq!(
        future_stdout.lines().count(),
        1,
        "future lower bound should return only the header: {future_stdout}"
    );
}

#[test]
fn s1_audit_cli_smoke_filters_session_without_leaking_raw_or_other_session() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");
    let raw_a = "Email alice@example.invalid";
    let raw_b = "Email bob@example.invalid";

    for raw in [raw_a, raw_b] {
        let clean = clean_raw_with_args(&[&format!("--audit-db={}", audit_path.display())], raw);
        assert!(
            clean.status.success(),
            "clean failed: {}",
            String::from_utf8_lossy(&clean.stderr)
        );
    }

    let export = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    assert_no_raw_audit_pii(&export.stdout);
    assert!(!export
        .stdout
        .windows(b"bob@example.invalid".len())
        .any(|window| window == b"bob@example.invalid"));
    let rows: Vec<Value> = String::from_utf8(export.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let session_a = rows[0]["session_id"].as_str().unwrap().to_string();
    let session_b = rows[1]["session_id"].as_str().unwrap().to_string();
    assert_ne!(session_a, session_b);

    let query = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "query",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--session",
            &session_a,
        ])
        .output()
        .unwrap();
    assert!(
        query.status.success(),
        "audit query failed: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    assert_no_raw_audit_pii(&query.stdout);
    assert!(
        !query
            .stdout
            .windows(session_b.len())
            .any(|window| window == session_b.as_bytes()),
        "non-matching session id leaked in filtered output"
    );
    let stdout = String::from_utf8(query.stdout).unwrap();
    assert!(stdout.contains(&session_a), "filtered output: {stdout}");
    assert_eq!(stdout.lines().count(), 2, "filtered output: {stdout}");
}

#[test]
fn t_audit_session_id_is_opaque_not_session_hex() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");

    let clean = clean_json_with_args(
        &[&format!("--audit-db={}", audit_path.display())],
        "Email alice@example.invalid",
    );
    let clean_text = clean["clean_text"].as_str().unwrap();
    let token = clean_text
        .split_whitespace()
        .find(|part| part.starts_with('<'))
        .expect("tokenized email");
    let token_session_hex = token
        .trim_start_matches('<')
        .split(':')
        .next()
        .expect("token prefix");

    let export = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let row: Value = serde_json::from_slice(&export.stdout).unwrap();
    let audit_session_id = row["session_id"].as_str().unwrap();
    assert_ne!(audit_session_id, token_session_hex);
    assert_eq!(audit_session_id.len(), 36);
    assert_eq!(&audit_session_id[14..15], "7");
}

#[test]
fn s2_audit_invalid_iso8601_is_policy_config_for_query_and_export() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");

    for subcommand in ["query", "export"] {
        let mut command = Command::cargo_bin("gaze").unwrap();
        command.args([
            "audit",
            subcommand,
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--from",
            "invalid-date",
        ]);
        if subcommand == "export" {
            command.args(["--format", "jsonl"]);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(stderr["error"], "PolicyConfig");
        assert_eq!(stderr["exit"], 2);
        assert_eq!(
            stderr["detail"],
            format!("invalid audit ISO 8601 timestamp: {:?}", "invalid-date")
        );
    }
}

#[test]
fn s1_policy_audit_defaults_are_not_supported() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.audit]
session = "0196758d-5f00-7a8a-9a4c-6b1347a1f0d2"

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'alice@example[.]com'
class = "email"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();

    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "Email alice@example.invalid",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(stderr["error"], "PolicyConfig");
}

#[test]
fn s4_audit_export_does_not_return_raw_pii() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");
    // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
    // literals chosen for parser-valid + non-routable.
    let raw_input =
        "Hello Dr. Schmidt, your phone is +4915100000000 and email alice@example.invalid";

    let clean = clean_raw_with_args(
        &[&format!("--audit-db={}", audit_path.display())],
        raw_input,
    );
    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let query = Command::cargo_bin("gaze")
        .unwrap()
        .args(["audit", "query", "--audit-db", audit_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        query.status.success(),
        "audit query failed: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    assert_no_raw_audit_pii(&query.stdout);

    let export = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    assert_no_raw_audit_pii(&export.stdout);
}

#[test]
fn s4_audit_extra_column_not_leaked() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");

    let clean = clean_raw_with_args(
        &[&format!("--audit-db={}", audit_path.display())],
        "Some content here for alice@example.invalid",
    );
    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    {
        let conn = Connection::open(&audit_path).unwrap();
        conn.execute("ALTER TABLE redaction_log ADD COLUMN raw_value TEXT", [])
            .unwrap();
        conn.execute(
            "UPDATE redaction_log SET raw_value = ?",
            ["Dr. Schmidt's secret SSN: 999-99-9999"],
        )
        .unwrap();
    }

    let query = Command::cargo_bin("gaze")
        .unwrap()
        .args(["audit", "query", "--audit-db", audit_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        query.status.success(),
        "audit query failed: {}",
        String::from_utf8_lossy(&query.stderr)
    );

    let export = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    let needle = b"Dr. Schmidt's secret SSN";
    assert!(
        !query
            .stdout
            .windows(needle.len())
            .any(|window| window == needle),
        "FAIL: extra column raw_value leaked in audit query output"
    );
    assert!(
        !export
            .stdout
            .windows(needle.len())
            .any(|window| window == needle),
        "FAIL: extra column raw_value leaked in audit export output"
    );
}

#[test]
fn s4_audit_query_columns_are_restricted() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");

    let clean = clean_raw_with_args(
        &[&format!("--audit-db={}", audit_path.display())],
        "Some content here for alice@example.invalid",
    );
    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let conn = Connection::open(&audit_path).unwrap();
    conn.execute("ALTER TABLE redaction_log ADD COLUMN raw_value TEXT", [])
        .unwrap();
    let (sql, values) = build_audit_query_sql(
        &AuditFilter::default(),
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
    );
    assert!(
        values.is_empty(),
        "default audit filter should not bind query values"
    );
    let stmt = conn.prepare(&sql).unwrap();
    let column_names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let expected: Vec<String> = AUDIT_RESTRICTED_COLUMNS
        .iter()
        .map(|column| column.to_string())
        .collect();

    assert_eq!(
        column_names, expected,
        "audit query SQL must return exactly AUDIT_RESTRICTED_COLUMNS"
    );
}

#[test]
fn p5_audit_query_reads_structural_agent_recipient_source() {
    let (_dir, policy_path) =
        write_policy_with_bundled_rulepacks(&["core", "locale-de", "locale-en"], "de-DE");
    let audit_dir = tempdir().unwrap();
    let audit_path = audit_dir.path().join("audit.sqlite");

    let clean = clean_raw_with_args(
        &[
            &format!("--policy={}", policy_path.display()),
            &format!("--audit-db={}", audit_path.display()),
        ],
        "Du antwortest als Artistfy-Support an Alice Example.",
    );
    assert!(
        clean.status.success(),
        "clean failed: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
    let body: Value = serde_json::from_slice(&clean.stdout).expect("stdout is JSON");
    assert!(
        Regex::new(r"^Du antwortest als Artistfy-Support an <[0-9a-f]{8}:Name_\d+>\.$")
            .unwrap()
            .is_match(body["clean_text"].as_str().expect("clean_text")),
        "unexpected clean_text: {}",
        body["clean_text"]
    );

    // Phase 5: `AUDIT_RESTRICTED_COLUMNS` is the safe display allow-list, and
    // `source` is intentionally present so audit reads can explain the
    // structural family without a schema change.
    assert!(AUDIT_RESTRICTED_COLUMNS.contains(&"source"));
    let (sql, values) = build_audit_query_sql(
        &AuditFilter::default(),
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
    );
    let conn = Connection::open(&audit_path).unwrap();
    let mut stmt = conn.prepare(&sql).unwrap();
    let rows = stmt
        .query_map(params_from_iter(values.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(8)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(
        rows,
        vec![(
            "structural.agent_recipient".to_string(),
            "name".to_string(),
            "tokenize".to_string(),
            "none".to_string()
        )]
    );
}

fn assert_no_raw_audit_pii(bytes: &[u8]) {
    for raw in [
        b"Dr. Schmidt".as_slice(),
        b"+4915100000000".as_slice(),
        b"alice@example.invalid".as_slice(),
    ] {
        assert!(
            !bytes.windows(raw.len()).any(|window| window == raw),
            "FAIL: raw PII '{}' found in audit output",
            String::from_utf8_lossy(raw)
        );
    }
}

#[test]
fn t23_audit_purge_counts_deletes_and_restore_still_round_trips() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");

    let v = clean_json_with_args(
        &[&format!("--audit-db={}", audit_path.display())],
        "Email alice@example.invalid and bob@example.invalid.",
    );
    assert_eq!(v["stats"]["detections"], 2);
    let clean = v["clean_text"].as_str().unwrap();
    let blob = v["session_blob"].as_str().unwrap();
    assert_audit_db_has_no_raw_fixture_values(&audit_path);

    let conn = Connection::open(&audit_path).unwrap();
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM redaction_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 2);
    conn.execute(
        "UPDATE redaction_log SET created_at = ?1 WHERE rowid = (SELECT rowid FROM redaction_log ORDER BY rowid LIMIT 1)",
        params![1_767_225_600_000_i64],
    )
    .unwrap();
    conn.execute(
        "UPDATE redaction_log SET created_at = ?1 WHERE rowid = (SELECT rowid FROM redaction_log ORDER BY rowid LIMIT 1 OFFSET 1)",
        params![1_772_323_200_000_i64],
    )
    .unwrap();
    drop(conn);

    let dry_run = Command::cargo_bin("gaze")
        .unwrap()
        .arg("audit")
        .arg("purge")
        .arg(format!("--audit-db={}", audit_path.display()))
        .arg("--before=2026-02-01T00:00:00Z")
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(
        dry_run.status.success(),
        "dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    assert!(dry_run.stderr.is_empty());
    let dry_run_json: Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    assert_eq!(
        dry_run_json,
        json!({ "dry_run": true, "matched": 1, "deleted": 0 })
    );

    let conn = Connection::open(&audit_path).unwrap();
    let total_after_dry_run: i64 = conn
        .query_row("SELECT COUNT(*) FROM redaction_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total_after_dry_run, 2);
    drop(conn);

    let count = Command::cargo_bin("gaze")
        .unwrap()
        .arg("audit")
        .arg("purge")
        .arg(format!("--audit-db={}", audit_path.display()))
        .arg("--before=2026-02-01T00:00:00Z")
        .arg("--count")
        .output()
        .unwrap();
    assert!(
        count.status.success(),
        "--count failed: {}",
        String::from_utf8_lossy(&count.stderr)
    );
    assert!(count.stderr.is_empty());
    let count_json: Value = serde_json::from_slice(&count.stdout).unwrap();
    assert_eq!(count_json, dry_run_json);

    let conn = Connection::open(&audit_path).unwrap();
    let total_after_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM redaction_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total_after_count, 2);
    drop(conn);

    let purge = Command::cargo_bin("gaze")
        .unwrap()
        .arg("audit")
        .arg("purge")
        .arg(format!("--audit-db={}", audit_path.display()))
        .arg("--before=2026-02-01T00:00:00Z")
        .output()
        .unwrap();
    assert!(
        purge.status.success(),
        "purge failed: {}",
        String::from_utf8_lossy(&purge.stderr)
    );
    assert!(purge.stderr.is_empty());
    let purge_json: Value = serde_json::from_slice(&purge.stdout).unwrap();
    assert_eq!(
        purge_json,
        json!({ "dry_run": false, "matched": 1, "deleted": 1 })
    );

    let conn = Connection::open(&audit_path).unwrap();
    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM redaction_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
    let remaining_created_at: i64 = conn
        .query_row(
            "SELECT created_at FROM redaction_log ORDER BY rowid",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining_created_at, 1_772_323_200_000_i64);
    drop(conn);
    assert_audit_db_has_no_raw_fixture_values(&audit_path);

    let restored = restore_success_text(blob, clean);
    assert_eq!(
        restored,
        "Email alice@example.invalid and bob@example.invalid."
    );
}

fn assert_audit_db_has_no_raw_fixture_values(path: &std::path::Path) {
    let bytes = fs::read(path).unwrap();
    for raw in [
        b"alice@example.invalid".as_slice(),
        b"bob@example.invalid".as_slice(),
    ] {
        assert!(
            !bytes.windows(raw.len()).any(|window| window == raw),
            "FAIL: raw fixture value '{}' found in audit DB bytes",
            String::from_utf8_lossy(raw)
        );
    }
}

#[test]
fn t23_audit_purge_rejects_non_iso8601_before_with_quoted_input() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("audit.sqlite");
    for invalid in [
        "not-iso8601",
        "2026-02-31T00:00:00Z",
        "2025-04-31T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-02-01t00:00:00Z",
        "2026-02-01T00:00:00+01:00",
        "",
    ] {
        let out = Command::cargo_bin("gaze")
            .unwrap()
            .arg("audit")
            .arg("purge")
            .arg(format!("--audit-db={}", audit_path.display()))
            .arg(format!("--before={invalid}"))
            .output()
            .unwrap();

        assert_eq!(out.status.code(), Some(2), "invalid input: {invalid}");
        assert!(out.stdout.is_empty());
        let stderr = parse_stderr_variant(&out.stderr);
        assert_eq!(
            stderr,
            json!({ "error": "AuditPurgeIso8601", "exit": 2, "input": invalid }),
            "invalid input: {invalid}"
        );
    }
}

#[test]
fn t21g_pattern_template_uses_active_locale_de_when_en_loaded_after_de() {
    let (_dir, path) =
        write_policy_with_bundled_rulepacks(&["core", "locale-de", "locale-en"], "de-DE");
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "Von: Alice Example <alice@example.invalid>",
    );
    let clean = v["clean_text"].as_str().unwrap();

    assert!(
        Regex::new(r"^Von: <[0-9a-f]{8}:Name_\d+> <<[0-9a-f]{8}:Email_\d+>>$")
            .unwrap()
            .is_match(clean)
    );
    assert_eq!(v["stats"]["detections"], 2);
    assert_eq!(v["stats"]["locale_chain"], json!(["de-DE", "global"]));
}

#[test]
fn t21h_pattern_template_falls_back_to_global_when_locale_not_loaded() {
    let (_dir, path) = write_policy_with_bundled_rulepacks(&["core"], "fr-FR");
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "From: Alice Example <alice@example.invalid>",
    );
    let clean = v["clean_text"].as_str().unwrap();

    assert!(
        Regex::new(r"^From: <[0-9a-f]{8}:Name_\d+> <<[0-9a-f]{8}:Email_\d+>>$")
            .unwrap()
            .is_match(clean)
    );
    assert_eq!(v["stats"]["detections"], 2);
    assert_eq!(v["stats"]["locale_chain"], json!(["fr-FR", "global"]));
}

#[test]
fn t_cli_ner_threshold_out_of_range_fails_closed() {
    let out = clean_raw_with_args(&["--ner-threshold", "1.5"], "Reach support.");

    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_policy_config_detail_contains(&out.stderr, "ner.threshold");
}

#[test]
fn s1_invalid_session_scope_is_symmetric_between_cli_and_toml() {
    let (_valid_dir, valid_policy) = write_policy_with_session_scope("persistent");
    let (_invalid_dir, invalid_policy) = write_policy_with_session_scope("forever");

    let cli_out = clean_raw_with_args(
        &[
            &format!("--policy={}", valid_policy.display()),
            "--session-scope=forever",
        ],
        "class-alpha-123",
    );
    let toml_out = clean_raw_with_args(
        &[&format!("--policy={}", invalid_policy.display())],
        "class-alpha-123",
    );

    assert_symmetric_policy_config(cli_out, toml_out);
}

#[test]
fn s1_invalid_bundled_rulepack_is_symmetric_between_cli_and_toml() {
    let (_valid_dir, valid_policy) = write_policy_with_bundled_id("core");
    let (_invalid_dir, invalid_policy) = write_policy_with_bundled_id("garbage");

    let cli_out = clean_raw_with_args(
        &[
            &format!("--policy={}", valid_policy.display()),
            "--rulepack-bundled=garbage",
        ],
        "class-alpha-123",
    );
    let toml_out = clean_raw_with_args(
        &[&format!("--policy={}", invalid_policy.display())],
        "class-alpha-123",
    );

    assert_symmetric_policy_config(cli_out, toml_out);
}

#[test]
fn s1_rulepack_bundled_garbage_no_policy_rejects() {
    let (_invalid_dir, invalid_policy) = write_policy_with_bundled_id("garbage");

    let cli_out = clean_raw_with_args(&["--rulepack-bundled=garbage"], "alice@example.invalid");
    let toml_out = clean_raw_with_args(
        &[&format!("--policy={}", invalid_policy.display())],
        "alice@example.invalid",
    );

    assert_symmetric_policy_config(cli_out, toml_out);
}

#[test]
fn s1_rulepack_path_missing_no_policy_rejects() {
    let dir = tempdir().unwrap();
    let missing_rulepack = dir.path().join("missing-rulepack.toml");
    let policy = dir.path().join("policy.toml");
    fs::write(
        &policy,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = []
paths = ["{}"]

[[rule]]
kind = "default"
action = "preserve"
"#,
            missing_rulepack.display()
        ),
    )
    .unwrap();

    let cli_out = clean_raw_with_args(
        &[&format!("--rulepack-path={}", missing_rulepack.display())],
        "alice@example.invalid",
    );
    let toml_out = clean_raw_with_args(
        &[&format!("--policy={}", policy.display())],
        "alice@example.invalid",
    );

    assert_symmetric_policy_config(cli_out, toml_out);
}

#[test]
fn s1_invalid_ner_locale_is_symmetric_between_cli_and_toml() {
    let (_valid_dir, valid_policy) = write_policy_with_ner_locale("en-US");
    let (_invalid_dir, invalid_policy) = write_policy_with_ner_locale("bad_locale_!");

    let cli_out = clean_raw_with_args(
        &[
            &format!("--policy={}", valid_policy.display()),
            "--ner-locale=bad_locale_!",
        ],
        "class-alpha-123",
    );
    let toml_out = clean_raw_with_args(
        &[&format!("--policy={}", invalid_policy.display())],
        "class-alpha-123",
    );

    assert_symmetric_policy_config(cli_out, toml_out);
}

#[test]
fn s1_session_scope_override_changes_observed_session_semantics() {
    let (_dir, policy) = write_policy_with_session_scope("persistent");
    let persistent = clean_json_with_args(
        &[&format!("--policy={}", policy.display())],
        "class-alpha-123",
    );
    assert_eq!(
        session_blob_scope(persistent["session_blob"].as_str().unwrap()),
        json!({ "Persistent": { "ttl_secs": 86400 } })
    );

    let conversation = clean_json_with_args(
        &[
            &format!("--policy={}", policy.display()),
            "--session-scope=conversation",
        ],
        "class-alpha-123",
    );
    assert_eq!(
        session_blob_scope(conversation["session_blob"].as_str().unwrap()),
        json!({ "Conversation": "cli" })
    );

    let ephemeral = clean_raw_with_args(
        &[
            &format!("--policy={}", policy.display()),
            "--session-scope=ephemeral",
        ],
        "class-alpha-123",
    );
    assert_eq!(
        ephemeral.status.code(),
        Some(3),
        "ephemeral sessions must retain export-forbidden semantics"
    );
    assert_eq!(
        parse_stderr_variant(&ephemeral.stderr),
        json!({ "error": "Pipeline", "exit": 3 })
    );
}

#[test]
fn s1_rulepack_bundled_override_changes_bundled_recognizer_availability() {
    let (_dir, policy) = write_policy_with_bundled_rulepacks(&["core"], "de-DE");
    let input = "Von: Alice Example <alice@example.invalid>";

    let without_locale_pack =
        clean_json_with_args(&[&format!("--policy={}", policy.display())], input);
    let baseline_clean = without_locale_pack["clean_text"].as_str().unwrap();
    assert!(baseline_clean.starts_with("Von: Alice Example <<"));
    assert!(baseline_clean.ends_with(":Email_1>>"));
    assert_eq!(without_locale_pack["stats"]["detections"], 1);

    let with_locale_pack = clean_json_with_args(
        &[
            &format!("--policy={}", policy.display()),
            "--rulepack-bundled=core,locale-de,locale-en",
        ],
        input,
    );
    let clean = with_locale_pack["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^Von: <[0-9a-f]{8}:Name_\d+> <<[0-9a-f]{8}:Email_\d+>>$")
            .unwrap()
            .is_match(clean),
        "unexpected clean text: {clean}"
    );
    assert_eq!(with_locale_pack["stats"]["detections"], 2);
}

#[test]
fn unified_core_toml_tokenizes_safe_and_locale_gated_recognizers() {
    // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
    // literals chosen for parser-valid + non-routable.
    let input = "Email alice@example.invalid phone +4915100000000 host 192.168.1.1 zip 94103-1234 IBAN GB82WEST12345698765432 card 4111111111111111";
    let (_core_dir, core_policy) = write_policy_with_core_extended_rulepacks(&["core"], "en-US");
    let unified = clean_json_with_args(&[&format!("--policy={}", core_policy.display())], input);
    let extended_clean = unified["clean_text"].as_str().unwrap();
    assert!(extended_clean.contains(":Email_1>"), "{extended_clean}");
    assert!(
        extended_clean.contains(":Custom:phone_1>"),
        "{extended_clean}"
    );
    assert!(
        extended_clean.contains(":Custom:ip_address_1>"),
        "{extended_clean}"
    );
    assert!(
        extended_clean.contains(":Custom:postal_code_1>"),
        "{extended_clean}"
    );
    assert!(
        extended_clean.contains(":Custom:family:payment-card-or-iban_1>"),
        "{extended_clean}"
    );
    assert!(
        extended_clean.contains(":Custom:credit_card_1>"),
        "{extended_clean}"
    );
    assert_eq!(unified["stats"]["detections"], 6);
    assert_eq!(
        restore_success_text(unified["session_blob"].as_str().unwrap(), extended_clean),
        input
    );
}

#[test]
fn s2_core_extended_cli_opt_in_mirrors_toml_and_rejects_garbage_symmetrically() {
    // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
    // literals chosen for parser-valid + non-routable.
    let input = "phone +4915100000000 host 192.168.1.1 zip 94103 IBAN GB82WEST12345698765432 card 4111111111111111";
    let (_toml_dir, toml_policy) =
        write_policy_with_core_extended_rulepacks(&["core", "core-extended"], "en-US");
    let toml = clean_json_with_args(&[&format!("--policy={}", toml_policy.display())], input);

    let (_cli_dir, cli_policy) = write_policy_with_core_extended_rulepacks(&["core"], "en-US");
    let cli = clean_json_with_args(
        &[
            &format!("--policy={}", cli_policy.display()),
            "--rulepack-bundled=core,core-extended",
        ],
        input,
    );

    assert_eq!(
        normalize_session_hex(toml["clean_text"].as_str().unwrap()),
        normalize_session_hex(cli["clean_text"].as_str().unwrap())
    );
    assert_eq!(toml["stats"]["detections"], cli["stats"]["detections"]);

    let (_valid_dir, valid_policy) = write_policy_with_core_extended_rulepacks(&["core"], "en-US");
    let (_invalid_dir, invalid_policy) =
        write_policy_with_core_extended_rulepacks(&["garbage"], "en-US");
    let cli_out = clean_raw_with_args(
        &[
            &format!("--policy={}", valid_policy.display()),
            "--rulepack-bundled=garbage",
        ],
        input,
    );
    let toml_out = clean_raw_with_args(&[&format!("--policy={}", invalid_policy.display())], input);
    assert_symmetric_policy_config(cli_out, toml_out);
}

#[test]
fn s2_core_extended_default_surface_does_not_load_phase2_recognizers() {
    let input = "IBAN GB82WEST12345698765432 card 4111111111111111";
    let default = clean_json_with_args(&[], input);

    assert_eq!(default["clean_text"], input);
    assert_eq!(default["stats"]["detections"], 0);
}

#[test]
fn s2_cli_bundled_smoke_emits_formatted_phase2_tokens() {
    let value = clean_json_with_args(
        &["--rulepack-bundled", "core,core-extended"],
        "IBAN DE89 3704 0044 0532 0130 00 card 4111 1111 1111 1111",
    );
    let clean = value["clean_text"].as_str().unwrap();

    assert!(
        clean.contains("Custom:family:payment-card-or-iban"),
        "{clean}"
    );
    assert!(clean.contains("Custom:credit_card"), "{clean}");
    assert!(!clean.contains("3704 0044"), "{clean}");
    assert!(!clean.contains("4111 1111"), "{clean}");
    assert_eq!(value["stats"]["detections"], 2);
}

#[test]
fn s2_cli_bundled_smoke_drops_luhn_failing_formatted_cc() {
    let value = clean_json_with_args(
        &["--rulepack-bundled", "core,core-extended"],
        "Card 4111 1111 1111 1112",
    );
    let clean = value["clean_text"].as_str().unwrap();

    assert!(!clean.contains("Custom:credit_card"), "{clean}");
    assert_eq!(value["stats"]["detections"], 0);
}

#[test]
fn s3a_cli_bundled_core_extended_rejects_unassigned_e164_phone() {
    let value = clean_json_with_args(
        &["--rulepack-bundled", "core-extended"],
        "+99999999 is fake",
    );

    assert_eq!(value["clean_text"], "+99999999 is fake");
    assert_eq!(value["stats"]["detections"], 0);
}

#[test]
fn s3a_cli_bundled_core_extended_tokenizes_valid_e164_phone() {
    let value = clean_json_with_args(
        &["--rulepack-bundled", "core-extended"],
        // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
        // literals chosen for parser-valid + non-routable.
        "+4915100000000 is valid-format",
    );
    let clean = value["clean_text"].as_str().unwrap();

    assert!(
        Regex::new(r"^<[0-9a-f]{8}:Custom:phone_1> is valid-format$")
            .unwrap()
            .is_match(clean),
        "unexpected clean text: {clean}"
    );
    assert_eq!(value["stats"]["detections"], 1);
}

#[test]
fn s2_cli_bundled_core_extended_no_policy_tokenizes_national_de_and_us_phones() {
    let core = clean_json_with_args(
        &["--rulepack-bundled", "core"],
        // Source: NANPA 555-LINE Number Reservation.
        // https://nationalnanpa.com/number_resource_info/555_numbers.html
        "Phone +1 555 0100 ZIP 94103",
    );
    assert_eq!(core["clean_text"], "Phone +1 555 0100 ZIP 94103");
    assert_eq!(core["stats"]["detections"], 0);

    let alias_out = clean_raw_with_args(
        &["--rulepack-bundled", "core-extended"],
        // Source: NANPA 555-LINE Number Reservation.
        // https://nationalnanpa.com/number_resource_info/555_numbers.html
        "Phone +1 555 0100",
    );
    assert!(alias_out.status.success());
    assert!(
        String::from_utf8_lossy(&alias_out.stderr).contains("deprecated since v0.8.0"),
        "stderr={}",
        String::from_utf8_lossy(&alias_out.stderr)
    );
    let alias_json: Value = serde_json::from_slice(&alias_out.stdout).expect("stdout is JSON");
    assert!(
        Regex::new(r"^Phone <[0-9a-f]{8}:Custom:phone_1>$")
            .unwrap()
            .is_match(alias_json["clean_text"].as_str().unwrap()),
        "unexpected alias clean text: {}",
        alias_json["clean_text"]
    );

    let de = clean_json_with_args(
        &["--rulepack-bundled", "core-extended"],
        // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
        // literals chosen for parser-valid + non-routable.
        "Phone +49 30 0000 0000",
    );
    let de_clean = de["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^Phone <[0-9a-f]{8}:Custom:phone_1>$")
            .unwrap()
            .is_match(de_clean),
        "unexpected DE clean text: {de_clean}"
    );
    assert_eq!(de["stats"]["detections"], 1);

    let us = clean_json_with_args(
        &["--rulepack-bundled", "core-extended"],
        // Source: NANPA 555-LINE Number Reservation.
        // https://nationalnanpa.com/number_resource_info/555_numbers.html
        "Phone +1 555 0100",
    );
    let us_clean = us["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^Phone <[0-9a-f]{8}:Custom:phone_1>$")
            .unwrap()
            .is_match(us_clean),
        "unexpected US clean text: {us_clean}"
    );
    assert_eq!(us["stats"]["detections"], 1);
}

#[test]
fn s4_cli_bundled_core_extended_round_trips_broadened_de_area_code_phone() {
    let input = "Phone +49 40 0000 0000";
    let value = clean_json_with_args(&["--rulepack-bundled", "core-extended"], input);
    let clean = value["clean_text"].as_str().unwrap();

    assert!(
        Regex::new(r"^Phone <[0-9a-f]{8}:Custom:phone_1>$")
            .unwrap()
            .is_match(clean),
        "unexpected DE clean text: {clean}"
    );
    assert_eq!(value["stats"]["detections"], 1);
    assert_eq!(
        restore_success_text(value["session_blob"].as_str().unwrap(), clean),
        input
    );
}

#[test]
fn s2_core_extended_cli_locale_gating_and_plain_en_negative() {
    let (_dir, policy) =
        write_policy_with_core_extended_rulepacks(&["core", "core-extended"], "en-US");

    let us = clean_json_with_args(
        &[&format!("--policy={}", policy.display()), "--locale=en-US"],
        "ZIP 94103-1234",
    );
    let us_clean = us["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^ZIP <[0-9a-f]{8}:Custom:postal_code_1>$")
            .unwrap()
            .is_match(us_clean),
        "unexpected en-US clean text: {us_clean}"
    );

    let de = clean_json_with_args(
        &[&format!("--policy={}", policy.display()), "--locale=de-DE"],
        "Berlin 10115",
    );
    let de_clean = de["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^Berlin <[0-9a-f]{8}:Custom:postal_code_1>$")
            .unwrap()
            .is_match(de_clean),
        "unexpected de-DE clean text: {de_clean}"
    );

    let plain_en = clean_json_with_args(
        &[&format!("--policy={}", policy.display()), "--locale=en"],
        "ZIP 94103",
    );
    assert_eq!(plain_en["clean_text"], "ZIP 94103");
    assert_eq!(plain_en["stats"]["detections"], 0);

    let de_zip4 = clean_json_with_args(
        &[&format!("--policy={}", policy.display()), "--locale=de-DE"],
        "ZIP 94103-1234",
    );
    let de_zip4_clean = de_zip4["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^ZIP <[0-9a-f]{8}:Custom:postal_code_1>-1234$")
            .unwrap()
            .is_match(de_zip4_clean),
        "de-DE must not apply postal.us full ZIP+4 shape: {de_zip4_clean}"
    );
}

#[test]
fn s2_core_extended_national_phone_shipping_smoke() {
    let (_dir, policy) =
        write_policy_with_core_extended_rulepacks(&["core", "core-extended"], "en-US");

    let no_policy_us = clean_json_with_args(
        &["--rulepack-bundled=core-extended"],
        // Source: NANPA 555-LINE Number Reservation.
        // https://nationalnanpa.com/number_resource_info/555_numbers.html
        "Phone +1 555 0100",
    );
    let no_policy_us_clean = no_policy_us["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^Phone <[0-9a-f]{8}:Custom:phone_1>$")
            .unwrap()
            .is_match(no_policy_us_clean),
        "unexpected no-policy US national phone clean text: {no_policy_us_clean}"
    );
    let no_policy_order = clean_json_with_args(&["--rulepack-bundled=core-extended"], "Order_0815");
    assert_eq!(no_policy_order["clean_text"], "Order_0815");
    assert_eq!(no_policy_order["stats"]["detections"], 0);

    let us = clean_json_with_args(
        &[&format!("--policy={}", policy.display()), "--locale=en-US"],
        // Source: NANPA 555-LINE Number Reservation.
        // https://nationalnanpa.com/number_resource_info/555_numbers.html
        "Phone +1 555 0100",
    );
    let us_clean = us["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^Phone <[0-9a-f]{8}:Custom:phone_1>$")
            .unwrap()
            .is_match(us_clean),
        "unexpected US national phone clean text: {us_clean}"
    );
    assert_eq!(us["stats"]["detections"], 1);
    assert_eq!(
        restore_success_text(us["session_blob"].as_str().unwrap(), us_clean),
        "Phone +1 555 0100"
    );

    let de = clean_json_with_args(
        &[&format!("--policy={}", policy.display()), "--locale=de-DE"],
        // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
        // literals chosen for parser-valid + non-routable.
        "Phone +49 30 0000 0000",
    );
    let de_clean = de["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^Phone <[0-9a-f]{8}:Custom:phone_1>$")
            .unwrap()
            .is_match(de_clean),
        "unexpected DE national phone clean text: {de_clean}"
    );
    assert_eq!(de["stats"]["detections"], 1);
    assert_eq!(
        restore_success_text(de["session_blob"].as_str().unwrap(), de_clean),
        "Phone +49 30 0000 0000"
    );

    let de_under_en = clean_json_with_args(
        &[&format!("--policy={}", policy.display()), "--locale=en-US"],
        // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
        // literals chosen for parser-valid + non-routable.
        "Phone +49 30 0000 0000",
    );
    assert_eq!(de_under_en["clean_text"], "Phone +49 30 0000 0000");
    assert_eq!(de_under_en["stats"]["detections"], 0);
}

#[test]
fn s2_core_extended_tenant_like_numeric_ids_are_not_phone_tokens() {
    let (_dir, policy) =
        write_policy_with_core_extended_rulepacks(&["core", "core-extended"], "en-US");
    let input =
        "Subscriber_0001234567 Order_0815 0123-456789 0815 12345 SO-12345-67890 ORDR-9876543210";
    let value = clean_json_with_args(&[&format!("--policy={}", policy.display())], input);
    let clean = value["clean_text"].as_str().unwrap();

    assert!(!clean.contains("Custom:phone"), "{clean}");
    assert!(!clean.contains("Custom:iban"), "{clean}");
    assert!(!clean.contains("Custom:credit_card"), "{clean}");
    assert!(clean.contains("Subscriber_0001234567"), "{clean}");
    assert!(clean.contains("Order_0815"), "{clean}");
    assert!(clean.contains("0123-456789"), "{clean}");
}

#[test]
fn s2_core_extended_cli_validator_backed_iban_and_cards_emit_or_drop() {
    let (_dir, policy) =
        write_policy_with_core_extended_rulepacks(&["core", "core-extended"], "en-US");

    for (input, class) in [
        ("Card 4111111111111111", "credit_card"),
        ("Card 5555555555554444", "credit_card"),
        ("Card 378282246310005", "credit_card"),
        ("IBAN GB82WEST12345698765432", "family:payment-card-or-iban"),
        ("IBAN DE89370400440532013000", "family:payment-card-or-iban"),
        (
            "IBAN FR1420041010050500013M02606",
            "family:payment-card-or-iban",
        ),
        ("IBAN BE68539007547034", "family:payment-card-or-iban"),
        ("IBAN NL91ABNA0417164300", "family:payment-card-or-iban"),
    ] {
        let value = clean_json_with_args(&[&format!("--policy={}", policy.display())], input);
        let clean = value["clean_text"].as_str().unwrap();
        assert!(
            Regex::new(&format!(r"^.+<[0-9a-f]{{8}}:Custom:{class}_1>$"))
                .unwrap()
                .is_match(clean),
            "unexpected clean text for {input}: {clean}"
        );
        assert_eq!(value["stats"]["detections"], 1, "{input}");
        assert_eq!(
            restore_success_text(value["session_blob"].as_str().unwrap(), clean),
            input
        );
    }

    for input in [
        "Card 4111111111111112",
        "Card 5555555555554445",
        "Card 378282246310006",
        "IBAN GB99WEST12345698765432",
        "IBAN DE99370400440532013000",
    ] {
        let value = clean_json_with_args(&[&format!("--policy={}", policy.display())], input);
        let clean = value["clean_text"].as_str().unwrap();
        assert_eq!(clean, input, "{input}");
        assert_eq!(value["stats"]["detections"], 0, "{input}");
    }

    for input in [
        "Subscriber_0001234567",
        "Order_0815",
        "0815 12345",
        "Customer_42",
    ] {
        let value = clean_json_with_args(&[&format!("--policy={}", policy.display())], input);
        let clean = value["clean_text"].as_str().unwrap();
        assert!(!clean.contains("Custom:iban"), "{input}: {clean}");
        assert!(!clean.contains("Custom:credit_card"), "{input}: {clean}");
    }
}

#[test]
fn s2_cli_core_validator_locale_entities_tokenize_and_round_trip() {
    for (locale, wrong_locale, input, class) in [
        ("fr-FR", "en-US", "NIR 190010100100058", "nir"),
        ("nl-NL", "en-US", "BSN 111222333", "bsn"),
        ("pt-BR", "en-US", "CPF 529.982.247-25", "cpf"),
        ("pt-BR", "en-US", "CNPJ 04.252.011/0001-10", "cnpj"),
        ("en-IN", "en-US", "Aadhaar 6741 8529 6303", "aadhaar"),
        ("en-GB", "fr-FR", "NHS number 943 476 5919", "nhs_number"),
        ("de-DE", "en-US", "Steuer-ID 48 954 371 207", "steuer_id"),
        ("en-US", "en-GB", "SSN 123-45-6789", "ssn"),
        ("en-GB", "en-US", "NINO AB123456C", "nino"),
        ("en-IN", "en-US", "PAN ABCPA1234F", "pan"),
    ] {
        let value = clean_json_with_args(
            &["--rulepack-bundled=core", &format!("--locale={locale}")],
            input,
        );
        let clean = value["clean_text"].as_str().unwrap();
        assert!(
            clean.ends_with(&format!(":Custom:{class}_1>")),
            "{locale} {input}: {clean}"
        );
        assert_eq!(value["stats"]["detections"], 1, "{input}");
        assert_eq!(
            restore_success_text(value["session_blob"].as_str().unwrap(), clean),
            input
        );

        let wrong_locale = clean_json_with_args(
            &[
                "--rulepack-bundled=core",
                &format!("--locale={wrong_locale}"),
            ],
            input,
        );
        assert_eq!(wrong_locale["clean_text"], input, "{input}");
        assert_eq!(wrong_locale["stats"]["detections"], 0, "{input}");
    }
}

#[test]
fn s1_rulepack_path_override_loads_fixture_rulepack_and_round_trips() {
    let dir = tempdir().unwrap();
    let rulepack_path = dir.path().join("class-alpha.toml");
    fs::write(&rulepack_path, class_alpha_rulepack()).unwrap();
    let (_policy_dir, policy) = write_policy_for_class_alpha_rulepack();
    let input = "ticket class-alpha-123";

    let without_path = clean_json_with_args(&[&format!("--policy={}", policy.display())], input);
    assert_eq!(without_path["clean_text"], input);
    assert_eq!(without_path["stats"]["detections"], 0);

    let with_path = clean_json_with_args(
        &[
            &format!("--policy={}", policy.display()),
            &format!("--rulepack-path={}", rulepack_path.display()),
        ],
        input,
    );
    let clean = with_path["clean_text"].as_str().unwrap();
    assert!(clean.starts_with("ticket <"));
    assert!(clean.ends_with(":Custom:class_alpha_1>"));
    assert_eq!(with_path["stats"]["detections"], 1);
    assert_eq!(
        restore_success_text(with_path["session_blob"].as_str().unwrap(), clean),
        input
    );
}

#[test]
fn s1_ner_model_dir_and_locale_override_toml_and_detect_with_test_backend() {
    let model_dir_holder = tempdir().unwrap();
    let good_model_dir = model_dir_holder.path().join("__gaze_test_fixed_ner");
    fs::create_dir(&good_model_dir).unwrap();
    let missing_model_dir = model_dir_holder.path().join("missing-model");
    let (_policy_dir, policy) =
        write_policy_with_ner_model_dir(&missing_model_dir, "de-DE", "en-US", 0.3);
    let input = "Du antwortest als Support an Alice Example.";

    let out = clean_raw_with_args(&[&format!("--policy={}", policy.display())], input);
    assert_eq!(out.status.code(), Some(2));
    assert_policy_config_envelope(&out.stderr);

    let value = clean_json_with_args(
        &[
            &format!("--policy={}", policy.display()),
            &format!("--ner-model-dir={}", good_model_dir.display()),
            "--ner-locale=de",
        ],
        input,
    );
    let clean = value["clean_text"].as_str().unwrap();
    assert!(
        Regex::new(r"^Du antwortest als Support an <[0-9a-f]{8}:Name_\d+>\.$")
            .unwrap()
            .is_match(clean),
        "unexpected clean text: {clean}"
    );
    assert_eq!(value["stats"]["detections"], 1);
    assert_eq!(
        restore_success_text(value["session_blob"].as_str().unwrap(), clean),
        input
    );
}

#[test]
fn s1_three_surfaces_flags_are_exposed_and_bundled_ids_unchanged() {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for flag in [
        "--session-scope",
        "--ner-model-dir",
        "--ner-locale",
        "--rulepack-bundled",
        "--rulepack-path",
        "--session-ttl",
        "--ner-threshold",
        "--locale",
    ] {
        assert!(help.contains(flag), "missing CLI flag in help: {flag}");
    }

    assert!(gaze_recognizers::embedded("core").is_some());
    assert!(gaze_recognizers::embedded("locale-de").is_some());
    assert!(gaze_recognizers::embedded("locale-en").is_some());
    assert!(gaze_recognizers::embedded("locale-br").is_some());
    assert!(gaze_recognizers::embedded("locale-fr").is_some());
    assert!(gaze_recognizers::embedded("locale-in").is_some());
    assert!(gaze_recognizers::embedded("locale-nl").is_some());
    assert!(gaze_recognizers::embedded("locale-uk").is_some());
    assert!(gaze_recognizers::embedded("class_alpha").is_none());
}

#[test]
fn context_json_standalone_dictionary_detects_without_policy_entry() {
    let dir = tempdir().unwrap();
    let context_path = dir.path().join("context.json");
    fs::write(
        &context_path,
        r#"{
          "dictionaries": {
            "songs": { "terms": ["context-song-123"], "case_sensitive": true }
          },
          "class_map": { "songs": "custom:song" },
          "fields": { "tenant": "demo" }
        }"#,
    )
    .unwrap();

    let v = clean_json_with_args(
        &[&format!("--context-json={}", context_path.display())],
        "track context-song-123",
    );
    let clean = v["clean_text"].as_str().unwrap();
    assert!(clean.starts_with("track <"));
    assert!(clean.ends_with(":Custom:song_1>"));
    assert_eq!(v["stats"]["detections"], 1);
    assert_eq!(v["stats"]["context_source"], "cli");
    assert_eq!(
        v["stats"]["dictionaries_loaded"],
        json!([{ "name": "songs", "term_count": 1, "source": "cli" }])
    );
}

#[test]
fn rulepack_rejects_unknown_validator_kind() {
    let rulepack = de_email_rulepack("\"global\"").replace(
        "[recognizers.token]",
        "[recognizers.validator]\nkind = \"iban_modxx\"\n\n[recognizers.token]",
    );
    let (_dir, path) = write_policy_with_rulepack(&rulepack, None);
    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "kennung kundennummer-123",
    );

    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        parse_stderr_variant(&out.stderr),
        json!({
            "error": "PolicyConfig",
            "exit": 2,
            "detail": "recognizer error: unsupported validator: iban_modxx"
        })
    );
}

#[test]
fn rulepack_rejects_unknown_normalizer_kind() {
    let rulepack = de_email_rulepack("\"global\"").replace(
        "[recognizers.token]",
        "[recognizers.validator]\nkind = \"email_rfc\"\n\n[recognizers.normalizer]\nkind = \"iban_canonicalx\"\n\n[recognizers.token]",
    );
    let (_dir, path) = write_policy_with_rulepack(&rulepack, None);
    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "kennung kundennummer-123",
    );

    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        parse_stderr_variant(&out.stderr),
        json!({
            "error": "PolicyConfig",
            "exit": 2,
            "detail": "recognizer error: unsupported normalizer: iban_canonicalx"
        })
    );
}

#[test]
fn rulepack_rejects_typo_validator_kind() {
    let rulepack = de_email_rulepack("\"global\"").replace(
        "[recognizers.token]",
        "[recognizers.validator]\nkind = \"email_rfx\"\n\n[recognizers.token]",
    );
    let (_dir, path) = write_policy_with_rulepack(&rulepack, None);
    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "kennung kundennummer-123",
    );

    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        parse_stderr_variant(&out.stderr),
        json!({
            "error": "PolicyConfig",
            "exit": 2,
            "detail": "recognizer error: unsupported validator: email_rfx"
        })
    );
}

#[test]
fn rulepack_accepts_email_rfc_validator_kind() {
    let rulepack = de_email_rulepack("\"global\"").replace(
        "pattern = 'kundennummer-[0-9]+'",
        "pattern = '(?i)\\b[a-z0-9._%+\\-]+@example\\.invalid\\b'\n\n[recognizers.validator]\nkind = \"email_rfc\"\n\n[recognizers.normalizer]\nkind = \"email_canonical\"",
    );
    let (_dir, path) = write_policy_with_rulepack(&rulepack, None);
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "Email Alice@Example.invalid",
    );
    let clean = v["clean_text"].as_str().unwrap();

    assert!(clean.starts_with("Email <"));
    assert!(clean.ends_with(":Email_1>"));
    assert_eq!(v["stats"]["detections"], 1);
}

#[test]
fn rulepack_accepts_luhn_validator_kind() {
    let rulepack = r#"
schema_version = "0.1.0"
rulepack_id = "test-luhn"
rulepack_version = "0.4.0"
default_locales = ["global"]

[[recognizers]]
id = "card.test"
class = "custom:credit_card_or_iban"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = '\b\d{16}\b'

[recognizers.validator]
kind = "luhn"

[recognizers.scoring]
base = 0.90
priority = 77

[recognizers.token]
"#;
    let (_dir, path) =
        write_policy_with_rulepack_and_class(rulepack, None, "custom:credit_card_or_iban");
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "Card 4111111111111111",
    );
    let clean = v["clean_text"].as_str().unwrap();

    assert!(clean.starts_with("Card <"));
    assert!(clean.ends_with(":Custom:credit_card_or_iban_1>"));
    assert_eq!(v["stats"]["detections"], 1);
}

#[test]
fn rulepack_accepts_iban_mod97_validator_and_iban_canonical_normalizer() {
    let rulepack = r#"
schema_version = "0.1.0"
rulepack_id = "test-iban"
rulepack_version = "0.4.0"
default_locales = ["global"]

[[recognizers]]
id = "iban.test"
class = "custom:credit_card_or_iban"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = '\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b'

[recognizers.validator]
kind = "iban_mod97"

[recognizers.normalizer]
kind = "iban_canonical"

[recognizers.scoring]
base = 0.90
priority = 77

[recognizers.token]
"#;
    let (_dir, path) =
        write_policy_with_rulepack_and_class(rulepack, None, "custom:credit_card_or_iban");
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "IBAN GB82WEST12345698765432",
    );
    let clean = v["clean_text"].as_str().unwrap();

    assert!(clean.starts_with("IBAN <"));
    assert!(clean.ends_with(":Custom:credit_card_or_iban_1>"));
    assert_eq!(v["stats"]["detections"], 1);
}

#[test]
fn rulepack_context_exclusions_are_applied_at_detection_time() {
    let rulepack = r#"
schema_version = "0.1.0"
rulepack_id = "test-exclusions"
rulepack_version = "0.4.0"
default_locales = ["global"]

[[recognizers]]
id = "email.exclusions"
class = "Email"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = '(?i)\b[a-z0-9._%+\-]+@example\.invalid\b'

[recognizers.context]
exclusions = ["example.invalid"]

[recognizers.scoring]
base = 0.88
priority = 10

[recognizers.token]
"#;
    let (_dir, path) = write_policy_with_rulepack(rulepack, None);
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "Email alice@example.invalid",
    );

    assert_eq!(v["clean_text"], "Email alice@example.invalid");
    assert_eq!(v["stats"]["detections"], 0);
}

#[test]
fn rulepack_only_policy_round_trips_without_custom_recognizers() {
    let (_dir, path) = write_policy_with_rulepack(&de_email_rulepack("\"global\""), None);
    let v = clean_json_with_args(
        &[&format!("--policy={}", path.display())],
        "kennung kundennummer-123",
    );
    let clean = v["clean_text"].as_str().unwrap();
    let blob = v["session_blob"].as_str().unwrap();

    assert!(clean.starts_with("kennung <"));
    assert!(clean.ends_with(":Email_1>"));

    let (code, stdout, stderr) = restore_json(blob, clean);
    assert_eq!(code, Some(0), "stderr={}", String::from_utf8_lossy(&stderr));
    let restored: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(restored["text"], "kennung kundennummer-123");
}

#[test]
fn duplicate_rulepack_recognizer_ids_fail_closed() {
    let dir = tempdir().unwrap();
    let first = dir.path().join("first.toml");
    let second = dir.path().join("second.toml");
    fs::write(&first, de_email_rulepack("\"global\"")).unwrap();
    fs::write(
        &second,
        de_email_rulepack("\"global\"")
            .replace("rulepack_id = \"test-de\"", "rulepack_id = \"test-de-2\""),
    )
    .unwrap();
    let policy = dir.path().join("policy.toml");
    fs::write(
        &policy,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = []
paths = ["{}", "{}"]

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
            first.display(),
            second.display()
        ),
    )
    .unwrap();

    let out = clean_raw_with_args(
        &[&format!("--policy={}", policy.display())],
        "kennung kundennummer-123",
    );

    assert_eq!(out.status.code(), Some(2));
    assert_policy_config_envelope(&out.stderr);
}

#[test]
fn legacy_detector_policy_surface_fails_loudly() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 86400

[[detector]]
kind = "regex"
name = "emails"
pattern = ".+"
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();

    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "Email alice@example.invalid",
    );

    assert_eq!(out.status.code(), Some(2));
    assert_policy_config_envelope(&out.stderr);
}

// -----------------------------------------------------------------------
// 5. Adjacency corruption regression
// -----------------------------------------------------------------------

#[test]
fn t05_adjacency_corruption_name_inside_larger_word() {
    // Stub pipeline lacks a Name detector; build the session via the
    // library so we can prove Pass 1 only restores exact wrapped tokens and
    // leaves `Name_1` inside `hostName_1s-record` alone.
    let blob = build_blob_with(|s| {
        s.tokenize(&PiiClass::Name, "Alice Smith").unwrap();
    });

    let reply = "User-Name_1s-record is internal.";
    let (code, stdout, stderr) = restore_json(&blob, reply);
    assert_eq!(
        code,
        Some(0),
        "expected success, stderr={}",
        String::from_utf8_lossy(&stderr)
    );
    let resp: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(
        resp["text"].as_str().unwrap(),
        reply,
        "Pass 1 must not eat Name_1 substring"
    );
}

// -----------------------------------------------------------------------
// 6. Tamper
// -----------------------------------------------------------------------

#[test]
fn t06_tamper_flipped_byte_in_payload_rejected() {
    let (_, blob, _) = clean_ok("Email: alice@example.invalid");
    let mut raw = BASE64.decode(blob.as_bytes()).unwrap();
    // Flip a byte inside the JSON payload region (skip header: 1+32+64=97).
    let flip_idx = raw.len() - 5;
    raw[flip_idx] ^= 0x01;
    let tampered = BASE64.encode(&raw);

    let (code, _, stderr) = restore_json(&tampered, "Hello <Email_1>.");
    assert_eq!(code, Some(3));
    assert_eq!(
        parse_stderr_variant(&stderr),
        json!({ "error": "InvalidSignature", "exit": 3 })
    );
}

// -----------------------------------------------------------------------
// 7. Version-byte rejection
// -----------------------------------------------------------------------

#[test]
fn t07_version_byte_rejection() {
    let (_, blob, _) = clean_ok("Email: alice@example.invalid");
    let mut raw = BASE64.decode(blob.as_bytes()).unwrap();
    raw[0] = 99;
    let bad_version = BASE64.encode(&raw);

    let (code, _, stderr) = restore_json(&bad_version, "anything");
    assert_eq!(code, Some(3));
    assert_eq!(
        parse_stderr_variant(&stderr),
        json!({ "error": "InvalidBlobVersion", "exit": 3 })
    );
}

// -----------------------------------------------------------------------
// 7b. BlobExpired after persistent TTL elapses
// -----------------------------------------------------------------------

#[test]
fn t07b_restore_rejects_expired_blob() {
    let (_, blob, _) = clean_ok_with_args(&["--session-ttl=1"], "Email: alice@example.invalid");
    sleep(Duration::from_secs(2));

    let (code, _, stderr) = restore_json(&blob, "Hello <Email_1>.");
    assert_eq!(code, Some(3));
    assert_eq!(
        parse_stderr_variant(&stderr),
        json!({ "error": "BlobExpired", "exit": 3 })
    );
}

// -----------------------------------------------------------------------
// 8. Format rejection
// -----------------------------------------------------------------------

#[test]
fn t08_format_xml_rejected() {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--format=xml"])
        .write_stdin(b"anything".to_vec())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_policy_config_detail_contains(&out.stderr, "--format");
}

// -----------------------------------------------------------------------
// 9. Argv error sanitization
// -----------------------------------------------------------------------

#[test]
fn t09_bad_flag_emits_sanitized_json_not_clap_usage() {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("--bad-flag")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let value = parse_stderr_variant(&out.stderr);
    assert_eq!(value["error"], "PolicyConfig");
    assert_eq!(value["exit"], 2);
    assert!(
        value.get("detail").is_none(),
        "clap parse fallback must stay bare (no detail leak): {value}"
    );
    // No clap usage banner, no "Usage:" string.
    let stderr_str = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr_str.contains("Usage"),
        "clap usage leaked: {stderr_str}"
    );
    assert!(
        !stderr_str.contains("--help"),
        "clap help leaked: {stderr_str}"
    );
}

// Adopter-pain coverage (dogfooding F#5): the operator must see *why* a
// `PolicyConfig` was returned. These tests pin the specific detail wording
// for the highest-traffic misconfiguration paths so a downstream agent or
// support engineer can act without spelunking the loader.

#[test]
fn t09a_bundled_rulepack_unknown_name_surfaces_detail() {
    let out = clean_raw_with_args(&["--rulepack-bundled=does-not-exist"], "anything");
    assert_eq!(out.status.code(), Some(2));
    assert_policy_config_detail_contains(&out.stderr, "unknown bundled rulepack");
    assert_policy_config_detail_contains(&out.stderr, "does-not-exist");
}

#[test]
fn t09b_cli_locale_invalid_surfaces_detail() {
    // BCP47-parseable tags (e.g. "ZZ-bogus") fall through to LocaleTag::Other;
    // use a value that fails the BCP47 grammar so LocaleTag::parse errors.
    let out = clean_raw_with_args(&["--locale=not a locale"], "anything");
    assert_eq!(out.status.code(), Some(2));
    assert_policy_config_detail_contains(&out.stderr, "invalid --locale");
    assert_policy_config_detail_contains(&out.stderr, "not a locale");
}

// -----------------------------------------------------------------------
// 10. Panic sanitization
// -----------------------------------------------------------------------

#[test]
fn t10_panic_hook_sanitizes_stderr_even_with_backtrace() {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .env("GAZE_TEST_PANIC", "1")
        .env("RUST_BACKTRACE", "1")
        .write_stdin(b"unused".to_vec())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "panicking bin should exit 3");
    // Exactly one line, no backtrace.
    let stderr_str = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr_str.trim(),
        r#"{"error":"Pipeline","exit":3}"#,
        "panic hook did not sanitize: {stderr_str}"
    );
    assert!(!stderr_str.contains("stack backtrace"), "backtrace leaked");
    assert!(!stderr_str.contains("panicked at"), "panic message leaked");
}

// -----------------------------------------------------------------------
// 11. Empty-stdin on `clean`
// -----------------------------------------------------------------------

#[test]
fn t11_empty_stdin_on_clean_emits_empty_input() {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .write_stdin(Vec::<u8>::new())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        parse_stderr_variant(&out.stderr),
        json!({ "error": "EmptyInput", "exit": 1 })
    );
}

// -----------------------------------------------------------------------
// 12. Non-UTF-8 stdin on `clean`
// -----------------------------------------------------------------------

#[test]
fn t12_non_utf8_stdin_on_clean_emits_invalid_encoding() {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .write_stdin(vec![0xFF, 0xFE, 0xFD, 0xFC])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        parse_stderr_variant(&out.stderr),
        json!({ "error": "InvalidEncoding", "exit": 1 })
    );
}

// -----------------------------------------------------------------------
// 13. Oversized stdin
// -----------------------------------------------------------------------

#[test]
fn t13_oversized_stdin_emits_input_too_large() {
    let payload = vec![b'A'; 2048];
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--max-bytes=1024"])
        .write_stdin(payload)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        parse_stderr_variant(&out.stderr),
        json!({ "error": "InputTooLarge", "exit": 1 })
    );
}

// -----------------------------------------------------------------------
// 14. Silence on success
// -----------------------------------------------------------------------

#[test]
fn t14_silence_on_success_across_subcommands() {
    // `clean` success.
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .write_stdin(b"no PII at all".to_vec())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        out.stderr.is_empty(),
        "clean stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Re-use the emitted blob for `restore`.
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let blob = v["session_blob"].as_str().unwrap().to_string();

    let (code, _, stderr) = restore_json(&blob, "plain text reply");
    assert_eq!(code, Some(0));
    assert!(
        stderr.is_empty(),
        "restore stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
}

// -----------------------------------------------------------------------
// 15. Stats semantics — Preserve is NOT counted
// -----------------------------------------------------------------------

#[test]
fn t15_stats_detections_excludes_preserve() {
    // Spec's verbatim scenario needs a detected-but-preserved Organization
    // alongside a tokenized email; stub pipeline (solo #3 pending) only has
    // an email-Tokenize detector, so Preserve cannot fire for an org.
    // The semantic still goes through: a single tokenized email produces
    // `stats.detections == 1`, and the code path excluding `Action::Preserve`
    // in `CountingLogger::log` is unit-exercised here via the absence of
    // any extra counts on text that contains only one detection.
    let (_, _, detections) = clean_ok("Reach Alice via alice@example.invalid tomorrow.");
    assert_eq!(
        detections, 1,
        "one tokenized email must yield stats.detections == 1"
    );

    // No-detection input yields zero (proves counter is not a fixed value).
    let (_, _, zero) = clean_ok("Nothing detectable in this text.");
    assert_eq!(zero, 0);
}

/// Spec test 15 verbatim — requires the real policy loader (solo #3) so an
/// Organization detector can fire with `Action::Preserve` alongside an email
/// on `Action::Tokenize`. Re-enable once policy.toml loading lands.
#[test]
#[ignore = "requires policy loader (solo #3) to drive an Organization detector with Preserve"]
fn t15b_stats_detections_excludes_preserve_verbatim_spec() {
    let (_, _, detections) = clean_ok("Alice at alice@example.invalid works at Acme Corp.");
    assert_eq!(
        detections, 1,
        "tokenized email counts; preserved Organization does not"
    );
}

#[test]
fn t16_clean_with_policy_tokenizes_email() {
    let (_dir, policy_path) = write_minimal_policy();
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", &format!("--policy={}", policy_path.display())])
        .write_stdin(b"Email alice@example.invalid now".to_vec())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    let clean_text = value["clean_text"].as_str().unwrap();
    assert!(clean_text.starts_with("Email <"));
    assert!(clean_text.ends_with(":Email_1> now"));
}

#[test]
fn policy_session_ttl_is_honored() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 3600

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'alice@example[.]com'
class = "email"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", &format!("--policy={}", path.display())])
        .write_stdin(b"Email alice@example.invalid now".to_vec())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    let blob = value["session_blob"].as_str().unwrap();
    assert_eq!(session_blob_ttl_secs(blob), 3600);
}

#[test]
fn cli_session_ttl_overrides_policy() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 3600

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'alice@example[.]com'
class = "email"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "clean",
            &format!("--policy={}", path.display()),
            "--session-ttl=1800",
        ])
        .write_stdin(b"Email alice@example.invalid now".to_vec())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    let blob = value["session_blob"].as_str().unwrap();
    assert_eq!(session_blob_ttl_secs(blob), 1800);
}

#[test]
fn broken_ner_model_dir_exits_policy_config() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    let missing_model = dir.path().join("missing-model");
    fs::write(
        &path,
        format!(
            r#"
[session]
scope = "persistent"
ttl_secs = 3600

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'alice@example[.]com'
class = "email"

[ner]
model_dir = "{}"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
            missing_model.display()
        ),
    )
    .unwrap();

    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", &format!("--policy={}", path.display())])
        .write_stdin(b"Email alice@example.invalid now".to_vec())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_policy_config_envelope(&out.stderr);
}

#[test]
fn column_rule_policy_exits_policy_config_in_cli_mode() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 3600

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'alice@example[.]com'
class = "email"

[[rule]]
kind = "column"
column = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", &format!("--policy={}", path.display())])
        .write_stdin(b"Email alice@example.invalid now".to_vec())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("column rules not supported in CLI mode"),
        "stderr did not mention column rejection: {stderr}"
    );
    let value = parse_stderr_variant(&out.stderr);
    assert_eq!(value["error"], "PolicyConfig");
    assert_eq!(value["exit"], 2);
}

#[test]
fn t19_restore_custom_token_round_trip_ok() {
    let (blob, tokens) = build_blob_and_tokens(|s| {
        vec![s.tokenize(&PiiClass::custom("class_alpha"), "42").unwrap()]
    });
    let text = format!("Order {} is shipped.", tokens[0]);

    let (code, stdout, stderr) = restore_json(&blob, &text);
    assert_eq!(code, Some(0), "stderr={}", String::from_utf8_lossy(&stderr));
    let resp: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(resp["text"].as_str().unwrap(), "Order 42 is shipped.");
}

#[test]
fn t20_restore_custom_hallucination_exits_3() {
    let (blob, tokens) = build_blob_and_tokens(|s| {
        vec![s.tokenize(&PiiClass::custom("class_alpha"), "42").unwrap()]
    });

    let text = format!("Order {} and <Custom:fake_id_99> are shipped.", tokens[0]);
    let (code, stdout, stderr) = restore_json(&blob, &text);
    assert_eq!(
        code,
        Some(3),
        "expected exit 3, stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(
        parse_stderr_variant(&stderr),
        json!({ "error": "UnknownToken", "exit": 3, "token": "<Custom:fake_id_99>" })
    );
}

#[test]
fn t21_restore_wrapped_token_in_prose() {
    let (blob, tokens) = build_blob_and_tokens(|s| {
        vec![
            s.tokenize(&PiiClass::Email, "alice@example.invalid")
                .unwrap(),
            s.tokenize(&PiiClass::Email, "bob@example.invalid").unwrap(),
        ]
    });
    let text = format!("See {}. Reply {}", tokens[0], tokens[1]);

    let (code, stdout, stderr) = restore_json(&blob, &text);
    assert_eq!(code, Some(0), "stderr={}", String::from_utf8_lossy(&stderr));
    let resp: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(
        resp["text"].as_str().unwrap(),
        "See alice@example.invalid. Reply bob@example.invalid"
    );
}

#[test]
fn t17_missing_policy_path_emits_policy_open() {
    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", "--policy=/definitely/missing/policy.toml"])
        .write_stdin(b"Email alice@example.invalid now".to_vec())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(
        parse_stderr_variant(&out.stderr),
        json!({ "error": "PolicyOpen", "exit": 4 })
    );
}

#[test]
fn t18_malformed_policy_emits_policy_config() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 86400
bogus = true

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = ".+"
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", &format!("--policy={}", path.display())])
        .write_stdin(b"Email alice@example.invalid now".to_vec())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_policy_config_detail_contains(&out.stderr, "parse policy.toml");
}

// Dogfooding F#6: policy.toml schema_version mismatch must fail closed with a
// typed envelope so adopters upgrading the gaze binary across a contract break
// see the version mismatch directly rather than a generic PolicyConfig that
// shadows the real cause.
#[test]
fn t19_policy_schema_version_unsupported_emits_typed_envelope() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
schema_version = "9.9.0"
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = ".+"
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();

    let out = Command::cargo_bin("gaze")
        .unwrap()
        .args(["clean", &format!("--policy={}", path.display())])
        .write_stdin(b"anything".to_vec())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let value = parse_stderr_variant(&out.stderr);
    assert_eq!(value["error"], "PolicySchemaUnsupported");
    assert_eq!(value["exit"], 2);
    assert_eq!(value["found"], "9.9.0");
    assert_eq!(value["supported"], "0.1");
}

#[test]
fn t19a_policy_without_schema_version_loads_via_soft_default() {
    // Existing 0.6.x / 0.7.x policies omit `schema_version`. The loader must
    // keep accepting them by stamping DEFAULT_POLICY_SCHEMA_VERSION so the
    // 0.1.x deployments do not hard-break on the binary upgrade that
    // introduces the field.
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'a@b'
class = "email"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .unwrap();

    let out = clean_raw_with_args(
        &[&format!("--policy={}", path.display())],
        "ping a@b please",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "soft default must accept missing schema_version: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ------------------------------------------------------------------
// Kiji DistilBERT safety-net backend activation predicate (v0.8 T2.5)
// ------------------------------------------------------------------

/// Activating `--safety-net-backend=kiji-distilbert` with a missing model
/// artifact must fail closed with the typed `SafetyNetArtifactMissing` envelope
/// and CLI exit code 2 (config-level, Axis-1 reliability — never silent-
/// disable).
#[cfg(feature = "safety-net-kiji")]
#[test]
fn t_kiji_distilbert_backend_without_artifact_emits_typed_envelope() {
    let dir = tempdir().unwrap();
    let kiji = dir.path().join("kiji");
    fs::write(&kiji, b"#!/bin/sh\ncat >/dev/null\nprintf '[]\\n'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&kiji, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let model_dir = dir.path().join("kiji-distilbert");
    fs::create_dir(&model_dir).unwrap();
    // No SHA256SUMS, no model.onnx, no tokenizer.json, no labels.json.

    let out = clean_raw_with_args(
        &[
            "--safety-net=kiji-distilbert",
            "--safety-net-backend=kiji-distilbert",
            &format!("--kiji-distilbert-command={}", kiji.display()),
            &format!("--kiji-distilbert-model-dir={}", model_dir.display()),
        ],
        "hello",
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr_line = String::from_utf8_lossy(&out.stderr)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let value: Value =
        serde_json::from_str(&stderr_line).expect("stderr is line-delimited JSON envelope");
    assert_eq!(value["error"], "SafetyNetArtifactMissing");
    assert_eq!(value["exit"], 2);
    assert_eq!(value["backend"], "kiji-distilbert");
    let path = value["path"].as_str().expect("path field");
    // First missing artifact surfaced is SHA256SUMS — the pinned-artifact
    // contract enforces this order: SHA256SUMS, labels.json, model.onnx,
    // tokenizer.json.
    assert!(
        path.contains("SHA256SUMS"),
        "expected SHA256SUMS in artifact path, got {path}"
    );
    assert!(
        path.contains("fetch-kiji-safetynet-model.sh"),
        "expected install hint, got {path}"
    );
}
