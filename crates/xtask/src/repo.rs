use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::publish_plan::{self, WorkspaceMember};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrateExemption {
    pub name: &'static str,
    pub reason: &'static str,
}

pub const EXEMPT_CRATES: &[CrateExemption] = &[
    CrateExemption {
        name: "xtask",
        reason: "repository gate runner; its denylist and synthetic-fixture definitions are not production runtime code",
    },
    CrateExemption {
        name: "gaze_dylint",
        reason: "detached compile-time Dylint crate; it is not linked into any production runtime",
    },
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

pub fn production_source_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    production_source_dirs_from_members(root, &publish_plan::workspace_members(root)?)
}

fn production_source_dirs_from_members(
    root: &Path,
    members: &[WorkspaceMember],
) -> Result<Vec<PathBuf>> {
    let mut source_dirs = Vec::new();
    for member in members {
        if EXEMPT_CRATES
            .iter()
            .any(|exemption| exemption.name == member.name)
        {
            continue;
        }
        if !member.manifest_dir.starts_with(root) {
            anyhow::bail!(
                "workspace member {} resolves outside repo root: {}",
                member.name,
                member.manifest_dir.display()
            );
        }
        source_dirs.push(member.manifest_dir.join("src"));
    }
    source_dirs.sort();
    Ok(source_dirs)
}

pub fn source_files(
    source_dirs: &[PathBuf],
    include_file: impl Fn(&Path) -> bool + Copy,
) -> std::result::Result<Vec<PathBuf>, WalkError> {
    let mut files = Vec::new();
    for src in source_dirs {
        collect_files(src, &mut files, include_file)?;
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
    use super::{
        fixture_citation_file, production_source_dirs_from_members, repo_root_from_manifest_dir,
        source_files, tenant_knowledge_file,
    };
    use crate::publish_plan::WorkspaceMember;
    use std::{fs, path::Path};

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

    #[test]
    fn new_workspace_member_is_scanned_unless_explicitly_exempted() {
        let temp = tempfile::tempdir().unwrap();
        let new_member_dir = temp.path().join("crates/gaze-new-runtime");
        let exempt_dir = temp.path().join("crates/xtask");
        fs::create_dir_all(new_member_dir.join("src")).unwrap();
        fs::create_dir_all(exempt_dir.join("src")).unwrap();
        fs::write(new_member_dir.join("src/lib.rs"), "pub fn runtime() {}\n").unwrap();
        fs::write(exempt_dir.join("src/main.rs"), "fn main() {}\n").unwrap();

        let source_dirs = production_source_dirs_from_members(
            temp.path(),
            &[
                WorkspaceMember {
                    name: "gaze-new-runtime".to_string(),
                    manifest_dir: new_member_dir.clone(),
                },
                WorkspaceMember {
                    name: "xtask".to_string(),
                    manifest_dir: exempt_dir,
                },
            ],
        )
        .unwrap();
        let files = source_files(&source_dirs, tenant_knowledge_file).unwrap();

        assert_eq!(files, vec![new_member_dir.join("src/lib.rs")]);
    }
}
