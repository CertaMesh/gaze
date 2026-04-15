use std::fs;
use std::path::Path;

use crate::policy::{build_pipeline, PolicyError, PolicyFile};

pub fn run(policy_path: &Path) -> Result<String, CheckError> {
    let text = fs::read_to_string(policy_path).map_err(|source| CheckError::Io {
        path: policy_path.display().to_string(),
        source,
    })?;
    let policy = PolicyFile::from_toml(&text)?;
    let _pipeline = build_pipeline(&policy)?;

    Ok(format!(
        "OK — policy at {path}\n  locale: {locale}\n  column_rules: {column_rules}\n  log_strip_patterns: {log_patterns}",
        path = policy_path.display(),
        locale = policy.ner.locale.as_deref().unwrap_or("unset"),
        column_rules = policy.policy.database.column_rules.len(),
        log_patterns = policy
            .policy
            .logs
            .as_ref()
            .map(|logs| logs.strip_patterns.len())
            .unwrap_or(0),
    ))
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Policy(#[from] PolicyError),
}
