use std::env;
use std::ffi::OsString;
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

fn nested_target_dir(root: &Path) -> PathBuf {
    root.join("target").join("class-map-override-safety")
}

fn gate_command_with_cargo(root: &Path, inherited_cargo: Option<OsString>) -> Command {
    let cargo = inherited_cargo.unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(cargo);
    command
        .args(["run", "-p", "xtask", "--", "class-map-override-safety"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", nested_target_dir(root));
    command
}

fn gate_command(root: &Path) -> Command {
    gate_command_with_cargo(root, env::var_os("CARGO"))
}

fn run_gate(root: &Path) -> Output {
    gate_command(root)
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

fn command_target_dir(command: &Command) -> PathBuf {
    command
        .get_envs()
        .find_map(|(key, value)| {
            (key == "CARGO_TARGET_DIR").then(|| {
                PathBuf::from(value.expect("CARGO_TARGET_DIR must have an assigned value"))
            })
        })
        .expect("gate command must assign CARGO_TARGET_DIR")
}

#[test]
fn gate_command_uses_inherited_cargo_and_distinct_nested_targets() {
    let root_a = Path::new("fixture-source-root-a");
    let root_b = Path::new("fixture-source-root-b");
    let inherited_cargo = OsString::from("inherited-pinned-cargo");
    let command_a = gate_command_with_cargo(root_a, Some(inherited_cargo.clone()));
    let command_b = gate_command_with_cargo(root_b, Some(inherited_cargo.clone()));
    let target_a = command_target_dir(&command_a);
    let target_b = command_target_dir(&command_b);

    assert_eq!(command_a.get_program(), inherited_cargo);
    assert_eq!(command_b.get_program(), inherited_cargo);
    assert_eq!(command_a.get_current_dir(), Some(root_a));
    assert_eq!(command_b.get_current_dir(), Some(root_b));
    assert_eq!(target_a, nested_target_dir(root_a));
    assert_eq!(target_b, nested_target_dir(root_b));
    assert!(target_a.starts_with(root_a));
    assert!(target_b.starts_with(root_b));
    assert_ne!(target_a, target_b);

    let fallback = gate_command_with_cargo(root_a, None);
    assert_eq!(fallback.get_program(), "cargo");
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

    let assembly = fixture_root.join("crates/gaze-assembly/src/tests.rs");
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
