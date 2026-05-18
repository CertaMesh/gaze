#![cfg(feature = "document")]

use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

fn fixture_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates dir")
        .join("gaze-document")
        .join("testdata")
        .join("synthetic_image.png")
}

fn run_document_clean(args: &[String]) -> Option<Value> {
    let output = Command::cargo_bin("gaze")
        .expect("gaze binary")
        .arg("document")
        .arg("clean")
        .args(args)
        .output()
        .expect("run gaze document clean");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("tesseract binary not found") {
            eprintln!("SKIP: tesseract not installed: {stderr}");
            return None;
        }
        panic!(
            "gaze document clean failed: status={:?} stderr={stderr}",
            output.status
        );
    }

    Some(serde_json::from_slice(&output.stdout).expect("stdout is JSON"))
}

#[test]
fn cli_document_clean_split_out_writes_files_to_correct_dirs() {
    let tmp = tempdir().expect("tempdir");
    let input = fixture_path();
    let agent = tmp.path().join("agent-bundle");
    let owner = tmp.path().join("owner-vault");
    let args = vec![
        input.display().to_string(),
        "--agent-out".to_string(),
        agent.display().to_string(),
        "--owner-out".to_string(),
        owner.display().to_string(),
    ];

    let Some(summary) = run_document_clean(&args) else {
        return;
    };

    assert_eq!(summary["ok"], true);
    assert!(agent.join("clean.md").exists());
    assert!(agent.join("report.json").exists());
    assert!(!agent.join("manifest.json").exists());
    assert!(owner.join("manifest.json").exists());
    assert_eq!(dir_entry_names(&owner), vec!["manifest.json".to_string()]);
}

#[test]
fn cli_document_clean_out_shorthand_creates_subdirs() {
    let tmp = tempdir().expect("tempdir");
    let input = fixture_path();
    let out = tmp.path().join("safe-bundle");
    let args = vec![
        input.display().to_string(),
        "--out".to_string(),
        out.display().to_string(),
    ];

    let Some(summary) = run_document_clean(&args) else {
        return;
    };

    assert_eq!(summary["ok"], true);
    assert!(out.join("agent").join("clean.md").exists());
    assert!(out.join("agent").join("report.json").exists());
    assert!(!out.join("agent").join("manifest.json").exists());
    assert!(out.join("owner").join("manifest.json").exists());
    assert_eq!(
        dir_entry_names(&out.join("owner")),
        vec!["manifest.json".to_string()]
    );
}

fn dir_entry_names(path: &std::path::Path) -> Vec<String> {
    let mut names = std::fs::read_dir(path)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}
