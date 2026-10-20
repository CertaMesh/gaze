use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
struct Workflow {
    jobs: BTreeMap<String, Job>,
}

#[derive(Deserialize)]
struct Job {
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Step {
    run: Option<String>,
}

fn check_matrix_ownership(workflow: &str) -> Result<(), String> {
    let workflow: Workflow = serde_yaml::from_str(workflow).map_err(|error| error.to_string())?;
    let mut commands = Vec::new();
    for (job, body) in workflow.jobs {
        for step in body.steps {
            if let Some(run) = step.run {
                for line in run.lines().map(str::trim) {
                    if line.starts_with("cargo run -p xtask -- ") {
                        commands.push((job.clone(), line.to_owned()));
                    }
                }
            }
        }
    }
    let expected = [
        (
            "workspace-default".to_owned(),
            "cargo run -p xtask -- ci-feature-matrix --partition ci-default".to_owned(),
        ),
        (
            "xtask-feature-gates".to_owned(),
            "cargo run -p xtask -- ci-feature-matrix --partition ci-gates".to_owned(),
        ),
    ];
    if commands == expected {
        Ok(())
    } else {
        Err(format!("standalone or missing xtask gates: {commands:?}"))
    }
}

#[test]
fn test_workflow_runs_xtask_gates_only_through_the_matrix() {
    check_matrix_ownership(include_str!("../../../.github/workflows/test.yml")).unwrap();
}

#[test]
fn matrix_ownership_checks_plain_and_block_run_strings() {
    for matrix in [
        "run: cargo run -p xtask -- ci-feature-matrix --partition ci-gates",
        "run: |\n          cargo run -p xtask -- ci-feature-matrix --partition ci-gates",
    ] {
        let workflow = format!(
            "jobs:\n  workspace-default:\n    steps:\n      - run: cargo run -p xtask -- ci-feature-matrix --partition ci-default\n  xtask-feature-gates:\n    steps:\n      - {matrix}\n"
        );
        check_matrix_ownership(&workflow).unwrap();
        for duplicate in [
            "run: cargo run -p xtask -- safety-net-sanity",
            "run: |\n          echo checking\n          cargo run -p xtask -- safety-net-sanity",
        ] {
            let changed = format!("{workflow}      - {duplicate}\n");
            assert!(check_matrix_ownership(&changed).is_err(), "{duplicate}");
        }
    }
}

fn job_section(workflow: &str, job: &str) -> String {
    let marker = format!("\n  {job}:\n");
    let body = workflow.split_once(&marker).expect("coverage job exists").1;
    body.split_inclusive('\n')
        .take_while(|line| {
            !line.starts_with("  ") || line.starts_with("   ") || line.trim().is_empty()
        })
        .collect()
}

#[test]
fn ci_workflow_keeps_all_coverage_owners_and_required_contexts() {
    let workflow = include_str!("../../../.github/workflows/test.yml");
    let test = job_section(workflow, "test-support");
    for command in [
        "cargo fmt --all -- --check",
        "cargo clippy --workspace --all-features --all-targets -- -D warnings",
        "cargo test -p gaze-document --features mcp --no-fail-fast",
        "cargo test -p gaze-cli --features mcp --no-fail-fast",
        "cargo test --workspace --all-features --doc --no-fail-fast",
    ] {
        assert!(
            test.lines()
                .any(|line| line.trim() == format!("run: {command}")),
            "test must own {command}"
        );
    }
    let required_test = job_section(workflow, "test");
    assert!(!required_test
        .lines()
        .any(|line| line.starts_with("    name:")));
    assert!(required_test.contains("    needs: [test-support, workspace-shards, test-coverage]\n"));
    assert!(required_test.contains("    if: ${{ always() }}\n"));
    for (variable, job) in [
        ("SUPPORT_RESULT", "test-support"),
        ("SHARDS_RESULT", "workspace-shards"),
        ("COVERAGE_RESULT", "test-coverage"),
    ] {
        assert!(required_test.contains(&format!("{variable}: ${{{{ needs.{job}.result }}}}")));
    }
    let shards = job_section(workflow, "workspace-shards");
    assert!(shards.contains("fail-fast: false"));
    assert!(shards.contains("shard: [cli, runtime, gates]"));
    assert!(shards.contains("python3 scripts/ci/test_shards.py collect"));
    assert!(
        shards.contains("cargo nextest run --workspace --all-features --profile ci --no-fail-fast")
    );
    for command in [
        "if: ${{ matrix.shard == 'gates' }}",
        "python3 scripts/ci/test_shards.py covered-package --evidence target/shard-evidence/gates --package gaze-proxy-dashboard",
        "python3 scripts/ci/test_shards.py covered-package --evidence target/shard-evidence/gates --package gaze-inspection",
    ] {
        assert!(shards.contains(command), "shards must retain {command}");
    }
    let proof = job_section(workflow, "test-coverage");
    assert!(proof.contains("needs: [workspace-shards]"));
    assert!(proof.contains("python3 scripts/ci/test_shards.py prove"));
    let defaults = job_section(workflow, "workspace-default");
    assert!(
        defaults.contains("run: cargo run -p xtask -- ci-feature-matrix --partition ci-default")
    );
    assert!(!defaults.contains("    needs:"));
    assert!(!job_section(workflow, "xtask-feature-gates").contains("    needs:"));
    let aggregate = job_section(workflow, "xtask-gates");
    assert!(aggregate.contains("    name: xtask gates\n"));
    assert!(aggregate.contains("    needs: [test, xtask-feature-gates, workspace-default]\n"));
    assert!(aggregate.contains("    if: ${{ always() }}\n"));
    for (variable, job) in [
        ("TEST_RESULT", "test"),
        ("FEATURE_GATES_RESULT", "xtask-feature-gates"),
        ("DEFAULT_RESULT", "workspace-default"),
    ] {
        assert!(aggregate.contains(&format!("{variable}: ${{{{ needs.{job}.result }}}}")));
    }
    assert!(!workflow.contains("continue-on-error"));
}

#[test]
fn required_aggregator_rejects_failure_skip_cancel_and_missing_results() {
    let workflow = include_str!("../../../.github/workflows/test.yml");
    for (job, variables) in [
        (
            "xtask-gates",
            ["TEST_RESULT", "FEATURE_GATES_RESULT", "DEFAULT_RESULT"],
        ),
        (
            "test",
            ["SUPPORT_RESULT", "SHARDS_RESULT", "COVERAGE_RESULT"],
        ),
    ] {
        let aggregate = job_section(workflow, job);
        let script: String = aggregate
            .split_once("        run: |\n")
            .expect("aggregator has a shell check")
            .1
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| format!("{}\n", line.strip_prefix("          ").unwrap()))
            .collect();
        for test in ["success", "failure", "skipped", "cancelled", ""] {
            for gates in ["success", "failure", "skipped", "cancelled", ""] {
                for defaults in ["success", "failure", "skipped", "cancelled", ""] {
                    let output = std::process::Command::new("bash")
                        .args(["-e", "-c", &script])
                        .env(variables[0], test)
                        .env(variables[1], gates)
                        .env(variables[2], defaults)
                        .output()
                        .unwrap();
                    assert_eq!(
                        output.status.success(),
                        [test, gates, defaults]
                            .iter()
                            .all(|result| *result == "success"),
                        "aggregator result for {test:?}/{gates:?}/{defaults:?}"
                    );
                }
            }
        }
    }
}
