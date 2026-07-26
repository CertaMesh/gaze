#[cfg(not(feature = "phone-parser"))]
use gaze_recognizers::{embedded, ValidatorKind};
#[cfg(not(feature = "phone-parser"))]
use gaze_types::ValidatorKindParseError;
#[cfg(feature = "phone-parser")]
use std::{fs, path::Path, process::Command, sync::Mutex};

#[cfg(feature = "phone-parser")]
static ISOLATED_HARNESS_LOCK: Mutex<()> = Mutex::new(());

#[cfg(feature = "phone-parser")]
const ISOLATED_HARNESS_MANIFEST: &str = r#"[package]
name = "gaze-recognizers-no-phone-parser-tests"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]
resolver = "2"

[features]
default = []
phone-parser = []

[dependencies]
gaze-recognizers = { path = "../../crates/gaze-recognizers", default-features = false }
gaze-types = { path = "../../crates/gaze-types", default-features = false }

[[test]]
name = "no_phone_parser_fail_closed"
path = "../../crates/gaze-recognizers/tests/no_phone_parser_fail_closed.rs"
"#;

#[cfg(feature = "phone-parser")]
#[test]
fn phone_validators_fail_closed_without_phone_parser() {
    run_in_isolated_no_phone_parser_harness("phone_validators_fail_closed_without_phone_parser");
}

#[cfg(not(feature = "phone-parser"))]
#[test]
fn phone_validators_fail_closed_without_phone_parser() {
    for validator in [
        "e164_phone",
        "e164_phone_national_de",
        "e164_phone_national_us",
    ] {
        assert_unsupported_phone_validator(validator);
    }
}

#[cfg(feature = "phone-parser")]
#[test]
fn embedded_spaced_e164_phone_recognizer_fails_closed_without_phone_parser() {
    run_in_isolated_no_phone_parser_harness(
        "embedded_spaced_e164_phone_recognizer_fails_closed_without_phone_parser",
    );
}

#[cfg(not(feature = "phone-parser"))]
#[test]
fn embedded_spaced_e164_phone_recognizer_fails_closed_without_phone_parser() {
    let raw = embedded("core-extended").expect("core-extended embedded rulepack");
    let recognizer = recognizer_block(&raw, "phone.e164.spaced");
    assert!(
        recognizer.contains("kind = \"e164_phone\""),
        "phone.e164.spaced must stay gated by e164_phone: {recognizer}"
    );
    assert_unsupported_phone_validator("e164_phone");
}

#[cfg(not(feature = "phone-parser"))]
fn assert_unsupported_phone_validator(validator: &str) {
    let err = ValidatorKind::parse(validator)
        .expect_err("phone validator must fail closed without phone-parser feature");

    assert!(
        matches!(err, ValidatorKindParseError::UnsupportedValidator { ref kind } if kind == validator),
        "expected UnsupportedValidator for {validator}, got {err:?}"
    );
}

#[cfg(not(feature = "phone-parser"))]
fn recognizer_block<'a>(rulepack: &'a str, id: &str) -> &'a str {
    rulepack
        .split("[[recognizers]]")
        .find(|block| block.contains(&format!("id = \"{id}\"")))
        .unwrap_or_else(|| panic!("missing recognizer {id}"))
}

#[cfg(feature = "phone-parser")]
fn run_in_isolated_no_phone_parser_harness(test_name: &str) {
    let _guard = ISOLATED_HARNESS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("gaze-recognizers must live under the workspace crates directory");
    let harness_dir = workspace_root
        .join("target")
        .join("no-phone-parser-fail-closed-harness");
    fs::create_dir_all(&harness_dir).expect("create isolated no-phone-parser harness directory");

    let manifest_path = harness_dir.join("Cargo.toml");
    fs::write(&manifest_path, ISOLATED_HARNESS_MANIFEST)
        .expect("write isolated no-phone-parser harness manifest");
    fs::copy(
        workspace_root.join("Cargo.lock"),
        harness_dir.join("Cargo.lock"),
    )
    .expect("seed isolated no-phone-parser harness lockfile");

    let output = Command::new(env!("CARGO"))
        .arg("test")
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--offline")
        .arg("--test")
        .arg("no_phone_parser_fail_closed")
        .arg("--")
        .arg(test_name)
        .arg("--exact")
        .env("CARGO_TARGET_DIR", harness_dir.join("target"))
        .output()
        .expect("run isolated no-phone-parser fail-closed test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "isolated no-phone-parser test {test_name} failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("running 1 test") || stderr.contains("running 1 test"),
        "isolated no-phone-parser test {test_name} did not execute exactly one test\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("test result: ok. 1 passed")
            || stderr.contains("test result: ok. 1 passed"),
        "isolated no-phone-parser test {test_name} did not pass exactly one test\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
