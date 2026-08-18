//! `mcp-tier-isolation` xtask gate.
//!
//! Verifies that the agent-tier and operator-tier surfaces of `gaze-mcp-core`
//! stay partitioned by Cargo features.
//!
//! The partition itself is enforced by rustc, through the
//! `#[cfg(feature = "operator-tier")]` gates on `tools::{export, restore,
//! restore_strict}` (`crates/gaze-mcp-core/src/tools/mod.rs`) and on the
//! `operator_tools` re-export module (`crates/gaze-mcp-core/src/lib.rs`).
//! This gate is the check that those stay in place: it drives the behavioral
//! `tier_isolation` integration test under every relevant feature graph, so
//! the gate is anchored to runnable assertions, not to symbol-presence string
//! matches (per CLAUDE.md "symbol-or-string-presence-only checks are
//! recursive-Potemkin and forbidden").
//!
//! Behavioral test coverage:
//! - `--no-default-features` and default features exercise
//!   `operator_tier_surface_is_unreachable_from_an_agent_tier_build`, whose
//!   `trybuild` fixtures fail to compile only while the operator surface is
//!   genuinely unnameable from an agent-tier build.
//! - default features (`core-tools`) additionally exercise
//!   `core_tools_module_is_present_with_core_tools_feature`.
//! - `--features operator-tier` and `--all-features` exercise
//!   `operator_tools_module_is_present_with_operator_tier_feature` (the
//!   compile-fail test is `#[cfg]`-gated off once the surface is legitimately
//!   present).
//!
//! Each graph declares the tests it must observe passing. A `cargo test` run
//! reports success for zero tests, so without that list a deleted or
//! accidentally `#[cfg]`-ed-out test would leave the gate green while
//! checking nothing.
//!
//! Each graph also declares *how* it is driven (`GraphRun`), and both runners
//! enforce that declaration before spawning cargo. A `cargo check` graph names
//! no tests because nothing executes; a `cargo test` graph that names none is a
//! gate failure. Recording the difference keeps "nothing executes here" from
//! looking identical to "nobody filled this in" — an empty list meaning both at
//! once is the same vacuity this gate exists to catch.

use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};

const CORE_PACKAGE: &str = "gaze-mcp-core";
const CORE_TEST_TARGET: &str = "tier_isolation";

/// Proves an agent-tier build cannot name the operator surface.
const AGENT_TIER_UNREACHABLE_TEST: &str =
    "operator_tier_surface_is_unreachable_from_an_agent_tier_build";
/// Proves the agent-tier tools are present and tagged agent-tier.
const CORE_TOOLS_PRESENT_TEST: &str = "core_tools_module_is_present_with_core_tools_feature";
/// Proves the operator tools are present and tagged operator-tier.
const OPERATOR_TOOLS_PRESENT_TEST: &str =
    "operator_tools_module_is_present_with_operator_tier_feature";

const CORE_FEATURE_GRAPHS: &[FeatureGraph] = &[
    FeatureGraph {
        label: "default",
        cargo_args: &[],
        run: GraphRun::Tests,
        required_tests: &[CORE_TOOLS_PRESENT_TEST, AGENT_TIER_UNREACHABLE_TEST],
    },
    FeatureGraph {
        label: "no-default-features",
        cargo_args: &["--no-default-features"],
        run: GraphRun::Tests,
        required_tests: &[AGENT_TIER_UNREACHABLE_TEST],
    },
    FeatureGraph {
        label: "operator-tier-only",
        cargo_args: &["--no-default-features", "--features", "operator-tier"],
        run: GraphRun::Tests,
        required_tests: &[OPERATOR_TOOLS_PRESENT_TEST],
    },
    FeatureGraph {
        label: "all-features",
        cargo_args: &["--all-features"],
        run: GraphRun::Tests,
        required_tests: &[CORE_TOOLS_PRESENT_TEST, OPERATOR_TOOLS_PRESENT_TEST],
    },
];

const RMCP_PACKAGE: &str = "gaze-mcp-rmcp";
const RMCP_FEATURE_GRAPHS: &[FeatureGraph] = &[
    FeatureGraph {
        label: "transport-stdio",
        cargo_args: &["--no-default-features", "--features", "transport-stdio"],
        run: GraphRun::CheckOnly,
        required_tests: &[],
    },
    FeatureGraph {
        label: "transport-stdio+transport-http",
        cargo_args: &[
            "--no-default-features",
            "--features",
            "transport-stdio,transport-http",
        ],
        run: GraphRun::CheckOnly,
        required_tests: &[],
    },
    FeatureGraph {
        label: "no-default-features",
        cargo_args: &["--no-default-features"],
        run: GraphRun::CheckOnly,
        required_tests: &[],
    },
];

/// How a graph is driven, and therefore what it owes an observation of.
///
/// Declared per graph rather than inferred from an empty `required_tests`: a
/// graph that names no tests because nothing executes and a graph that names
/// none because nobody filled the list in must not look alike. Leaving those
/// two indistinguishable is the same vacuity this gate exists to catch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GraphRun {
    /// Driven with `cargo test`; must name the tests it observes passing.
    Tests,
    /// Driven with `cargo check`; nothing executes, so it names none.
    CheckOnly,
}

#[derive(Clone, Copy)]
struct FeatureGraph {
    label: &'static str,
    cargo_args: &'static [&'static str],
    /// How this graph is driven. Paired with `required_tests` by
    /// `graph_observation_contract`.
    run: GraphRun,
    /// Tests that must be observed passing in this graph's `cargo test`
    /// output. Empty exactly when `run` is `GraphRun::CheckOnly`.
    required_tests: &'static [&'static str],
}

/// Fail-closed precondition: a graph must be driven the way it declared, and
/// its `required_tests` must agree with that declaration.
///
/// Checked before spawning cargo, so the gate itself refuses to run a vacuous
/// graph rather than leaving it to a unit test to notice the omission later.
fn graph_observation_contract(graph: FeatureGraph, driven_as: GraphRun) -> Result<()> {
    if graph.run != driven_as {
        bail!(
            "mcp_tier_isolation: graph={} declares {:?} but is driven as {:?}. The declaration \
             and the runner must agree, or the graph's observation is meaningless.",
            graph.label,
            graph.run,
            driven_as
        );
    }
    match graph.run {
        GraphRun::Tests if graph.required_tests.is_empty() => bail!(
            "mcp_tier_isolation: graph={} is driven with `cargo test` but names no test to \
             observe. `cargo test` exits 0 for zero tests, so a graph observing nothing passes \
             silently — name the tests it must see, or declare it `GraphRun::CheckOnly`.",
            graph.label
        ),
        GraphRun::CheckOnly if !graph.required_tests.is_empty() => bail!(
            "mcp_tier_isolation: graph={} is `cargo check`-only but names {} test(s) to observe. \
             Nothing executes in a check graph, so that expectation could never be met.",
            graph.label,
            graph.required_tests.len()
        ),
        _ => Ok(()),
    }
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
    graph_observation_contract(graph, GraphRun::Tests)?;
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
    let output = cmd.output().with_context(|| {
        format!(
            "failed to spawn cargo test for {CORE_PACKAGE} graph={}",
            graph.label
        )
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stdout}");
    eprint!("{stderr}");
    if !output.status.success() {
        bail!(
            "mcp_tier_isolation: {CORE_PACKAGE} graph={}: the agent/operator tier boundary is \
             not holding — {} integration tests failed. A `trybuild` fixture that compiled \
             means the operator-tier surface it names became reachable from an agent-tier \
             build; check the `#[cfg(feature = \"operator-tier\")]` gates in \
             crates/gaze-mcp-core/src/tools/mod.rs and crates/gaze-mcp-core/src/lib.rs.",
            graph.label,
            CORE_TEST_TARGET
        );
    }
    if let Some(observed) = unobserved_required_tests(&stdout, graph.required_tests).first() {
        bail!(
            "mcp_tier_isolation: {CORE_PACKAGE} graph={}: expected to observe `{}` but the \
             test did not run. This gate is only worth its exit code while that test \
             executes: `cargo test` exits 0 for zero tests, so a deleted or \
             accidentally `#[cfg]`-ed-out tier test would otherwise pass silently.",
            graph.label,
            observed
        );
    }
    Ok(())
}

/// The `test <name> ... ok` lines a graph promised that its run did not
/// produce.
///
/// Matching on `... ok` rather than on the bare name is deliberate: a test that
/// ran and failed, and a target that ran no tests at all, must both count as
/// unobserved.
fn unobserved_required_tests(stdout: &str, required: &[&str]) -> Vec<String> {
    required
        .iter()
        .map(|name| format!("test {name} ... ok"))
        .filter(|observed| !stdout.contains(observed.as_str()))
        .collect()
}

fn run_rmcp_under(graph: FeatureGraph) -> Result<()> {
    graph_observation_contract(graph, GraphRun::CheckOnly)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_run_that_reports_every_required_test_passing() {
        let stdout = format!(
            "running 2 tests\ntest {CORE_TOOLS_PRESENT_TEST} ... ok\n\
             test {AGENT_TIER_UNREACHABLE_TEST} ... ok\n\ntest result: ok. 2 passed;\n"
        );
        assert!(unobserved_required_tests(
            &stdout,
            &[CORE_TOOLS_PRESENT_TEST, AGENT_TIER_UNREACHABLE_TEST]
        )
        .is_empty());
    }

    #[test]
    fn rejects_a_run_that_executed_no_tests() {
        // The exact failure this guard exists for: `cargo test` exits 0 for a
        // target with no tests, so a deleted or `#[cfg]`-ed-out tier test
        // would leave the gate green while checking nothing.
        let stdout = "\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed;\n";
        assert_eq!(
            unobserved_required_tests(stdout, &[AGENT_TIER_UNREACHABLE_TEST]),
            vec![format!("test {AGENT_TIER_UNREACHABLE_TEST} ... ok")]
        );
    }

    #[test]
    fn rejects_a_required_test_that_ran_but_failed() {
        let stdout = format!("running 1 test\ntest {AGENT_TIER_UNREACHABLE_TEST} ... FAILED\n");
        assert_eq!(
            unobserved_required_tests(&stdout, &[AGENT_TIER_UNREACHABLE_TEST]),
            vec![format!("test {AGENT_TIER_UNREACHABLE_TEST} ... ok")]
        );
    }

    #[test]
    fn names_only_the_tests_that_were_not_observed() {
        let stdout = format!("running 1 test\ntest {CORE_TOOLS_PRESENT_TEST} ... ok\n");
        assert_eq!(
            unobserved_required_tests(
                &stdout,
                &[CORE_TOOLS_PRESENT_TEST, AGENT_TIER_UNREACHABLE_TEST]
            ),
            vec![format!("test {AGENT_TIER_UNREACHABLE_TEST} ... ok")]
        );
    }

    #[test]
    fn every_feature_graph_declares_its_observation_explicitly() {
        // Both tables, not just the core one. A `cargo test` graph with an
        // empty list would spawn cargo and verify nothing, which is how this
        // gate became vacuous in the first place; a `cargo check` graph must
        // say so outright rather than express it by omission, so that "nothing
        // executes here" stays distinguishable from "nobody filled this in".
        for (table, graphs) in [("core", CORE_FEATURE_GRAPHS), ("rmcp", RMCP_FEATURE_GRAPHS)] {
            for graph in graphs {
                match graph.run {
                    GraphRun::Tests => assert!(
                        !graph.required_tests.is_empty(),
                        "{table} feature graph `{}` runs `cargo test` but declares no required test",
                        graph.label
                    ),
                    GraphRun::CheckOnly => assert!(
                        graph.required_tests.is_empty(),
                        "{table} feature graph `{}` is check-only but declares required tests",
                        graph.label
                    ),
                }
            }
        }
    }

    #[test]
    fn core_graphs_run_tests_and_rmcp_graphs_only_check() {
        // Pins each table to its runner, so a future `cargo test` graph cannot
        // be parked in the check-only table to dodge the non-empty rule.
        for graph in CORE_FEATURE_GRAPHS {
            assert_eq!(graph.run, GraphRun::Tests, "core graph `{}`", graph.label);
        }
        for graph in RMCP_FEATURE_GRAPHS {
            assert_eq!(
                graph.run,
                GraphRun::CheckOnly,
                "rmcp graph `{}`",
                graph.label
            );
        }
    }

    #[test]
    fn the_contract_rejects_a_test_graph_that_observes_nothing() {
        let vacuous = FeatureGraph {
            label: "vacuous",
            cargo_args: &[],
            run: GraphRun::Tests,
            required_tests: &[],
        };
        let error = graph_observation_contract(vacuous, GraphRun::Tests)
            .expect_err("a `cargo test` graph naming no test must be rejected");
        assert!(
            error.to_string().contains("names no test to observe"),
            "{error}"
        );
    }

    #[test]
    fn the_contract_rejects_a_check_graph_that_claims_to_observe_tests() {
        let confused = FeatureGraph {
            label: "confused",
            cargo_args: &[],
            run: GraphRun::CheckOnly,
            required_tests: &[AGENT_TIER_UNREACHABLE_TEST],
        };
        let error = graph_observation_contract(confused, GraphRun::CheckOnly)
            .expect_err("a check-only graph naming tests must be rejected");
        assert!(error.to_string().contains("could never be met"), "{error}");
    }

    #[test]
    fn the_contract_rejects_a_graph_driven_against_its_declaration() {
        let check_only = FeatureGraph {
            label: "check-only",
            cargo_args: &[],
            run: GraphRun::CheckOnly,
            required_tests: &[],
        };
        let error = graph_observation_contract(check_only, GraphRun::Tests)
            .expect_err("driving a check-only graph as a test graph must be rejected");
        assert!(error.to_string().contains("is driven as"), "{error}");
    }

    #[test]
    fn the_contract_accepts_every_shipped_graph() {
        for graph in CORE_FEATURE_GRAPHS {
            graph_observation_contract(*graph, GraphRun::Tests)
                .unwrap_or_else(|error| panic!("core graph `{}`: {error}", graph.label));
        }
        for graph in RMCP_FEATURE_GRAPHS {
            graph_observation_contract(*graph, GraphRun::CheckOnly)
                .unwrap_or_else(|error| panic!("rmcp graph `{}`: {error}", graph.label));
        }
    }

    #[test]
    fn the_agent_tier_graphs_require_the_compile_fail_test() {
        // `default` and `no-default-features` are the graphs where the
        // operator surface must be unreachable; both must prove it ran.
        for label in ["default", "no-default-features"] {
            let graph = CORE_FEATURE_GRAPHS
                .iter()
                .find(|graph| graph.label == label)
                .unwrap_or_else(|| panic!("core feature graph `{label}` is missing"));
            assert!(
                graph.required_tests.contains(&AGENT_TIER_UNREACHABLE_TEST),
                "agent-tier graph `{label}` must require `{AGENT_TIER_UNREACHABLE_TEST}`"
            );
        }
    }
}
