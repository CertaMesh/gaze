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
    "ci-feature-matrix",
];

#[test]
fn test_workflow_runs_every_required_xtask_gate() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap();
    let workflow = std::fs::read_to_string(repo_root.join(".github/workflows/test.yml")).unwrap();

    for gate in REQUIRED_XTASK_GATES {
        let command = format!("run: cargo run -p xtask -- {gate}");
        assert!(
            workflow.lines().any(|line| line.trim() == command),
            "test.yml must run `{command}`"
        );
    }
}
