use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{bail, Context, Result};

use crate::{ensure_test_exists, run_behavioral_test, BehavioralTarget, BehavioralTest};

const ALLOWLIST_PATH: &str = "lint/data/safety-net-class-allowlist.toml";
const OPENAI_CLASS_MAP_PATH: &str =
    "crates/gaze-recognizers/src/safety_net/openai_filter/class_map.rs";
const GAZE_TYPES_PATH: &str = "crates/gaze-types/src/lib.rs";

/// Adversarial self-test: reviewer manually renames one of the listed
/// tests on a throwaway branch and verifies xtask exits non-zero.
/// Codifies meta-Potemkin guard per drawer gaze_architecture_12b32d53.
const CLASS_MAP_OVERRIDE_SAFETY_TESTS: &[BehavioralTest] = &[
    BehavioralTest {
        package: "gaze-assembly",
        target: BehavioralTarget::Lib,
        name: "tests::t20_context_class_map_overrides_policy_dict_class",
    },
    BehavioralTest {
        package: "gaze-assembly",
        target: BehavioralTarget::Lib,
        name: "tests::t20a_class_map_override_fails_closed_when_action_rule_uncovered",
    },
    BehavioralTest {
        package: "gaze-assembly",
        target: BehavioralTarget::Lib,
        name: "tests::t20b_rulepack_context_dict_override_fails_closed_when_uncovered",
    },
    BehavioralTest {
        package: "gaze-cli",
        target: BehavioralTarget::Integration("cli_pipe"),
        name: "context_json_standalone_dictionary_detects_without_policy_entry",
    },
];

pub fn run() -> Result<()> {
    assert_safety_net_allowlist()?;

    println!(
        "class_map_override_safety: checking {} behavioral tests",
        CLASS_MAP_OVERRIDE_SAFETY_TESTS.len()
    );
    for test in CLASS_MAP_OVERRIDE_SAFETY_TESTS {
        ensure_test_exists(*test)?;
    }
    for test in CLASS_MAP_OVERRIDE_SAFETY_TESTS {
        run_behavioral_test("class_map_override_safety", *test)?;
    }
    println!("class_map_override_safety: passed");
    Ok(())
}

fn assert_safety_net_allowlist() -> Result<()> {
    let allowlist = parse_allowlist(ALLOWLIST_PATH)?;
    let expected = expected_allowlist();
    if allowlist != expected {
        bail!(
            "class_map_override_safety: {} drifted\nexpected: {expected:?}\nactual: {allowlist:?}",
            ALLOWLIST_PATH
        );
    }

    let class_map = fs::read_to_string(OPENAI_CLASS_MAP_PATH)
        .with_context(|| format!("failed to read {OPENAI_CLASS_MAP_PATH}"))?;
    let types = fs::read_to_string(GAZE_TYPES_PATH)
        .with_context(|| format!("failed to read {GAZE_TYPES_PATH}"))?;

    let actual_labels = extract_map_openai_label_arms(&class_map)?;
    if actual_labels != expected_label_to_variant() {
        bail!(
            "class_map_override_safety: OpenAI label match arms drifted\nexpected: {:?}\nactual: {:?}",
            expected_label_to_variant(),
            actual_labels
        );
    }

    let actual_classes = extract_openai_variant_to_class_arms(&class_map)?;
    if actual_classes != expected_variant_to_safety_class() {
        bail!(
            "class_map_override_safety: safety-net class match arms drifted\nexpected: {:?}\nactual: {:?}",
            expected_variant_to_safety_class(),
            actual_classes
        );
    }

    assert_gaze_type_label_names(&types)?;
    assert_gaze_pii_class_mappings(&types)?;
    Ok(())
}

fn parse_allowlist(path: &str) -> Result<BTreeMap<String, String>> {
    let contents = fs::read_to_string(Path::new(path))
        .with_context(|| format!("failed to read safety-net allowlist at {path}"))?;
    let mut in_allowlist = false;
    let mut entries = BTreeMap::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[allowlist]" {
            in_allowlist = true;
            continue;
        }
        if line.starts_with('[') {
            in_allowlist = false;
            continue;
        }
        if !in_allowlist {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            bail!("class_map_override_safety: malformed allowlist line `{line}`");
        };
        entries.insert(key.trim().to_string(), unquote(value.trim())?.to_string());
    }

    Ok(entries)
}

fn unquote(value: &str) -> Result<&str> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        Ok(&value[1..value.len() - 1])
    } else {
        bail!("class_map_override_safety: unquoted allowlist value `{value}`");
    }
}

fn extract_map_openai_label_arms(contents: &str) -> Result<BTreeMap<String, String>> {
    let body = function_body(contents, "pub fn map_openai_label")?;
    let mut labels = BTreeMap::new();
    for line in body.lines().map(str::trim) {
        let Some((label, rest)) = line.split_once("\" => Ok(OpenAiPrivateLabel::") else {
            continue;
        };
        let label = label.trim_start_matches('"').to_string();
        let variant = rest
            .split(')')
            .next()
            .context("missing OpenAiPrivateLabel arm close")?
            .to_string();
        labels.insert(label, variant);
    }
    Ok(labels)
}

fn extract_openai_variant_to_class_arms(contents: &str) -> Result<BTreeMap<String, String>> {
    let body = function_body(contents, "pub fn openai_label_to_safety_net_class")?;
    let mut classes = BTreeMap::new();
    for line in body.lines().map(str::trim) {
        let Some((variant, rest)) = line.split_once(" => SafetyNetPiiClass::") else {
            continue;
        };
        let variant = variant
            .trim_start_matches("OpenAiPrivateLabel::")
            .to_string();
        let class = rest.trim_end_matches(',').to_string();
        classes.insert(variant, class);
    }
    Ok(classes)
}

fn function_body<'a>(contents: &'a str, signature: &str) -> Result<&'a str> {
    let start = contents
        .find(signature)
        .with_context(|| format!("missing function `{signature}`"))?;
    let body_start = contents[start..]
        .find('{')
        .map(|offset| start + offset + 1)
        .with_context(|| format!("missing body for `{signature}`"))?;
    let mut depth = 1usize;
    for (offset, byte) in contents[body_start..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&contents[body_start..body_start + offset]);
                }
            }
            _ => {}
        }
    }
    bail!("unterminated function body for `{signature}`")
}

fn assert_gaze_type_label_names(types: &str) -> Result<()> {
    for (label, variant) in expected_label_to_variant() {
        let needle = format!("Self::{variant} => \"{label}\"");
        if !types.contains(&needle) {
            bail!("class_map_override_safety: missing OpenAiPrivateLabel::as_str arm `{needle}`");
        }
    }
    Ok(())
}

fn assert_gaze_pii_class_mappings(types: &str) -> Result<()> {
    for (variant, pii_class) in expected_variant_to_pii_class() {
        let safety_class = expected_variant_to_safety_class()
            .remove(&variant)
            .context("missing safety class")?;
        let needle = match pii_class.as_str() {
            "Name" | "Location" | "Email" => {
                format!("Self::{safety_class} => PiiClass::{pii_class}")
            }
            custom => {
                let custom = custom
                    .strip_prefix("Custom(\"")
                    .and_then(|value| value.strip_suffix("\")"))
                    .context("unexpected custom class spelling")?;
                format!("Self::{safety_class} => PiiClass::custom(\"{custom}\")")
            }
        };
        if !types.contains(&needle) {
            bail!("class_map_override_safety: missing SafetyNetPiiClass mapping `{needle}`");
        }
    }
    Ok(())
}

fn expected_allowlist() -> BTreeMap<String, String> {
    [
        ("private_person", "Name"),
        ("private_address", "Location"),
        ("private_email", "Email"),
        ("private_phone", "Custom(\"phone\")"),
        ("private_url", "Custom(\"url\")"),
        ("private_date", "Custom(\"date\")"),
        ("account_number", "Custom(\"account_number\")"),
        ("secret", "Custom(\"secret\")"),
    ]
    .into_iter()
    .map(|(label, class)| (label.to_string(), class.to_string()))
    .collect()
}

fn expected_label_to_variant() -> BTreeMap<String, String> {
    [
        ("private_person", "PrivatePerson"),
        ("private_address", "PrivateAddress"),
        ("private_email", "PrivateEmail"),
        ("private_phone", "PrivatePhone"),
        ("private_url", "PrivateUrl"),
        ("private_date", "PrivateDate"),
        ("account_number", "AccountNumber"),
        ("secret", "Secret"),
    ]
    .into_iter()
    .map(|(label, variant)| (label.to_string(), variant.to_string()))
    .collect()
}

fn expected_variant_to_safety_class() -> BTreeMap<String, String> {
    [
        ("PrivatePerson", "Name"),
        ("PrivateAddress", "Location"),
        ("PrivateEmail", "Email"),
        ("PrivatePhone", "Phone"),
        ("PrivateUrl", "Url"),
        ("PrivateDate", "Date"),
        ("AccountNumber", "AccountNumber"),
        ("Secret", "Secret"),
    ]
    .into_iter()
    .map(|(variant, class)| (variant.to_string(), class.to_string()))
    .collect()
}

fn expected_variant_to_pii_class() -> BTreeMap<String, String> {
    expected_label_to_variant()
        .into_iter()
        .map(|(label, variant)| {
            let pii_class = expected_allowlist()
                .remove(&label)
                .expect("allowlist entry");
            (variant, pii_class)
        })
        .collect()
}
