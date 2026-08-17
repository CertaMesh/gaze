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
        label: "cargo run -p xtask -- trybuild-fixture-hygiene",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "trybuild-fixture-hygiene"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- family-policy-table-coherence",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "family-policy-table-coherence"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- locale-cue-bundle-coherence",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "locale-cue-bundle-coherence"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- readme-version-check",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "readme-version-check"],
    },
    MatrixCommand {
        label: "cargo test -p gaze-recognizers --no-default-features",
        program: "cargo",
        args: &["test", "-p", "gaze-recognizers", "--no-default-features"],
    },
    MatrixCommand {
        label: "cargo test -p gaze-document --features mcp",
        program: "cargo",
        args: &["test", "-p", "gaze-document", "--features", "mcp"],
    },
    MatrixCommand {
        label: "cargo test -p gaze-cli",
        program: "cargo",
        args: &["test", "-p", "gaze-cli"],
    },
    MatrixCommand {
        label: "cargo test -p gaze-cli --features mcp",
        program: "cargo",
        args: &["test", "-p", "gaze-cli", "--features", "mcp"],
    },
    MatrixCommand {
        label: "cargo test -p gaze-cli --features dashboard",
        program: "cargo",
        args: &["test", "-p", "gaze-cli", "--features", "dashboard"],
    },
    MatrixCommand {
        label: "cargo test -p gaze-proxy-dashboard",
        program: "cargo",
        args: &["test", "-p", "gaze-proxy-dashboard"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- dashboard-isolation",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "dashboard-isolation"],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- mcp-tier-isolation",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "mcp-tier-isolation"],
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
        label: "cargo run -p xtask -- tokenbridge-no-raw-index",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "tokenbridge-no-raw-index"],
    },
    MatrixCommand {
        label: "cargo build -p gaze-token-bridge --features os-keychain",
        program: "cargo",
        args: &[
            "build",
            "-p",
            "gaze-token-bridge",
            "--features",
            "os-keychain",
        ],
    },
    MatrixCommand {
        label: "cargo test -p gaze-token-bridge --features os-keychain",
        program: "cargo",
        args: &[
            "test",
            "-p",
            "gaze-token-bridge",
            "--features",
            "os-keychain",
        ],
    },
    MatrixCommand {
        label: "cargo run -p xtask -- tokenbridge-encrypted-index",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "tokenbridge-encrypted-index"],
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
        label: "cargo run -p xtask -- dylint-gate  # ui-fixture-shape only in PR CI; cargo-dylint runs in dylint.yml",
        program: "cargo",
        args: &["run", "-p", "xtask", "--", "dylint-gate"],
    },
    MatrixCommand {
        label: "cargo test --workspace --all-features",
        program: "cargo",
        args: &["test", "--workspace", "--all-features"],
    },
    MatrixCommand {
        label: "cargo test --workspace --lib --bins --tests",
        program: "cargo",
        args: &["test", "--workspace", "--lib", "--bins", "--tests"],
    },
];

const REQUIRED_PACKAGE_TARGET: &str = "gaze-recognizers";
const REQUIRED_NO_DEFAULT_FEATURES: &str = "--no-default-features";
const REQUIRED_NO_PHONE_PARSER_TEST_TARGET: &str = "no_phone_parser_fail_closed";
const REQUIRED_NO_PHONE_PARSER_TEST_COUNT: &str = "running 2 tests";
const REQUIRED_SAFETY_NET_SANITY_TASK: &str = "safety-net-sanity";
const REQUIRED_README_VERSION_CHECK_TASK: &str = "readme-version-check";
const REQUIRED_TOKENBRIDGE_ENCRYPTED_INDEX_TASK: &str = "tokenbridge-encrypted-index";
const REQUIRED_TRYBUILD_FIXTURE_HYGIENE_TASK: &str = "trybuild-fixture-hygiene";
const REQUIRED_DASHBOARD_ISOLATION_TASK: &str = "dashboard-isolation";
const REQUIRED_MCP_TIER_ISOLATION_TASK: &str = "mcp-tier-isolation";
const NO_PHONE_PARSER_FAIL_CLOSED_GUARD: MatrixCommand = MatrixCommand {
    label:
        "cargo test -p gaze-recognizers --no-default-features --test no_phone_parser_fail_closed",
    program: "cargo",
    args: &[
        "test",
        "-p",
        REQUIRED_PACKAGE_TARGET,
        REQUIRED_NO_DEFAULT_FEATURES,
        "--test",
        REQUIRED_NO_PHONE_PARSER_TEST_TARGET,
    ],
};

#[derive(Debug, Clone, Copy)]
struct MatrixCommand {
    label: &'static str,
    program: &'static str,
    args: &'static [&'static str],
}

pub fn run() -> Result<()> {
    ensure_matrix_contract()?;

    run_command_requiring_output(
        NO_PHONE_PARSER_FAIL_CLOSED_GUARD,
        REQUIRED_NO_PHONE_PARSER_TEST_COUNT,
    )?;

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

    if !FEATURE_MATRIX
        .iter()
        .any(|command| command.args.contains(&REQUIRED_README_VERSION_CHECK_TASK))
    {
        bail!(
            "ci_feature_matrix: feature matrix must run xtask {}",
            REQUIRED_README_VERSION_CHECK_TASK
        );
    }

    if !FEATURE_MATRIX.iter().any(|command| {
        command
            .args
            .contains(&REQUIRED_TOKENBRIDGE_ENCRYPTED_INDEX_TASK)
    }) {
        bail!(
            "ci_feature_matrix: feature matrix must run xtask {}",
            REQUIRED_TOKENBRIDGE_ENCRYPTED_INDEX_TASK
        );
    }

    if !FEATURE_MATRIX.iter().any(|command| {
        command
            .args
            .contains(&REQUIRED_TRYBUILD_FIXTURE_HYGIENE_TASK)
    }) {
        bail!(
            "ci_feature_matrix: feature matrix must run xtask {}",
            REQUIRED_TRYBUILD_FIXTURE_HYGIENE_TASK
        );
    }

    if !FEATURE_MATRIX
        .iter()
        .any(|command| command.args.contains(&REQUIRED_DASHBOARD_ISOLATION_TASK))
    {
        bail!(
            "ci_feature_matrix: feature matrix must run xtask {}",
            REQUIRED_DASHBOARD_ISOLATION_TASK
        );
    }

    if !FEATURE_MATRIX
        .iter()
        .any(|command| command.args.contains(&REQUIRED_MCP_TIER_ISOLATION_TASK))
    {
        bail!(
            "ci_feature_matrix: feature matrix must run xtask {}",
            REQUIRED_MCP_TIER_ISOLATION_TASK
        );
    }

    Ok(())
}

fn run_command(command: MatrixCommand) -> Result<()> {
    println!("ci_feature_matrix: running {}", command.label);
    let mut cmd = configured_command(command)?;

    let status = cmd
        .status()
        .with_context(|| format!("failed to run {}", command.label))?;
    if !status.success() {
        bail!("ci_feature_matrix: command failed: {}", command.label);
    }
    Ok(())
}

fn run_command_requiring_output(command: MatrixCommand, required_output: &str) -> Result<()> {
    println!("ci_feature_matrix: running {}", command.label);
    let mut cmd = configured_command(command)?;

    let output = cmd
        .output()
        .with_context(|| format!("failed to run {}", command.label))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stdout}");
    eprint!("{stderr}");
    if !output.status.success() {
        bail!("ci_feature_matrix: command failed: {}", command.label);
    }
    if !stdout.contains(required_output) && !stderr.contains(required_output) {
        bail!(
            "ci_feature_matrix: command {} must report `{}`",
            command.label,
            required_output
        );
    }
    Ok(())
}

fn configured_command(command: MatrixCommand) -> Result<ProcessCommand> {
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
        "RUSTC",
        "RUSTDOC",
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

    Ok(cmd)
}
