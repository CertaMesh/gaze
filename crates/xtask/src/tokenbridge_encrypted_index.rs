use crate::test_commands::{
    exact_test_command_with_args, list_command_output, list_test_command_with_args,
    BehavioralTarget, BehavioralTest,
};

use anyhow::{bail, Context, Result};

const GATE: &str = "tokenbridge_encrypted_index";
const PACKAGE: &str = "gaze-token-bridge";
const TEST_NAME: &str = "persistent::tests::saved_index_file_is_aead_sealed_and_omits_plaintext";

pub fn run() -> Result<()> {
    println!("{GATE}: checking encrypted persistent-index behavioral test");
    let test = BehavioralTest {
        package: PACKAGE,
        target: BehavioralTarget::All,
        name: TEST_NAME,
    };
    let stdout = list_command_output(
        list_test_command_with_args(test, &["--all-features"]),
        PACKAGE,
    )?;
    let expected_line = format!("{TEST_NAME}: test");
    if !stdout.lines().any(|line| line == expected_line) {
        bail!("{GATE}: missing behavioral test: {PACKAGE} {TEST_NAME}");
    }
    println!("{GATE}: running {PACKAGE} {TEST_NAME}");
    let status = exact_test_command_with_args(test, &["--all-features"])
        .status()
        .with_context(|| format!("failed to run {PACKAGE} {TEST_NAME}"))?;
    if !status.success() {
        bail!("{GATE}: behavioral test failed: {PACKAGE} {TEST_NAME}");
    }
    println!("{GATE}: passed");
    Ok(())
}
