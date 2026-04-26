use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RENAMED_TEST: &str = "t20a_class_map_override_fails_closed_when_action_rule_uncovered";
const RENAMED_TEST_DISABLED: &str =
    "t20a_class_map_override_fails_closed_when_action_rule_uncovered_disabled";

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
        .args(["run", "-p", "xtask", "--", "class-map-override-safety"])
        .current_dir(root)
        .output()
        .expect("run class-map-override-safety gate")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn class_map_override_safety_gate_passes_on_baseline() {
    let output = run_gate(&workspace_root());
    assert!(
        output.status.success(),
        "gate must pass on baseline; {}",
        output_text(&output)
    );
}

#[test]
fn class_map_override_safety_gate_fails_when_required_test_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture_root = temp.path().join("gaze-class-map-override-adversarial");
    let add_status = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&fixture_root)
        .arg("HEAD")
        .current_dir(workspace_root())
        .status()
        .expect("create adversarial worktree");
    assert!(add_status.success(), "git worktree add must succeed");

    let assembly = fixture_root.join("crates/gaze-assembly/src/lib.rs");
    let source = fs::read_to_string(&assembly).expect("read gaze-assembly lib");
    assert!(
        source.contains(RENAMED_TEST),
        "fixture must contain required test before mutation"
    );
    fs::write(
        &assembly,
        source.replacen(RENAMED_TEST, RENAMED_TEST_DISABLED, 1),
    )
    .expect("rename required test in fixture");

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
        "gate must fail when a required test is missing; {text}"
    );
    assert!(
        text.contains("missing behavioral test"),
        "gate failure must identify the list-phase miss; {text}"
    );
    assert!(
        text.contains("tests::t20a_class_map_override_fails_closed_when_action_rule_uncovered"),
        "gate failure must name the missing behavioral test; {text}"
    );
}
