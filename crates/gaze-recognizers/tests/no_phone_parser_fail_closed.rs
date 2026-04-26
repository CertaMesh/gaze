#![cfg(not(feature = "phone-parser"))]

use gaze::{RulepackError, RulepackSource};

#[test]
fn phone_validators_fail_closed_at_rulepack_load_without_phone_parser() {
    for validator in [
        "e164_phone",
        "e164_phone_national_de",
        "e164_phone_national_us",
    ] {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("rulepack.toml");
        std::fs::write(&path, rulepack(validator)).expect("write minimal rulepack");

        let err = gaze_recognizers::load_rulepack(RulepackSource::Path(path))
            .expect_err("phone validator must fail closed without phone-parser feature");

        assert!(
            matches!(err, RulepackError::UnsupportedValidator { ref kind } if kind == validator),
            "expected UnsupportedValidator for {validator}, got {err:?}"
        );
    }
}

fn rulepack(validator: &str) -> String {
    format!(
        r#"
schema_version = "0.1.0"
rulepack_id = "no-phone-parser-fail-closed-{validator}"
rulepack_version = "0.4.6"
default_locales = ["global"]

[[recognizers]]
id = "phone.{validator}"
class = "custom:phone"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = '''\+\d{{6,15}}\b'''

[recognizers.validator]
kind = "{validator}"

[recognizers.scoring]
base = 0.70
priority = 80

[recognizers.token]
"#
    )
}
