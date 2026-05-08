use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};

const FEATURE_MATRIX: &[MatrixCommand] = &[
    MatrixCommand {
        label: "cargo test -p gaze-recognizers --no-default-features",
        program: "cargo",
        args: &["test", "-p", "gaze-recognizers", "--no-default-features"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- safety-net-sanity",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "safety-net-sanity"],
    },
];

const REQUIRED_PACKAGE_TARGET: &str = "gaze-recognizers";
const REQUIRED_NO_DEFAULT_FEATURES: &str = "--no-default-features";
const REQUIRED_SAFETY_NET_SANITY_TASK: &str = "safety-net-sanity";

#[derive(Debug, Clone, Copy)]
struct MatrixCommand {
    label: &'static str,
    program: &'static str,
    args: &'static [&'static str],
}

pub fn run() -> Result<()> {
    ensure_matrix_contract()?;

    println!(
        "ci_feature_matrix: running {} feature-matrix commands",
        FEATURE_MATRIX.len()
    );
    for command in FEATURE_MATRIX {
        run_command(*command)?;
    }

    println!("ci_feature_matrix: passed");
    Ok(())
}

fn ensure_matrix_contract() -> Result<()> {
    if !FEATURE_MATRIX.iter().any(|command| {
        command.args.contains(&REQUIRED_PACKAGE_TARGET)
            && command.args.contains(&REQUIRED_NO_DEFAULT_FEATURES)
    }) {
        bail!(
            "ci_feature_matrix: feature matrix must test {} with {}",
            REQUIRED_PACKAGE_TARGET,
            REQUIRED_NO_DEFAULT_FEATURES
        );
    }

    if !FEATURE_MATRIX
        .iter()
        .any(|command| command.args.contains(&REQUIRED_SAFETY_NET_SANITY_TASK))
    {
        bail!(
            "ci_feature_matrix: feature matrix must run xtask {}",
            REQUIRED_SAFETY_NET_SANITY_TASK
        );
    }

    Ok(())
}

fn run_command(command: MatrixCommand) -> Result<()> {
    println!("ci_feature_matrix: running {}", command.label);
    let mut cmd = ProcessCommand::new(command.program);
    cmd.args(command.args);
    cmd.env_clear();
    // Keep matrix children deterministic: no caller Cargo/Rust pollution, only
    // process basics Cargo/rustup need to find toolchains and temp storage.
    for var in [
        "PATH",
        "HOME",
        "USER",
        "SHELL",
        "TMPDIR",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "LANG",
        "LC_ALL",
        "CI",
        "GITHUB_ACTIONS",
        "TERM",
        "COLORTERM",
    ] {
        if let Ok(value) = std::env::var(var) {
            cmd.env(var, value);
        }
    }
    if std::env::var_os("CARGO_HOME").is_none() {
        cmd.env(
            "CARGO_HOME",
            std::env::current_dir()
                .context("failed to resolve workspace for clean CARGO_HOME")?
                .join("target/ci-feature-matrix-cargo-home"),
        );
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed to run {}", command.label))?;
    if !status.success() {
        bail!("ci_feature_matrix: command failed: {}", command.label);
    }
    Ok(())
}
