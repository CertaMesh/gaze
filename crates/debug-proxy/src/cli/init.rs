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
    #[error("policy.toml already exists")]
    PolicyExists,
}

const EXAMPLE_POLICY: &str = r#"[ner]
locale = "de"

[connection.production]
kind = "mysql"
ssh_host = "deploy@prod.example.com"
local_port = 13306
remote_host = "127.0.0.1"
remote_port = 3306
database = "app"
user = "gaze_ro"
password_env = "GAZE_DB_PASSWORD"

[policy.database]

[[policy.database.columns]]
column = "email"
class = "email"
action = "tokenize"

[policy.logs]
path = "/var/log/app/laravel.log"
strip_patterns = ["(?i)password[=:][^ ]+"]
"#;

pub fn run(dir: &Path) -> Result<(), InitError> {
    let policy_path = dir.join("policy.toml");
    if policy_path.exists() {
        return Err(InitError::PolicyExists);
    }

    write_file(&policy_path, EXAMPLE_POLICY)?;

    let gaze_dir = dir.join(".gaze");
    fs::create_dir_all(&gaze_dir).map_err(|source| InitError::Io {
        path: gaze_dir.display().to_string(),
        source,
    })?;

    append_gitignore(dir)?;
    Ok(())
}

fn write_file(path: &PathBuf, contents: &str) -> Result<(), InitError> {
    fs::write(path, contents).map_err(|source| InitError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn append_gitignore(dir: &Path) -> Result<(), InitError> {
    let path = dir.join(".gitignore");
    let mut contents = fs::read_to_string(&path).unwrap_or_default();
    if contents.lines().any(|line| line.trim() == ".gaze/") {
        return Ok(());
    }
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(".gaze/\n");
    fs::write(&path, contents).map_err(|source| InitError::Io {
        path: path.display().to_string(),
        source,
    })
}
