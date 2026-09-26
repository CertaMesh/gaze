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
    run_gate_on(
        root,
        &format!("crates/xtask/fixtures/scrub_public_text/{fixture}"),
    )
}

fn run_gate_on(root: &Path, path: &str) -> Output {
    Command::new("cargo")
        .args(["run", "-p", "xtask", "--", "scrub-public-text", path])
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

#[test]
fn scrub_public_text_passes_allowlisted_public_urls() {
    let output = run_gate(&workspace_root(), "allowed_urls.md");
    assert!(
        output.status.success(),
        "project-repo and semver.org links must pass; {}",
        output_text(&output)
    );
}

#[test]
fn scrub_public_text_fails_lookalike_repo_url() {
    let output = run_gate(&workspace_root(), "lookalike_url.md");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "a lookalike repo URL must fail; {text}"
    );
    assert!(text.contains("gaze clean emitted"), "{text}");
}

#[test]
fn scrub_public_text_fails_lookalike_semver_host() {
    let output = run_gate(&workspace_root(), "other_url.md");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "a semver.org lookalike host must fail; {text}"
    );
    assert!(text.contains("gaze clean emitted"), "{text}");
}

/// UPGRADE.md is public release text, so every PR keeps it scrub-clean, not only a release run.
#[test]
fn scrub_public_text_passes_upgrade_md() {
    let output = run_gate_on(&workspace_root(), "UPGRADE.md");
    assert!(
        output.status.success(),
        "UPGRADE.md must pass the public-text scrub; {}",
        output_text(&output)
    );
}

#[test]
fn scrub_public_text_fails_suffix_lookalike_host() {
    let output = run_gate(&workspace_root(), "suffix_host_url.md");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "a host that only ends in semver.org must fail; {text}"
    );
    assert!(text.contains("gaze clean emitted"), "{text}");
}

#[test]
fn scrub_public_text_fails_free_text_path_on_allowlisted_host() {
    let output = run_gate(&workspace_root(), "allowlisted_host_free_path.md");
    let text = output_text(&output);
    assert!(
        !output.status.success(),
        "an allowlisted host must not carry free text past the scrub; {text}"
    );
    assert!(text.contains("gaze clean emitted"), "{text}");
}
