use std::{fs, path::Path};

use anyhow::{bail, Context, Result};

const NO_PHONE_PARSER_WORKFLOW: &str = ".github/workflows/no-phone-parser.yml";
const SAFETY_NET_SANITY_WORKFLOW: &str = ".github/workflows/safety-net-sanity.yml";
const REQUIRED_NO_FEATURE_TEST_COMMAND: &str =
    "cargo test -p gaze-recognizers --no-default-features";
const REQUIRED_PACKAGE_TARGET: &str = "gaze-recognizers";
const REQUIRED_NO_DEFAULT_FEATURES: &str = "--no-default-features";
const REQUIRED_SAFETY_NET_COMMANDS: &[&str] = &[
    "cargo check -p gaze --features safety-net",
    "cargo check -p gaze-recognizers --features safety-net-openai",
    "cargo check -p gaze-cli --features safety-net-openai",
    "cargo test -p gaze-audit",
    "cargo run -p xtask -- safety-net-sanity",
];

pub fn run() -> Result<()> {
    assert_no_phone_parser_matrix()?;
    assert_safety_net_matrix()?;
    println!("ci_feature_matrix: passed");
    Ok(())
}

fn assert_no_phone_parser_matrix() -> Result<()> {
    let workflow = Path::new(NO_PHONE_PARSER_WORKFLOW);
    let contents = fs::read_to_string(workflow)
        .with_context(|| format!("failed to read {}", workflow.display()))?;

    if !contents.contains(REQUIRED_PACKAGE_TARGET) {
        bail!(
            "ci_feature_matrix: {} no longer targets {}",
            workflow.display(),
            REQUIRED_PACKAGE_TARGET
        );
    }

    if !contents.contains(REQUIRED_NO_DEFAULT_FEATURES) {
        bail!(
            "ci_feature_matrix: {} no longer uses {}",
            workflow.display(),
            REQUIRED_NO_DEFAULT_FEATURES
        );
    }

    if !contents.contains(REQUIRED_NO_FEATURE_TEST_COMMAND) {
        bail!(
            "ci_feature_matrix: {} must contain `{}`",
            workflow.display(),
            REQUIRED_NO_FEATURE_TEST_COMMAND
        );
    }

    Ok(())
}

fn assert_safety_net_matrix() -> Result<()> {
    let workflow = Path::new(SAFETY_NET_SANITY_WORKFLOW);
    let contents = fs::read_to_string(workflow)
        .with_context(|| format!("failed to read {}", workflow.display()))?;

    for required in REQUIRED_SAFETY_NET_COMMANDS {
        if !contents.contains(required) {
            bail!(
                "ci_feature_matrix: {} must contain `{}`",
                workflow.display(),
                required
            );
        }
    }
    Ok(())
}
