use std::path::PathBuf;
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn run_gate() -> Output {
    Command::new("cargo")
        .args(["run", "-p", "xtask", "--", "tokenbridge-encrypted-index"])
        .current_dir(workspace_root())
        .output()
        .expect("run tokenbridge-encrypted-index gate")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn tokenbridge_encrypted_index_gate_runs_behavioral_test() {
    let output = run_gate();
    assert!(
        output.status.success(),
        "gate must pass when sealed index bytes omit plaintext; {}",
        output_text(&output)
    );
}
