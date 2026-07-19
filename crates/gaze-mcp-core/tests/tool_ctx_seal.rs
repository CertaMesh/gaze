//! Compile-fail fixtures verifying the [`gaze_mcp_core::ToolCtx`] seal:
//! external crates must NOT be able to construct a `ToolCtx` via either
//! [`gaze_mcp_core::ctx::ToolCtx::new_with_resources`] (private constructor) or a struct
//! literal (private fields + `#[non_exhaustive]`).
//!
//! The fixtures live in `tests/ui/`; this driver runs them via trybuild.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn assert_trybuild_rustc_matches_workspace() {
    let Some(toolchain_file) = workspace_toolchain_file() else {
        return;
    };
    let channel = pinned_toolchain_channel(&toolchain_file);
    let rustc_from_env = std::env::var_os("RUSTC");
    let compiler_source = if rustc_from_env.is_some() {
        "RUSTC"
    } else {
        "PATH rustc"
    };
    let rustc = rustc_from_env.unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .args(["--version", "--verbose"])
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "trybuild compiler guard could not run the compiler selected by {compiler_source} with `--version --verbose`: {error}"
            )
        });
    assert!(
        output.status.success(),
        "trybuild compiler guard: compiler selected by {compiler_source} failed `--version --verbose` with status {}",
        output.status
    );
    let verbose = String::from_utf8(output.stdout)
        .expect("trybuild compiler guard: rustc verbose version output was not UTF-8");
    let release = verbose
        .lines()
        .find_map(|line| line.strip_prefix("release:").map(str::trim))
        .expect("trybuild compiler guard: rustc verbose version output omitted `release:`");
    assert_eq!(
        release, channel,
        "trybuild rustc identity mismatch: compiler selected by {compiler_source} reports release `{release}`, but workspace rust-toolchain.toml pins `{channel}`. Bind Cargo children explicitly: `RUSTC=$(rustup which --toolchain {channel} rustc) RUSTDOC=$(rustup which --toolchain {channel} rustdoc) cargo test ...`"
    );
}

fn workspace_toolchain_file() -> Option<PathBuf> {
    for root in Path::new(env!("CARGO_MANIFEST_DIR")).ancestors() {
        let toolchain_file = root.join("rust-toolchain.toml");
        if !toolchain_file.is_file() {
            continue;
        }
        let manifest_file = root.join("Cargo.toml");
        if !manifest_file.is_file() {
            continue;
        }
        let manifest = fs::read_to_string(manifest_file).unwrap_or_else(|error| {
            panic!("trybuild compiler guard could not read workspace Cargo.toml: {error}")
        });
        if manifest.lines().any(|line| line.trim() == "[workspace]") {
            return Some(toolchain_file);
        }
    }
    None
}

fn pinned_toolchain_channel(toolchain_file: &Path) -> String {
    let source = fs::read_to_string(toolchain_file).unwrap_or_else(|error| {
        panic!("trybuild compiler guard could not read workspace rust-toolchain.toml: {error}")
    });
    let mut in_toolchain = false;
    for raw_line in source.lines() {
        let line = raw_line
            .split_once('#')
            .map_or(raw_line, |(before, _)| before)
            .trim();
        if line.starts_with('[') {
            in_toolchain = line == "[toolchain]";
            continue;
        }
        if !in_toolchain {
            continue;
        }
        let Some(value) = line.strip_prefix("channel") else {
            continue;
        };
        let Some(value) = value.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim();
        if let Some(value) = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
        {
            return value.to_owned();
        }
    }
    panic!("trybuild compiler guard: workspace rust-toolchain.toml omitted a quoted `[toolchain].channel`");
}

#[test]
fn external_construction_is_compile_fail() {
    assert_trybuild_rustc_matches_workspace();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
