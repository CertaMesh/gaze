use std::collections::{hash_map::Entry, HashMap};
use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BehavioralTarget {
    All,
    Lib,
    Bin(&'static str),
    Integration(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BehavioralTest {
    pub(crate) package: &'static str,
    pub(crate) target: BehavioralTarget,
    pub(crate) name: &'static str,
}

pub(crate) fn ensure_tests_exist(tests: &[BehavioralTest]) -> Result<()> {
    let mut listed = HashMap::<(&'static str, BehavioralTarget), String>::new();

    for test in tests {
        let key = (test.package, test.target);
        let stdout = match listed.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(list_tests(*test)?),
        };
        let expected_line = format!("{}: test", test.name);
        if !stdout.lines().any(|line| line == expected_line) {
            bail!("missing behavioral test: {}", describe(*test));
        }
    }

    Ok(())
}

pub(crate) fn ensure_test_exists(test: BehavioralTest) -> Result<()> {
    ensure_tests_exist(&[test])
}

pub(crate) fn list_tests(test: BehavioralTest) -> Result<String> {
    list_command_output(list_test_command(test), &describe(test))
}

pub(crate) fn run_behavioral_test(gate: &str, test: BehavioralTest) -> Result<()> {
    println!("{gate}: running {}", describe(test));
    let status = exact_test_command(test)
        .status()
        .with_context(|| format!("failed to run {}", describe(test)))?;
    if !status.success() {
        bail!("behavioral test failed: {}", describe(test));
    }
    Ok(())
}

pub(crate) fn cargo_test_command(package: &str, target: BehavioralTarget) -> ProcessCommand {
    let mut command = ProcessCommand::new("cargo");
    command.arg("test").arg("-p").arg(package);
    match target {
        BehavioralTarget::All => {}
        BehavioralTarget::Lib => {
            command.arg("--lib");
        }
        BehavioralTarget::Bin(name) => {
            command.arg("--bin").arg(name);
        }
        BehavioralTarget::Integration(name) => {
            command.arg("--test").arg(name);
        }
    }
    command
}

pub(crate) fn list_test_command(test: BehavioralTest) -> ProcessCommand {
    list_test_command_with_args(test, &[])
}

pub(crate) fn list_test_command_with_args(
    test: BehavioralTest,
    cargo_args: &[&str],
) -> ProcessCommand {
    let mut command = cargo_test_command(test.package, test.target);
    command.args(cargo_args);
    command.arg("--").arg("--list");
    command
}

pub(crate) fn exact_test_command(test: BehavioralTest) -> ProcessCommand {
    exact_test_command_with_args(test, &[])
}

pub(crate) fn exact_test_command_with_args(
    test: BehavioralTest,
    cargo_args: &[&str],
) -> ProcessCommand {
    let mut command = cargo_test_command(test.package, test.target);
    command.args(cargo_args);
    command.arg(test.name).arg("--").arg("--exact");
    command
}

pub(crate) fn describe(test: BehavioralTest) -> String {
    match test.target {
        BehavioralTarget::All => format!("{} {}", test.package, test.name),
        BehavioralTarget::Lib => format!("{} --lib {}", test.package, test.name),
        BehavioralTarget::Bin(name) => {
            format!("{} --bin {} {}", test.package, name, test.name)
        }
        BehavioralTarget::Integration(name) => {
            format!("{} --test {} {}", test.package, name, test.name)
        }
    }
}

pub(crate) fn list_command_output(
    mut command: ProcessCommand,
    description: &str,
) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("failed to list tests for {description}"))?;
    if !output.status.success() {
        bail!(
            "failed to list tests for {description}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("failed to decode test list for {description}"))
}
