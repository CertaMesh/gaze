use std::{fs, path::Path};

use anyhow::{bail, Context, Result};

const PRE_PUSH_HOOK: &str = ".githooks/pre-push";
const REQUIRED_NO_FEATURE_TEST_COMMAND: &str =
    "cargo test -p gaze-recognizers --no-default-features";
const REQUIRED_PACKAGE_TARGET: &str = "gaze-recognizers";
const REQUIRED_NO_DEFAULT_FEATURES: &str = "--no-default-features";
const REQUIRED_SAFETY_NET_SANITY_COMMAND: &str = "cargo run -p xtask -- safety-net-sanity";

pub fn run() -> Result<()> {
    let hook = Path::new(PRE_PUSH_HOOK);
    let contents =
        fs::read_to_string(hook).with_context(|| format!("failed to read {}", hook.display()))?;

    if !contents.contains(REQUIRED_PACKAGE_TARGET) {
        bail!(
            "ci_feature_matrix: {} no longer targets {}",
            hook.display(),
            REQUIRED_PACKAGE_TARGET
        );
    }

    if !contents.contains(REQUIRED_NO_DEFAULT_FEATURES) {
        bail!(
            "ci_feature_matrix: {} no longer uses {}",
            hook.display(),
            REQUIRED_NO_DEFAULT_FEATURES
        );
    }

    if !contents.contains(REQUIRED_NO_FEATURE_TEST_COMMAND) {
        bail!(
            "ci_feature_matrix: {} must contain `{}`",
            hook.display(),
            REQUIRED_NO_FEATURE_TEST_COMMAND
        );
    }

    if !contents.contains(REQUIRED_SAFETY_NET_SANITY_COMMAND) {
        bail!(
            "ci_feature_matrix: {} must contain `{}`",
            hook.display(),
            REQUIRED_SAFETY_NET_SANITY_COMMAND
        );
    }

    println!("ci_feature_matrix: passed");
    Ok(())
}
