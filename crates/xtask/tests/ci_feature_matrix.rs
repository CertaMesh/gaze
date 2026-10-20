#[test]
fn test_workflow_runs_xtask_gates_only_through_the_matrix() {
    let workflow = include_str!("../../../.github/workflows/test.yml");
    let gates = job_section(workflow, "xtask-feature-gates");
    let command = "run: cargo run -p xtask -- ci-feature-matrix --partition ci-gates";
    assert_eq!(
        gates.lines().filter(|line| line.trim() == command).count(),
        1,
        "the feature-gates job must run the matrix exactly once"
    );
    let direct_xtask_commands: Vec<_> = workflow
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("run: cargo run -p xtask -- "))
        .collect();
    assert_eq!(
        direct_xtask_commands,
        [
            command,
            "run: cargo run -p xtask -- ci-feature-matrix --partition ci-default",
        ],
        "standalone xtask gates duplicate commands owned by the matrix"
    );
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
    assert!(required_test
        .contains("    needs: [plan, fast, test-support, workspace-shards, test-coverage]\n"));
    assert!(required_test.contains("    if: ${{ always() }}\n"));
    assert!(required_test.contains("python3 scripts/ci/ci_gate.py check --context test"));
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
    assert!(proof.contains("needs: [plan, workspace-shards]"));
    assert!(proof.contains("python3 scripts/ci/test_shards.py prove"));
    let defaults = job_section(workflow, "workspace-default");
    assert!(
        defaults.contains("run: cargo run -p xtask -- ci-feature-matrix --partition ci-default")
    );
    assert!(defaults.contains("    needs: [plan]"));
    assert!(job_section(workflow, "xtask-feature-gates").contains("    needs: [plan]"));
    let aggregate = job_section(workflow, "xtask-gates");
    assert!(aggregate.contains("    name: xtask gates\n"));
    assert!(aggregate
        .contains("    needs: [plan, fast, test, xtask-feature-gates, workspace-default]\n"));
    assert!(aggregate.contains("    if: ${{ always() }}\n"));
    assert!(aggregate.contains("python3 scripts/ci/ci_gate.py check --context xtask"));
    assert!(!workflow.contains("continue-on-error"));
}

#[test]
fn integration_context_depends_on_every_full_suite_owner() {
    let workflow = include_str!("../../../.github/workflows/test.yml");
    let aggregate = job_section(workflow, "integration");
    assert!(aggregate.contains("    name: ci/integration\n"));
    assert!(aggregate.contains("    if: ${{ always() }}\n"));
    assert!(aggregate.contains("    needs: [plan, fast, msrv, test-support, workspace-shards, test-coverage, test, benchmark-model-free, xtask-feature-gates, workspace-default, xtask-gates, dylint]\n"));
    assert!(aggregate.contains("NEEDS_JSON: ${{ toJSON(needs) }}"));
    assert!(aggregate.contains("python3 scripts/ci/ci_gate.py check --context integration"));
    assert!(workflow.contains("  merge_group:\n    types: [checks_requested]"));
    assert!(workflow.contains("  workflow_dispatch:"));
    for job in [
        "test-support",
        "workspace-shards",
        "xtask-feature-gates",
        "workspace-default",
    ] {
        assert!(job_section(workflow, job)
            .contains("if: ${{ needs.plan.outputs.integration == 'true' }}"));
    }
    for (job, flag) in [
        ("msrv", "msrv"),
        ("benchmark-model-free", "bench"),
        ("dylint", "dylint"),
    ] {
        assert!(job_section(workflow, job).contains(&format!(
            "if: ${{{{ needs.plan.outputs.{flag} == 'true' }}}}"
        )));
    }
    let dylint = include_str!("../../../.github/workflows/dylint.yml");
    assert!(dylint.contains("  workflow_call:"));
    assert!(dylint.contains("GAZE_DYLINT_REQUIRED: \"1\""));
}
