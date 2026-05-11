use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};

const FEATURE_MATRIX: &[MatrixCommand] = &[
    MatrixCommand {
        label: "cargo fmt --all -- --check",
        program: "cargo",
        args: &["fmt", "--all", "--", "--check"],
    },
    MatrixCommand {
        label: "cargo clippy --workspace --all-features --all-targets -- -D warnings",
        program: "cargo",
        args: &[
            "clippy",
            "--workspace",
            "--all-features",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- bundle-tokenization-drift",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "bundle-tokenization-drift"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- bundle-tokenization-drift --verify-ack",
        program: "cargo",
        args: &[
            "run",
            "-p",
            "xtask",
            "--",
            "bundle-tokenization-drift",
            "--verify-ack",
        ],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- cargo-metadata-audit-isolation",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "cargo-metadata-audit-isolation"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- class-map-override-safety",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "class-map-override-safety"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- fixture-citation-lint",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "fixture-citation-lint"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- family-policy-table-coherence",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "family-policy-table-coherence"],
    },
    MatrixCommand {
        label: "cargo test -p gaze-recognizers --no-default-features",
        program: "cargo",
        args: &["test", "-p", "gaze-recognizers", "--no-default-features"],
    },
    // The removed hook called `xtask ci-feature-matrix`; omit it here to avoid
    // recursive self-execution while preserving the actual gate coverage.
    MatrixCommand {
        label: "cargo run -p xtask -- no-tenant-knowledge",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "no-tenant-knowledge"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- safety-net-sanity",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "safety-net-sanity"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- symmetric-potemkin",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "symmetric-potemkin"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- recognizer-composition-validator",
        program: "cargo",
        args: &[
            "run",
            "-p",
            "xtask",
            "--",
            "recognizer-composition-validator",
        ],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- dylint-gate",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "dylint-gate"],
    },
    MatrixCommand {
        label: "cargo test --workspace --all-features",
        program: "cargo",
        args: &["test", "--workspace", "--all-features"],
    },
    MatrixCommand {
        label: "cargo test --workspace --all-targets",
        program: "cargo",
        args: &["test", "--workspace", "--all-targets"],
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
