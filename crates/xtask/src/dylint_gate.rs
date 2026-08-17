//! The scheduled `dylint.yml` workflow sets `GAZE_DYLINT_REQUIRED=1` and must
//! fail closed if cargo-dylint is unavailable. Other callers still verify the
//! UI fixture shape, but report that the compiled lint run is deferred.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

const EXPECTED_UI_FIXTURES: usize = 18;
const DYLINT_REQUIRED_ENV: &str = "GAZE_DYLINT_REQUIRED";
const DEFERRED_MESSAGE: &str = "dylint_gate: ui-fixture-shape passed; cargo-dylint DEFERRED to the scheduled dylint.yml workflow (solo todo #1870)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DylintDisposition {
    Run,
    Deferred,
}

pub fn run() -> Result<()> {
    let root = std::env::current_dir().context("failed to resolve current directory")?;
    assert_ui_fixture_shape(&root)?;
    match cargo_dylint_requirement(env_flag(DYLINT_REQUIRED_ENV), cargo_dylint_available())? {
        DylintDisposition::Run => {
            run_cargo_dylint(&root)?;
            println!("dylint_gate: passed");
        }
        DylintDisposition::Deferred => println!("{DEFERRED_MESSAGE}"),
    }
    Ok(())
}

fn cargo_dylint_requirement(required: bool, available: bool) -> Result<DylintDisposition> {
    if available {
        return Ok(DylintDisposition::Run);
    }
    if required {
        bail!("dylint_gate: cargo-dylint is required by {DYLINT_REQUIRED_ENV}=1 but was not found");
    }
    Ok(DylintDisposition::Deferred)
}

fn env_flag(var: &str) -> bool {
    env::var(var).is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

fn cargo_dylint_available() -> bool {
    Command::new("cargo")
        .args(["dylint", "--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn assert_ui_fixture_shape(root: &Path) -> Result<()> {
    let ui_root = root.join("lint/dylint/ui");
    let fixtures = collect_rs_files(&ui_root)?;
    let disabled = fixtures
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_disabled.rs"))
        })
        .collect::<Vec<_>>();
    if !disabled.is_empty() {
        bail!(
            "dylint_gate: disabled UI fixtures are forbidden: {}",
            disabled
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let fixture_count = fixtures
        .iter()
        .filter(|path| {
            !path
                .components()
                .any(|component| component.as_os_str() == "auxiliary")
        })
        .count();
    if fixture_count != EXPECTED_UI_FIXTURES {
        bail!("dylint_gate: expected {EXPECTED_UI_FIXTURES} UI fixtures, found {fixture_count}");
    }

    println!("dylint_gate: verified {fixture_count} UI fixtures");
    Ok(())
}

fn collect_rs_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_rs_files_inner(root, &mut files)?;
    Ok(files)
}

fn collect_rs_files_inner(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let entry = entry.with_context(|| format!("failed to read entry in {}", path.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files_inner(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn run_cargo_dylint(root: &Path) -> Result<()> {
    let status = Command::new("cargo")
        .args(["dylint", "--workspace", "--all"])
        .current_dir(root)
        .status()
        .context("failed to run cargo dylint")?;
    if !status.success() {
        bail!("dylint_gate: cargo dylint --workspace --all failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_cargo_dylint_fails_closed_when_required() {
        let error = cargo_dylint_requirement(true, false).unwrap_err();

        assert_eq!(
            error.to_string(),
            "dylint_gate: cargo-dylint is required by GAZE_DYLINT_REQUIRED=1 but was not found"
        );
    }

    #[test]
    fn missing_cargo_dylint_reports_deferred_when_not_required() {
        assert_eq!(
            cargo_dylint_requirement(false, false).unwrap(),
            DylintDisposition::Deferred
        );
        assert_eq!(
            DEFERRED_MESSAGE,
            "dylint_gate: ui-fixture-shape passed; cargo-dylint DEFERRED to the scheduled dylint.yml workflow (solo todo #1870)"
        );
    }

    #[test]
    fn available_cargo_dylint_runs_even_when_not_required() {
        assert_eq!(
            cargo_dylint_requirement(false, true).unwrap(),
            DylintDisposition::Run
        );
    }
}
