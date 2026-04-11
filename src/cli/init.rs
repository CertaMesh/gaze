//! `gaze init` — scaffold a new Gaze project in the current directory.
//!
//! Creates:
//!   ./policy.toml       (copy of the example)
//!   ./.gaze/            (audit log dir, gitignored)
//!   appends ".gaze/" to .gitignore if not already present

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("policy.toml already exists — refusing to overwrite")]
    PolicyExists,
}

pub const EXAMPLE_POLICY: &str = include_str!("../../policy.example.toml");

pub fn run(dir: &Path) -> Result<(), InitError> {
    let policy_path = dir.join("policy.toml");
    if policy_path.exists() {
        return Err(InitError::PolicyExists);
    }
    write_file(&policy_path, EXAMPLE_POLICY)?;

    let gaze_dir = dir.join(".gaze");
    fs::create_dir_all(&gaze_dir).map_err(|e| InitError::Io {
        path: gaze_dir.display().to_string(),
        source: e,
    })?;

    append_gitignore(dir)?;
    Ok(())
}

fn write_file(path: &PathBuf, contents: &str) -> Result<(), InitError> {
    fs::write(path, contents).map_err(|e| InitError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

fn append_gitignore(dir: &Path) -> Result<(), InitError> {
    let gi = dir.join(".gitignore");
    let mut contents = fs::read_to_string(&gi).unwrap_or_default();
    if contents.lines().any(|l| l.trim() == ".gaze/") {
        return Ok(());
    }
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(".gaze/\n");
    fs::write(&gi, contents).map_err(|e| InitError::Io {
        path: gi.display().to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_scaffolds_policy_and_gaze_dir() {
        let tmp = tempdir().unwrap();
        run(tmp.path()).unwrap();
        assert!(tmp.path().join("policy.toml").exists());
        assert!(tmp.path().join(".gaze").is_dir());
        let gi = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".gaze/"));
    }

    #[test]
    fn init_refuses_to_overwrite() {
        let tmp = tempdir().unwrap();
        run(tmp.path()).unwrap();
        let err = run(tmp.path()).unwrap_err();
        assert!(matches!(err, InitError::PolicyExists));
    }

    #[test]
    fn init_does_not_double_append_gitignore() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".gaze/\n").unwrap();
        run(tmp.path()).unwrap();
        let gi = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(gi.matches(".gaze/").count(), 1);
    }
}
