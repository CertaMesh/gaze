//! `mcp-tier-isolation` xtask gate.
//!
//! Verifies that the agent-tier and operator-tier surfaces of `gaze-mcp-core`
//! stay partitioned by Cargo features. Drives the behavioral
//! `tier_isolation` integration test under multiple feature configurations
//! so the gate is anchored to runnable assertions, not to symbol-presence
//! string matches (per CLAUDE.md "symbol-or-string-presence-only checks are
//! recursive-Potemkin and forbidden").
//!
//! Behavioral test coverage:
//! - `--no-default-features` exercises
//!   `operator_tools_module_is_absent_without_operator_tier_feature`.
//! - default features (`core-tools`) exercises
//!   `core_tools_module_is_present_with_core_tools_feature`.
//! - `--features operator-tier` exercises
//!   `operator_tools_module_is_present_with_operator_tier_feature` (and the
//!   `core-tools` test is `#[cfg]`-gated, so it is silently skipped here).
//! - `--all-features` exercises the union of agent + operator surfaces.
//!
//! The deeper "linker cannot reach `Session::restore*` from agent-tier
//! builds" guarantee is enforced by the existing `dylint-gate` extension
//! (see `dylint.toml` `protected_paths`).

use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};

const CORE_PACKAGE: &str = "gaze-mcp-core";
const CORE_TEST_TARGET: &str = "tier_isolation";

const CORE_FEATURE_GRAPHS: &[FeatureGraph] = &[
    FeatureGraph {
        label: "default",
        cargo_args: &[],
    },
    FeatureGraph {
        label: "no-default-features",
        cargo_args: &["--no-default-features"],
    },
    FeatureGraph {
        label: "operator-tier-only",
        cargo_args: &["--no-default-features", "--features", "operator-tier"],
    },
    FeatureGraph {
        label: "all-features",
        cargo_args: &["--all-features"],
    },
];

const RMCP_PACKAGE: &str = "gaze-mcp-rmcp";
const RMCP_FEATURE_GRAPHS: &[FeatureGraph] = &[
    FeatureGraph {
        label: "transport-stdio",
        cargo_args: &["--no-default-features", "--features", "transport-stdio"],
    },
    FeatureGraph {
        label: "transport-stdio+transport-http",
        cargo_args: &[
            "--no-default-features",
            "--features",
            "transport-stdio,transport-http",
        ],
    },
    FeatureGraph {
        label: "no-default-features",
        cargo_args: &["--no-default-features"],
    },
];

#[derive(Clone, Copy)]
struct FeatureGraph {
    label: &'static str,
    cargo_args: &'static [&'static str],
}

pub fn run() -> Result<()> {
    println!(
        "mcp_tier_isolation: running {test} integration tests across {n} core feature graphs",
        test = CORE_TEST_TARGET,
        n = CORE_FEATURE_GRAPHS.len()
    );
    for graph in CORE_FEATURE_GRAPHS {
        run_core_under(*graph)?;
    }
    println!(
        "mcp_tier_isolation: checking {package} across {n} transport feature graphs",
        package = RMCP_PACKAGE,
        n = RMCP_FEATURE_GRAPHS.len()
    );
    for graph in RMCP_FEATURE_GRAPHS {
        run_rmcp_under(*graph)?;
    }
    println!("mcp_tier_isolation: passed");
    Ok(())
}

fn run_core_under(graph: FeatureGraph) -> Result<()> {
    println!(
        "mcp_tier_isolation: package={CORE_PACKAGE} graph={}",
        graph.label
    );
    let mut cmd = ProcessCommand::new("cargo");
    cmd.arg("test")
        .arg("-p")
        .arg(CORE_PACKAGE)
        .arg("--test")
        .arg(CORE_TEST_TARGET);
    cmd.args(graph.cargo_args);
    let status = cmd.status().with_context(|| {
        format!(
            "failed to spawn cargo test for {CORE_PACKAGE} graph={}",
            graph.label
        )
    })?;
    if !status.success() {
        bail!(
            "mcp_tier_isolation: {CORE_PACKAGE} graph={}: {} integration tests failed",
            graph.label,
            CORE_TEST_TARGET
        );
    }
    Ok(())
}

fn run_rmcp_under(graph: FeatureGraph) -> Result<()> {
    println!(
        "mcp_tier_isolation: package={RMCP_PACKAGE} graph={}",
        graph.label
    );
    let mut cmd = ProcessCommand::new("cargo");
    cmd.arg("check").arg("-p").arg(RMCP_PACKAGE);
    cmd.args(graph.cargo_args);
    let status = cmd.status().with_context(|| {
        format!(
            "failed to spawn cargo check for {RMCP_PACKAGE} graph={}",
            graph.label
        )
    })?;
    if !status.success() {
        bail!(
            "mcp_tier_isolation: {RMCP_PACKAGE} graph={}: cargo check failed",
            graph.label
        );
    }
    Ok(())
}
