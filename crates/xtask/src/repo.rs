use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};

pub const PRODUCTION_CRATES: &[&str] = &[
    "gaze-pii",
    "gaze-types",
    "gaze-recognizers",
    "gaze-assembly",
    "gaze-cli",
];

#[derive(Debug)]
pub struct WalkError {
    pub path: PathBuf,
    pub message: String,
}

pub fn repo_root() -> Result<PathBuf> {
    repo_root_from_manifest_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn repo_root_from_manifest_dir(manifest_dir: &Path) -> Result<PathBuf> {
    Ok(manifest_dir
        .parent()
        .and_then(Path::parent)
        .context("resolve workspace root from xtask manifest dir")?
        .to_path_buf())
}

pub fn production_files(
    root: &Path,
    crates: &[&str],
    include_file: impl Fn(&Path) -> bool + Copy,
) -> std::result::Result<Vec<PathBuf>, WalkError> {
    let mut files = Vec::new();
    for crate_name in crates {
        let src = root
            .join("crates")
            .join(package_source_dir(crate_name))
            .join("src");
        if src.exists() {
            collect_files(&src, &mut files, include_file)?;
        }
    }
    files.sort();
    Ok(files)
}

pub fn fixture_citation_file(path: &Path) -> bool {
    is_rust_file(path)
        && !matches!(
            path.file_stem().and_then(|stem| stem.to_str()),
            Some("tests" | "test_support")
        )
}

pub fn tenant_knowledge_file(path: &Path) -> bool {
    is_rust_file(path)
}

fn package_source_dir(package_name: &str) -> &str {
    match package_name {
        "gaze-pii" => "gaze",
        _ => package_name,
    }
}

fn collect_files(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    include_file: impl Fn(&Path) -> bool + Copy,
) -> std::result::Result<(), WalkError> {
    if dir
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(WalkError {
            path: dir.to_path_buf(),
            message: format!("Invalid input: {}", dir.display()),
        });
    }

    for entry in fs::read_dir(dir).map_err(|error| walk_error(dir, error))? {
        let entry = entry.map_err(|error| walk_error(dir, error))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files, include_file)?;
        } else if include_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_rust_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "rs")
}

fn walk_error(path: &Path, error: io::Error) -> WalkError {
    WalkError {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{fixture_citation_file, repo_root_from_manifest_dir, tenant_knowledge_file};
    use std::path::Path;

    #[test]
    fn repo_root_resolves_from_nested_xtask_manifest_dir() {
        assert_eq!(
            repo_root_from_manifest_dir(Path::new("/workspace/crates/xtask")).unwrap(),
            Path::new("/workspace")
        );
    }

    #[test]
    fn leak_guard_filters_preserve_their_exact_divergence() {
        let cases = [
            ("lib.rs", true, true),
            ("tests.rs", false, true),
            ("test_support.rs", false, true),
            ("fixture.toml", false, false),
        ];

        for (path, fixture_citation, tenant_knowledge) in cases {
            assert_eq!(fixture_citation_file(Path::new(path)), fixture_citation);
            assert_eq!(tenant_knowledge_file(Path::new(path)), tenant_knowledge);
        }
    }
}
