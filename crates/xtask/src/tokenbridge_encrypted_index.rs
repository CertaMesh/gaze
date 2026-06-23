use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};

const GATE: &str = "tokenbridge_encrypted_index";
const PACKAGE: &str = "gaze-token-bridge";
const TEST_NAME: &str = "persistent::tests::saved_index_file_is_aead_sealed_and_omits_plaintext";

pub fn run() -> Result<()> {
    println!("{GATE}: checking encrypted persistent-index behavioral test");
    ensure_test_exists()?;
    println!("{GATE}: running {PACKAGE} {TEST_NAME}");
    let status = cargo_test_command(Some(TEST_NAME))
        .arg("--")
        .arg("--exact")
        .status()
        .with_context(|| format!("failed to run {PACKAGE} {TEST_NAME}"))?;
    if !status.success() {
        bail!("{GATE}: behavioral test failed: {PACKAGE} {TEST_NAME}");
    }
    println!("{GATE}: passed");
    Ok(())
}

fn ensure_test_exists() -> Result<()> {
    let output = cargo_test_command(None)
        .arg("--")
        .arg("--list")
        .output()
        .with_context(|| format!("failed to list tests for {PACKAGE}"))?;
    if !output.status.success() {
        bail!(
            "{GATE}: failed to list tests for {PACKAGE}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected_line = format!("{TEST_NAME}: test");
    if !stdout.lines().any(|line| line == expected_line) {
        bail!("{GATE}: missing behavioral test: {PACKAGE} {TEST_NAME}");
    }
    Ok(())
}

fn cargo_test_command(filter: Option<&str>) -> ProcessCommand {
    let mut command = ProcessCommand::new("cargo");
    command
        .arg("test")
        .arg("-p")
        .arg(PACKAGE)
        .arg("--all-features");
    if let Some(filter) = filter {
        command.arg(filter);
    }
    command
}
