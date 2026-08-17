use std::path::PathBuf;

use serde::Deserialize;

#[path = "../../../lint/dylint/src/default_config.rs"]
mod default_config;

#[derive(Debug, Deserialize)]
struct RootConfig {
    gaze_dylint: DylintConfig,
}

#[derive(Debug, Deserialize)]
struct DylintConfig {
    protected_paths: Vec<String>,
    forbidden_crates: Vec<String>,
    forbidden_items: Vec<String>,
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn compiled_default_matches_repository_dylint_config() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap();
    let source = std::fs::read_to_string(repo_root.join("dylint.toml")).unwrap();
    let parsed: RootConfig = toml::from_str(&source).unwrap();

    assert_eq!(
        parsed.gaze_dylint.protected_paths,
        owned(default_config::PROTECTED_PATHS)
    );
    assert_eq!(
        parsed.gaze_dylint.forbidden_crates,
        owned(default_config::FORBIDDEN_CRATES)
    );
    assert_eq!(
        parsed.gaze_dylint.forbidden_items,
        owned(default_config::FORBIDDEN_ITEMS)
    );
}
