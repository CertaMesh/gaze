use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use regex::Regex;

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
    run_gate_on_files(root, &[PathBuf::from(path)])
}

fn run_gate_on_files(root: &Path, paths: &[PathBuf]) -> Output {
    Command::new("cargo")
        .args(["run", "-p", "xtask", "--", "scrub-public-text"])
        .args(paths)
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

/// These files ship through GitHub Releases or crates.io, or are linked from the changelog.
/// The workspace test job runs this gate on every PR, before a release tag exists.
#[test]
fn scrub_public_text_passes_published_and_linked_docs() {
    let root = workspace_root();
    let mut paths = BTreeSet::from([
        PathBuf::from("CHANGELOG.md"),
        PathBuf::from("UPGRADE.md"),
        PathBuf::from("README.md"),
    ]);

    // Cargo accepts an explicit readme or the crate's conventional README.md.
    let readme = Regex::new(r#"(?m)^readme\s*=\s*"([^"]+)""#).expect("readme pattern");
    for entry in fs::read_dir(root.join("crates")).expect("list crates") {
        let crate_dir = entry.expect("crate entry").path();
        let manifest = crate_dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let manifest_text = fs::read_to_string(&manifest).expect("read crate manifest");
        let crate_readme = if let Some(captures) = readme.captures(&manifest_text) {
            crate_dir.join(&captures[1])
        } else {
            crate_dir.join("README.md")
        };
        if crate_readme.is_file() {
            paths.insert(
                crate_readme
                    .strip_prefix(&root)
                    .expect("crate inside workspace")
                    .to_path_buf(),
            );
        }
    }

    // Current local docs cited by the changelog remain part of release-facing text.
    let changelog = fs::read_to_string(root.join("CHANGELOG.md")).expect("read changelog");
    let cited_doc = Regex::new(r"docs/[A-Za-z0-9_./-]+\.md").expect("doc link pattern");
    for hit in cited_doc.find_iter(&changelog) {
        let path = PathBuf::from(hit.as_str());
        if root.join(&path).is_file() {
            paths.insert(path);
        }
    }

    let paths: Vec<_> = paths.into_iter().collect();
    let output = run_gate_on_files(&root, &paths);
    assert!(
        output.status.success(),
        "published and linked Markdown must pass the public-text scrub; {}",
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
