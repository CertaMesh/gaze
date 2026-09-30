use std::path::PathBuf;

const REQUIRED_XTASK_GATES: &[&str] = &[
    "symmetric-potemkin",
    "class-map-override-safety",
    "recognizer-composition-validator",
    "no-tenant-knowledge",
    "bundle-tokenization-drift --verify-ack",
    "family-policy-table-coherence",
    "locale-cue-bundle-coherence",
    "fixture-citation-lint",
    "trybuild-fixture-hygiene",
    "cargo-metadata-audit-isolation",
    "readme-version-check",
    "safety-net-sanity",
    "dashboard-isolation",
    "mcp-tier-isolation",
    "ci-feature-matrix --partition ci-gates",
];

#[test]
fn test_workflow_runs_every_required_xtask_gate() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap();
    let workflow = std::fs::read_to_string(repo_root.join(".github/workflows/test.yml")).unwrap();
    let gates = job_section(&workflow, "xtask-feature-gates");

    for gate in REQUIRED_XTASK_GATES {
        let command = format!("run: cargo run -p xtask -- {gate}");
        assert!(
            gates.lines().any(|line| line.trim() == command),
            "test.yml must run `{command}`"
        );
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
    let test = job_section(workflow, "test");
    for command in [
        "cargo fmt --all -- --check",
        "cargo clippy --workspace --all-features --all-targets -- -D warnings",
        "cargo test -p gaze-document --features mcp --no-fail-fast",
        "cargo test -p gaze-cli --features mcp --no-fail-fast",
        "cargo test --workspace --all-features --no-fail-fast",
    ] {
        assert!(
            test.lines()
                .any(|line| line.trim() == format!("run: {command}")),
            "test must own {command}"
        );
    }
    // The job id remains the required context; no display-name override.
    assert!(!test.lines().any(|line| line.starts_with("    name:")));
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
    let aggregate = job_section(workflow, "xtask-gates");
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
                    .env("TEST_RESULT", test)
                    .env("FEATURE_GATES_RESULT", gates)
                    .env("DEFAULT_RESULT", defaults)
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
