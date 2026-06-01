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

fn run_gate(root: &Path, fixture: &str) -> Output {
    Command::new("cargo")
        .args(["run", "-p", "xtask", "--", "scrub-public-text"])
        .arg(format!("crates/xtask/fixtures/scrub_public_text/{fixture}"))
        .current_dir(root)
        .output()
        .expect("run scrub-public-text gate")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn scrub_public_text_passes_clean_fixture() {
    let output = run_gate(&workspace_root(), "clean.md");
    assert!(
        output.status.success(),
        "clean public text must pass; {}",
        output_text(&output)
    );
}

#[test]
fn scrub_public_text_fails_token_and_user_path_fixture() {
    let output = run_gate(&workspace_root(), "dirty.md");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "dirty public text must fail; {text}"
    );
    assert!(
        text.contains("gaze clean emitted Email token"),
        "gate must report tokens emitted by gaze clean; {text}"
    );
    assert!(
        text.contains("existing gaze token <Email_1>"),
        "gate must report already-tokenized public text; {text}"
    );
    assert!(
        text.contains("OS user path `/home/<name>/`"),
        "gate must report OS user path patterns without echoing the username; {text}"
    );
}
