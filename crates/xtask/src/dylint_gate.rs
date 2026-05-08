use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

const EXPECTED_UI_FIXTURES: usize = 18;
const RUN_DYLINT_ENV: &str = "GAZE_RUN_DYLINT";

pub fn run() -> Result<()> {
    let root = std::env::current_dir().context("failed to resolve current directory")?;
    assert_ui_fixture_shape(&root)?;
    if !should_run_cargo_dylint() {
        eprintln!(
            "dylint_gate: cargo-dylint skipped outside CI; set {RUN_DYLINT_ENV}=1 to run it locally."
        );
        println!("dylint_gate: passed");
        return Ok(());
    }
    if !cargo_dylint_available() {
        eprintln!("cargo-dylint not installed; skipping dylint gate. CI installs it explicitly.");
        return Ok(());
    }
    run_cargo_dylint(&root)?;
    println!("dylint_gate: passed");
    Ok(())
}

fn should_run_cargo_dylint() -> bool {
    env_flag("CI") || env_flag("GITHUB_ACTIONS") || env_flag(RUN_DYLINT_ENV)
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
    let ui_root = root.join("xtask/dylint/ui");
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
