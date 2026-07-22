//! `dashboard-isolation` xtask gate.
//!
//! Enforces the Track B dashboard boundary behaviorally:
//!
//! 1. `gaze-proxy-dashboard`'s Gaze-crate dependency set is exactly
//!    `gaze-inspection` + `gaze-types` (the Revision 60 two-crate allowlist).
//!    `gaze-proxy`, provider adapters, and every other workspace crate are
//!    forbidden edges in every dependency kind.
//! 2. The `gaze-cli` default feature graph excludes the dashboard crate:
//!    the dashboard ships strictly opt-in behind the default-off `dashboard`
//!    feature.
//! 3. The boundary-relevant behavioral suites run: the dashboard crate's
//!    `sink_isolation`, `process_isolation`, and `runtime_lifecycle`
//!    integration tests plus the `gaze-cli` `proxy_dashboard` integration
//!    suite under `--features dashboard`. The gate is anchored to runnable
//!    assertions, not to symbol-presence string matches (per the AGENTS.md /
//!    CLAUDE.md rule that presence-only checks are recursive-Potemkin and
//!    forbidden).

use std::collections::BTreeSet;
use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const DASHBOARD_PACKAGE: &str = "gaze-proxy-dashboard";
const CLI_PACKAGE: &str = "gaze-cli";
const ALLOWED_GAZE_EDGES: &[&str] = &["gaze-inspection", "gaze-types"];

const DASHBOARD_BEHAVIORAL_SUITES: &[&str] =
    &["sink_isolation", "process_isolation", "runtime_lifecycle"];

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

/// A dashboard dependency edge grouped by cargo dependency kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardEdges {
    /// Names of `gaze-*` crates in the dashboard's normal dependency list.
    pub normal_gaze: Vec<String>,
    /// Names of `gaze-*` crates in the dashboard's dev/build dependency lists.
    pub non_normal_gaze: Vec<String>,
}

/// Extracts the dashboard crate's Gaze-crate edges from `cargo metadata`
/// JSON. A missing workspace package is an error so a renamed or removed
/// crate can never turn this gate into a silent no-op.
pub fn parse_dashboard_edges(metadata_json: &str) -> Result<DashboardEdges> {
    let metadata: Metadata =
        serde_json::from_str(metadata_json).context("failed to parse cargo metadata JSON")?;
    let package = metadata
        .packages
        .iter()
        .find(|package| {
            package.name == DASHBOARD_PACKAGE
                && metadata
                    .workspace_members
                    .iter()
                    .any(|member| member == &package.id)
        })
        .with_context(|| {
            format!("workspace member `{DASHBOARD_PACKAGE}` not found in cargo metadata")
        })?;
    let mut normal_gaze = Vec::new();
    let mut non_normal_gaze = Vec::new();
    for dependency in &package.dependencies {
        if !dependency.name.starts_with("gaze") {
            continue;
        }
        match dependency.kind.as_deref() {
            None | Some("normal") => normal_gaze.push(dependency.name.clone()),
            Some(_) => non_normal_gaze.push(dependency.name.clone()),
        }
    }
    Ok(DashboardEdges {
        normal_gaze,
        non_normal_gaze,
    })
}

/// Verifies the Revision 60 two-crate allowlist: the normal Gaze edge set is
/// exactly `gaze-inspection` + `gaze-types`, and no Gaze crate at all rides
/// in through dev or build dependencies.
pub fn verify_dashboard_gaze_edges(edges: &DashboardEdges) -> Result<()> {
    let actual: BTreeSet<&str> = edges.normal_gaze.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = ALLOWED_GAZE_EDGES.iter().copied().collect();
    if actual != expected {
        bail!(
            "dashboard_isolation: `{DASHBOARD_PACKAGE}` normal Gaze dependencies must be exactly {expected:?}, found {actual:?}"
        );
    }
    if !edges.non_normal_gaze.is_empty() {
        bail!(
            "dashboard_isolation: `{DASHBOARD_PACKAGE}` must not reach Gaze crates through dev/build dependencies, found {:?}",
            edges.non_normal_gaze
        );
    }
    Ok(())
}

/// Verifies the default-off proof: `cargo tree -p gaze-cli -e normal` under
/// default features must not resolve the dashboard crate.
pub fn verify_default_cli_graph(tree_output: &str) -> Result<()> {
    if tree_output.contains(DASHBOARD_PACKAGE) {
        bail!(
            "dashboard_isolation: `{CLI_PACKAGE}` default feature graph must not contain `{DASHBOARD_PACKAGE}`; the dashboard ships default-off"
        );
    }
    Ok(())
}

pub fn run() -> Result<()> {
    println!("dashboard_isolation: checking {DASHBOARD_PACKAGE} Gaze dependency edges");
    let metadata_output = ProcessCommand::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .context("failed to spawn cargo metadata")?;
    if !metadata_output.status.success() {
        bail!("dashboard_isolation: cargo metadata failed");
    }
    let metadata_json = String::from_utf8(metadata_output.stdout)
        .context("cargo metadata emitted non-UTF-8 output")?;
    let edges = parse_dashboard_edges(&metadata_json)?;
    verify_dashboard_gaze_edges(&edges)?;

    println!("dashboard_isolation: checking {CLI_PACKAGE} default graph excludes the dashboard");
    let tree_output = ProcessCommand::new("cargo")
        .args(["tree", "-p", CLI_PACKAGE, "-e", "normal", "--locked"])
        .output()
        .context("failed to spawn cargo tree")?;
    if !tree_output.status.success() {
        bail!("dashboard_isolation: cargo tree -p {CLI_PACKAGE} failed");
    }
    let tree_text =
        String::from_utf8(tree_output.stdout).context("cargo tree emitted non-UTF-8 output")?;
    verify_default_cli_graph(&tree_text)?;

    for suite in DASHBOARD_BEHAVIORAL_SUITES {
        println!("dashboard_isolation: running {DASHBOARD_PACKAGE} --test {suite}");
        run_cargo_test(&["test", "-p", DASHBOARD_PACKAGE, "--test", suite])?;
    }
    println!(
        "dashboard_isolation: running {CLI_PACKAGE} --features dashboard --test proxy_dashboard"
    );
    run_cargo_test(&[
        "test",
        "-p",
        CLI_PACKAGE,
        "--features",
        "dashboard",
        "--test",
        "proxy_dashboard",
    ])?;

    println!("dashboard_isolation: passed");
    Ok(())
}

fn run_cargo_test(args: &[&str]) -> Result<()> {
    let status = ProcessCommand::new("cargo")
        .args(args)
        .arg("--locked")
        .status()
        .with_context(|| format!("failed to spawn cargo {args:?}"))?;
    if !status.success() {
        bail!("dashboard_isolation: cargo {args:?} failed");
    }
    Ok(())
}
