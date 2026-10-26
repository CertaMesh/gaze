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
    CORE_NO_PHONE_PARSER_LOAD_GUARD,
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
        label: "cargo test -p gaze-cli --no-default-features --test nym_no_feature",
        program: "cargo",
        args: &[
            "test",
            "-p",
            "gaze-cli",
            "--no-default-features",
            "--test",
            "nym_no_feature",
        ],
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
        label: "cargo test --workspace --all-features",
        program: "cargo",
        args: &["test", "--workspace", "--all-features"],
    },
    DEFAULT_WORKSPACE_SUITE,
];

/// The default-feature suite denies warnings, so a warning that only appears
/// without `--all-features` (for example in `gaze-cli` test targets) fails a PR
/// instead of surfacing at publish time. It reuses the suite's own build.
///
/// The flag goes through `--config build.rustflags`, not the `RUSTFLAGS`
/// environment variable: test binaries inherit the environment, and nested
/// cargo/trybuild builds inside tests must keep their own warning policy.
const DEFAULT_WORKSPACE_SUITE: MatrixCommand = MatrixCommand {
    label: "cargo test --config build.rustflags=['-D','warnings'] --workspace --lib --bins --tests",
    program: "cargo",
    args: &[
        "test",
        "--config",
        "build.rustflags=['-D','warnings']",
        "--workspace",
        "--lib",
        "--bins",
        "--tests",
    ],
};

const REQUIRED_PACKAGE_TARGET: &str = "gaze-recognizers";
const REQUIRED_NO_DEFAULT_FEATURES: &str = "--no-default-features";
const REQUIRED_NO_PHONE_PARSER_TEST_TARGET: &str = "no_phone_parser_fail_closed";
const REQUIRED_NO_PHONE_PARSER_TEST_COUNT: &str = "running 2 tests";
const REQUIRED_EMBEDDED_CORE_LOAD_RESULT: &str = "embedded_core_loads_without_phone_parser ... ok";
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
const CORE_NO_PHONE_PARSER_LOAD_GUARD: MatrixCommand = MatrixCommand {
    label: "cargo test -p gaze-pii --no-default-features --lib",
    program: "cargo",
    args: &["test", "-p", "gaze-pii", "--no-default-features", "--lib"],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MatrixCommand {
    label: &'static str,
    program: &'static str,
    args: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Partition {
    Full,
    CiGates,
    CiDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CiOwner {
    Test,
    FeatureGates,
    DefaultWorkspace,
}

fn ci_owner(command: &MatrixCommand) -> CiOwner {
    match command.args {
        ["fmt", "--all", "--", "--check"]
        | ["clippy", "--workspace", "--all-features", "--all-targets", "--", "-D", "warnings"]
        | ["test", "-p", "gaze-document", "--features", "mcp"]
        | ["test", "-p", "gaze-cli", "--features", "mcp"]
        | ["test", "--workspace", "--all-features"] => CiOwner::Test,
        ["test", "--config", "build.rustflags=['-D','warnings']", "--workspace", "--lib", "--bins", "--tests"] => {
            CiOwner::DefaultWorkspace
        }
        _ => CiOwner::FeatureGates,
    }
}

fn partition_commands(partition: Partition) -> impl Iterator<Item = &'static MatrixCommand> {
    FEATURE_MATRIX
        .iter()
        .filter(move |command| match partition {
            Partition::Full => true,
            Partition::CiGates => ci_owner(command) == CiOwner::FeatureGates,
            Partition::CiDefault => ci_owner(command) == CiOwner::DefaultWorkspace,
        })
}

pub fn run(partition: Partition) -> Result<()> {
    if partition != Partition::CiDefault {
        run_library_graphs()?;
        run_command_requiring_output(
            NO_PHONE_PARSER_FAIL_CLOSED_GUARD,
            REQUIRED_NO_PHONE_PARSER_TEST_COUNT,
        )?;
    }

    println!(
        "ci_feature_matrix: running {} feature-matrix commands",
        partition_commands(partition).count()
    );
    for command in partition_commands(partition) {
        if *command == CORE_NO_PHONE_PARSER_LOAD_GUARD {
            run_command_requiring_output(*command, REQUIRED_EMBEDDED_CORE_LOAD_RESULT)?;
        } else if command.args.contains(&"nym_no_feature") {
            run_command_requiring_output(
                *command,
                "test policy_nym_refuses_when_binary_lacks_feature ... ok",
            )?;
        } else {
            run_command(*command)?;
        }
    }

    println!("ci_feature_matrix: passed");
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

/// Library graphs a published adopter compiles, without dev-dependency unification.
///
/// `cargo test -p <crate>` builds the crate's dev-dependencies into the same
/// graph, and Cargo feature unification lets them re-enable library features.
/// Under resolver 2 a plain `cargo check -p <crate>` (lib and bins) and a
/// `cargo run` of a bin never activate dev-dependencies, so these commands see
/// exactly what an adopter of each published crate compiles and runs.
const LIBRARY_GRAPH_RUSTFLAGS: &str = "-D warnings";
const FEATURE_GRAPH_PROBE_PACKAGE: &str = "feature-graph-probe";

/// One runtime probe: a `feature-graph-probe` feature that selects one
/// published library graph, and the exact line the probe must print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GraphProbe {
    library: &'static str,
    graph: &'static str,
    feature: &'static str,
    required_output: &'static str,
}

const FEATURE_GRAPH_PROBES: &[GraphProbe] = &[
    GraphProbe {
        library: "gaze-pii",
        graph: "--no-default-features",
        feature: "gaze-pii-no-default",
        required_output:
            "feature-graph-probe: gaze-pii embedded core rulepack loaded, phone-parser=off ok",
    },
    GraphProbe {
        library: "gaze-pii",
        graph: "default features",
        feature: "gaze-pii-default",
        required_output:
            "feature-graph-probe: gaze-pii embedded core rulepack loaded, phone-parser=on ok",
    },
    GraphProbe {
        library: "gaze-assembly",
        graph: "default features",
        feature: "gaze-assembly-default",
        required_output:
            "feature-graph-probe: gaze-assembly bundled core pipeline tokenized email, phone-parser=on ok",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryGraphCommand {
    label: String,
    args: Vec<String>,
    rustflags: Option<&'static str>,
    required_output: Option<&'static str>,
}

/// Plans the runtime probes, then a deny-warnings `cargo check` of every
/// published crate under its default and `--no-default-features` graphs.
fn library_graph_commands(published: &[String]) -> Result<Vec<LibraryGraphCommand>> {
    if published.is_empty() {
        bail!("ci_feature_matrix: publish plan listed no crates");
    }
    for probe in FEATURE_GRAPH_PROBES {
        if !published.iter().any(|name| name == probe.library) {
            bail!(
                "ci_feature_matrix: feature-graph probe `{}` targets unpublished crate {}",
                probe.feature,
                probe.library
            );
        }
    }
    // Probes first: they are cheap and catch the adopter-visible break.
    let mut commands = Vec::new();
    for probe in FEATURE_GRAPH_PROBES {
        let args: Vec<String> = [
            "run",
            "--locked",
            "-p",
            FEATURE_GRAPH_PROBE_PACKAGE,
            "--no-default-features",
            "--features",
            probe.feature,
        ]
        .map(str::to_string)
        .to_vec();
        commands.push(LibraryGraphCommand {
            label: format!(
                "cargo {} ({} {})",
                args.join(" "),
                probe.library,
                probe.graph
            ),
            args,
            rustflags: None,
            required_output: Some(probe.required_output),
        });
    }
    for package in published {
        for graph in [None, Some("--no-default-features")] {
            let mut args = vec![
                "check".to_string(),
                "--locked".to_string(),
                "-p".to_string(),
                package.clone(),
            ];
            args.extend(graph.map(str::to_string));
            commands.push(LibraryGraphCommand {
                label: format!(
                    "RUSTFLAGS='{LIBRARY_GRAPH_RUSTFLAGS}' cargo {}",
                    args.join(" ")
                ),
                args,
                rustflags: Some(LIBRARY_GRAPH_RUSTFLAGS),
                required_output: None,
            });
        }
    }
    Ok(commands)
}

fn run_library_graphs() -> Result<()> {
    let root = crate::repo::repo_root()?;
    let commands = library_graph_commands(&crate::publish_plan::published_package_names(&root)?)?;
    println!(
        "ci_feature_matrix: running {} library-graph commands without dev-dependencies",
        commands.len()
    );
    for command in &commands {
        println!("ci_feature_matrix: running {}", command.label);
        let mut cmd = clean_command("cargo", &command.args)?;
        if let Some(rustflags) = command.rustflags {
            cmd.env("RUSTFLAGS", rustflags);
        }
        let output = cmd
            .output()
            .with_context(|| format!("failed to run {}", command.label))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        print!("{stdout}");
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        if !output.status.success() {
            bail!("ci_feature_matrix: command failed: {}", command.label);
        }
        if let Some(required) = command.required_output {
            if !stdout.lines().any(|line| line == required) {
                bail!(
                    "ci_feature_matrix: command {} must report `{required}`",
                    command.label
                );
            }
        }
    }
    Ok(())
}

fn configured_command(command: MatrixCommand) -> Result<ProcessCommand> {
    clean_command(command.program, command.args)
}

fn clean_command<S: AsRef<std::ffi::OsStr>>(program: &str, args: &[S]) -> Result<ProcessCommand> {
    let mut cmd = ProcessCommand::new(program);
    cmd.args(args);
    cmd.env_clear();
    // Bound artifacts and retain a caller's stricter shared-machine job cap.
    let jobs = std::env::var("CARGO_BUILD_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|jobs| *jobs > 0)
        .unwrap_or(4)
        .min(4);
    cmd.env("GAZE_DISABLE_OS_KEYSTORE", "1")
        .env("CARGO_BUILD_JOBS", jobs.to_string())
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_TEST_DEBUG", "0");
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

#[cfg(test)]
#[path = "ci_feature_matrix_tests.rs"]
mod tests;
