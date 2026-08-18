// drift-ack: v0.8 unifies core/core-extended snapshots under the single core bundle.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn run_gate(root: &Path) -> Output {
    Command::new("cargo")
        .args(["run", "-p", "xtask", "--", "bundle-tokenization-drift"])
        .current_dir(root)
        .output()
        .expect("run bundle-tokenization-drift gate")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn bundle_tokenization_drift_gate_passes_on_baseline() {
    let output = run_gate(&workspace_root());
    assert!(
        output.status.success(),
        "gate must pass on baseline; {}",
        output_text(&output)
    );
}

#[test]
fn bundle_tokenization_drift_gate_targets_repo_snapshot_from_nested_cwd() {
    let root = workspace_root();
    let nested = root.join("crates/xtask/fixtures");
    let output = Command::new("cargo")
        .args([
            "run",
            "-p",
            "xtask",
            "--",
            "bundle-tokenization-drift",
            "--verify-ack",
        ])
        .current_dir(&nested)
        .output()
        .expect("run bundle-tokenization-drift gate from nested cwd");

    assert!(
        output.status.success(),
        "nested-cwd gate must resolve the repo-root snapshot; {}",
        output_text(&output)
    );
    assert!(
        output_text(&output).contains("bundle-tokenization-drift: passed"),
        "nested-cwd gate must execute against the committed snapshot; {}",
        output_text(&output)
    );
}

#[test]
fn bundle_tokenization_drift_gate_fails_when_enabled_recognizer_id_drifts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture_root = temp
        .path()
        .join("gaze-bundle-tokenization-drift-adversarial");
    let add_status = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&fixture_root)
        .arg("HEAD")
        .current_dir(workspace_root())
        .status()
        .expect("create adversarial worktree");
    assert!(add_status.success(), "git worktree add must succeed");

    let rulepack = fixture_root.join("crates/gaze-recognizers/embedded/core.toml");
    let source = fs::read_to_string(&rulepack).expect("read core rulepack");
    assert!(
        source.contains("id = \"ip.v4\""),
        "fixture must contain expected recognizer id before mutation"
    );
    fs::write(
        &rulepack,
        source.replacen("id = \"ip.v4\"", "id = \"ip.v4.drift\"", 1),
    )
    .expect("rename recognizer id in fixture");

    let output = run_gate(&fixture_root);
    let remove_status = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&fixture_root)
        .current_dir(workspace_root())
        .status()
        .expect("remove adversarial worktree");
    assert!(remove_status.success(), "git worktree remove must succeed");

    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "gate must fail when recognizer id drifts; {text}"
    );
    assert!(
        text.contains("snapshot drift detected"),
        "gate failure must identify snapshot drift; {text}"
    );
    assert!(
        text.contains("bundle `core`"),
        "gate failure must name the changed bundle; {text}"
    );
    assert!(
        text.contains("recognizer=ip.v4") && text.contains("recognizer=ip.v4.drift"),
        "gate failure must name old and new recognizer ids; {text}"
    );
    assert!(
        text.contains("class=custom:ip_address"),
        "gate failure must name the changed class; {text}"
    );
}
