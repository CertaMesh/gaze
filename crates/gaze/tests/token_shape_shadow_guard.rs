use std::fs;

use gaze::{Policy, PolicyError, Rulepack, RulepackError};
use tempfile::tempdir;

#[test]
fn policy_load_rejects_custom_recognizer_regex_matching_token_shape() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "ephemeral"

[[policy.custom_recognizers]]
kind = "regex"
name = "token-shaped-email"
pattern = 'Email_\d+'
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .expect("policy fixture");

    let err = Policy::load(&path).expect_err("token-shaped regex must fail closed");

    assert!(matches!(
        &err,
        PolicyError::TokenShapeShadow {
            name,
            shadowed_shape,
            ..
        } if name == "token-shaped-email" && shadowed_shape == "<deadbeef:Email_1>"
    ));
    assert!(
        err.to_string().contains("Email_1"),
        "error should name the shadowed sample: {err}"
    );
}

#[test]
fn rulepack_load_rejects_custom_regex_matching_token_shape() {
    let err = Rulepack::parse(&rulepack_with_pattern(r"Email_\d+"))
        .expect_err("token-shaped regex must fail closed");

    assert!(matches!(
        &err,
        RulepackError::TokenShapeShadow {
            id,
            shadowed_shape,
            ..
        } if id == "custom.email" && shadowed_shape == "<deadbeef:Email_1>"
    ));
    assert!(
        err.to_string().contains("Email_1"),
        "error should name the shadowed sample: {err}"
    );
}

#[test]
fn non_shadowing_custom_regexes_load() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        r#"
[session]
scope = "ephemeral"

[[policy.custom_recognizers]]
kind = "regex"
name = "order-id"
pattern = '\bORD-[0-9]{4}\b'
class = "custom:order_id"

[[rule]]
kind = "default"
action = "preserve"
"#,
    )
    .expect("policy fixture");

    let policy = Policy::load(&path).expect("non-shadowing policy regex should load");
    assert_eq!(policy.detectors.len(), 1);

    let rulepack = Rulepack::parse(&rulepack_with_pattern(r"\bORD-[0-9]{4}\b"))
        .expect("non-shadowing rulepack regex should load");
    assert_eq!(rulepack.recognizers.len(), 1);
}

fn rulepack_with_pattern(pattern: &str) -> String {
    format!(
        r#"
schema_version = "0.1.0"
rulepack_id = "custom-email"
rulepack_version = "0.7.0"
default_locales = ["global"]

[[recognizers]]
id = "custom.email"
class = "Email"
enabled = true

[recognizers.match]
kind = "regex"
pattern = '{pattern}'
"#
    )
}
