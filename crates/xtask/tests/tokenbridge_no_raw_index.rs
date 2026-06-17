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

fn run_gate(root: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new("cargo");
    command.args(["run", "-p", "xtask", "--", "tokenbridge-no-raw-index"]);
    command.args(extra_args);
    command
        .current_dir(root)
        .output()
        .expect("run tokenbridge-no-raw-index gate")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn tokenbridge_no_raw_index_gate_passes_on_real_ingest_path() {
    let output = run_gate(&workspace_root(), &[]);
    assert!(
        output.status.success(),
        "gate must pass on baseline real ingest path; {}",
        output_text(&output)
    );
}

#[test]
fn tokenbridge_no_raw_index_gate_fails_when_raw_value_would_be_stored() {
    let output = run_gate(&workspace_root(), &["--inject-adversarial-raw-snippet"]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "gate must fail when a raw value reaches an indexed snippet; {text}"
    );
    assert!(
        text.contains("raw PII leaked into"),
        "gate failure must name the leak class; {text}"
    );
    assert!(
        text.contains("adapter search hit") || text.contains("stored index hit"),
        "gate failure must identify the indexed/search output surface; {text}"
    );
    assert!(
        text.contains("alice@example.invalid"),
        "gate failure must name the synthetic fixture value; {text}"
    );
}
