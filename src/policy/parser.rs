//! TOML policy parser. Reject anything with more than one connection block.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::policy::classifier::{Classifier, PiiClass};

#[derive(Debug, Clone, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub connection: HashMap<String, ConnectionConfig>,
    pub policy: PolicySection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionConfig {
    pub kind: String,
    pub ssh_host: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub database: String,
    pub user: String,
    pub password_env: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicySection {
    pub database: DatabasePolicy,
    #[serde(default)]
    pub logs: Option<LogsPolicy>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabasePolicy {
    #[serde(default)]
    pub allowed_tables: Vec<String>,
    #[serde(default)]
    pub blocked_columns: Vec<String>,
    #[serde(default = "default_max_rows")]
    pub max_rows: usize,
    #[serde(default = "default_max_distinct")]
    pub max_distinct: usize,
    #[serde(default)]
    pub count_allowed_columns: Vec<String>,
    #[serde(default)]
    pub distinct_allowed_columns: Vec<String>,
    #[serde(default)]
    pub allowed_operations: Vec<String>,
    #[serde(default, rename = "columns")]
    pub column_rules: Vec<ColumnRule>,
}

fn default_max_rows() -> usize {
    50
}
fn default_max_distinct() -> usize {
    50
}

#[derive(Debug, Clone, Deserialize)]
pub struct ColumnRule {
    pub table: String,
    pub column: String,
    pub class: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogsPolicy {
    pub path: String,
    #[serde(default)]
    pub strip_patterns: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("policy must contain exactly one [connection.production] block; found {found}")]
    ConnectionCount { found: usize },
    #[error("only [connection.production] is supported in v0.1 (found `{name}`)")]
    NonProductionConnection { name: String },
    #[error("unknown PII class `{class}` for column `{table}.{column}`")]
    UnknownPiiClass {
        table: String,
        column: String,
        class: String,
    },
}

pub fn load_from_file(path: &Path) -> Result<Policy, PolicyError> {
    let text = std::fs::read_to_string(path).map_err(|e| PolicyError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let policy: Policy = toml::from_str(&text)?;
    validate(&policy)?;
    Ok(policy)
}

pub fn validate(policy: &Policy) -> Result<(), PolicyError> {
    if policy.connection.len() != 1 {
        return Err(PolicyError::ConnectionCount {
            found: policy.connection.len(),
        });
    }
    let (name, _) = policy.connection.iter().next().expect("checked len == 1");
    if name != "production" {
        return Err(PolicyError::NonProductionConnection { name: name.clone() });
    }
    for rule in &policy.policy.database.column_rules {
        parse_class(&rule.class).ok_or_else(|| PolicyError::UnknownPiiClass {
            table: rule.table.clone(),
            column: rule.column.clone(),
            class: rule.class.clone(),
        })?;
    }
    Ok(())
}

pub fn parse_class(s: &str) -> Option<PiiClass> {
    Some(match s {
        "id" => PiiClass::Id,
        "email" => PiiClass::Email,
        "name" => PiiClass::Name,
        "phone" => PiiClass::Phone,
        "address" => PiiClass::Address,
        "iban" => PiiClass::Iban,
        "ip" => PiiClass::Ip,
        "date" => PiiClass::Date,
        "generic" => PiiClass::GenericText,
        "none" | "non_pii" => PiiClass::NonPii,
        _ => return None,
    })
}

/// Flatten column rules into a `Classifier`. The classifier keys on
/// column name only; per-table classification lives in a later task
/// if it turns out we need it. For v0.1 column names are globally
/// unique within the allowed-tables set.
pub fn build_classifier(policy: &Policy) -> Classifier {
    let mut c = Classifier::new();
    for rule in &policy.policy.database.column_rules {
        if let Some(class) = parse_class(&rule.class) {
            c = c.with_column(&rule.column, class);
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn valid_policy_parses() {
        let p = load_from_file(&fixture("policy_valid.toml")).unwrap();
        assert_eq!(p.connection.len(), 1);
        assert!(p.connection.contains_key("production"));
        assert_eq!(p.policy.database.max_rows, 50);
        assert_eq!(p.policy.database.column_rules.len(), 3);
    }

    #[test]
    fn two_connections_rejected() {
        let err = load_from_file(&fixture("policy_two_conns.toml")).unwrap_err();
        assert!(matches!(err, PolicyError::ConnectionCount { found: 2 }));
    }

    #[test]
    fn classifier_is_built_from_columns() {
        let p = load_from_file(&fixture("policy_valid.toml")).unwrap();
        let c = build_classifier(&p);
        assert_eq!(c.classify("email"), PiiClass::Email);
        assert_eq!(c.classify("full_name"), PiiClass::Name);
        assert_eq!(c.classify("created_at"), PiiClass::NonPii);
    }
}
