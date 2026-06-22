#![cfg(not(feature = "phone-parser"))]

use gaze_recognizers::{embedded, ValidatorKind};
use gaze_types::ValidatorKindParseError;

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

fn assert_unsupported_phone_validator(validator: &str) {
    let err = ValidatorKind::parse(validator)
        .expect_err("phone validator must fail closed without phone-parser feature");

    assert!(
        matches!(err, ValidatorKindParseError::UnsupportedValidator { ref kind } if kind == validator),
        "expected UnsupportedValidator for {validator}, got {err:?}"
    );
}

fn recognizer_block<'a>(rulepack: &'a str, id: &str) -> &'a str {
    rulepack
        .split("[[recognizers]]")
        .find(|block| block.contains(&format!("id = \"{id}\"")))
        .unwrap_or_else(|| panic!("missing recognizer {id}"))
}
