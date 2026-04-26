use std::{fs, path::Path};

use anyhow::{bail, Context, Result};

const NO_PHONE_PARSER_WORKFLOW: &str = ".github/workflows/no-phone-parser.yml";
const REQUIRED_NO_FEATURE_TEST_COMMAND: &str =
    "cargo test -p gaze-recognizers --no-default-features";
const REQUIRED_PACKAGE_TARGET: &str = "gaze-recognizers";
const REQUIRED_NO_DEFAULT_FEATURES: &str = "--no-default-features";

pub fn run() -> Result<()> {
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

    println!("ci_feature_matrix: passed");
    Ok(())
}
