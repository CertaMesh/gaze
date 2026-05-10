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

const PACKAGE: &str = "gaze-mcp-core";
const TEST_TARGET: &str = "tier_isolation";

const FEATURE_GRAPHS: &[FeatureGraph] = &[
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

#[derive(Clone, Copy)]
struct FeatureGraph {
    label: &'static str,
    cargo_args: &'static [&'static str],
}

pub fn run() -> Result<()> {
    println!(
        "mcp_tier_isolation: running {test} integration tests across {n} feature graphs",
        test = TEST_TARGET,
        n = FEATURE_GRAPHS.len()
    );
    for graph in FEATURE_GRAPHS {
        run_under(*graph)?;
    }
    println!("mcp_tier_isolation: passed");
    Ok(())
}

fn run_under(graph: FeatureGraph) -> Result<()> {
    println!("mcp_tier_isolation: graph={}", graph.label);
    let mut cmd = ProcessCommand::new("cargo");
    cmd.arg("test")
        .arg("-p")
        .arg(PACKAGE)
        .arg("--test")
        .arg(TEST_TARGET);
    cmd.args(graph.cargo_args);
    let status = cmd
        .status()
        .with_context(|| format!("failed to spawn cargo test for graph={}", graph.label))?;
    if !status.success() {
        bail!(
            "mcp_tier_isolation: graph={}: {} integration tests failed",
            graph.label,
            TEST_TARGET
        );
    }
    Ok(())
}
