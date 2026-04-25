use std::collections::HashMap;
use std::path::PathBuf;

use gaze::{
    Action, ClassRule as GazeClassRule, ColumnRule as GazeColumnRule, DefaultRule, PiiClass,
    Pipeline,
};
use gaze_recognizers::{NerDetector, NerOptions, RegexDetector};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyFile {
    #[serde(default)]
    pub connection: HashMap<String, ConnectionConfig>,
    #[serde(default)]
    pub ner: NerSection,
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

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NerSection {
    #[serde(default)]
    pub model_dir: Option<PathBuf>,
    #[serde(default)]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicySection {
    pub database: DatabasePolicy,
    #[serde(default)]
    pub logs: Option<LogsPolicy>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DatabasePolicy {
    #[serde(default, rename = "columns")]
    pub column_rules: Vec<ColumnRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ColumnRule {
    pub column: String,
    pub class: String,
    #[serde(default)]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LogsPolicy {
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub strip_patterns: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("failed to parse TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("policy must contain exactly one [connection.production] block; found {found}")]
    ConnectionCount { found: usize },
    #[error("only [connection.production] is supported (found `{name}`)")]
    NonProductionConnection { name: String },
    #[error("invalid action `{action}` for column `{column}`")]
    InvalidAction { column: String, action: String },
    #[error("unknown pii class `{class}` for column `{column}`")]
    UnknownPiiClass { column: String, class: String },
    #[error("pipeline build failed: {0}")]
    Pipeline(#[from] gaze::Error),
}

impl PolicyFile {
    pub fn from_toml(input: &str) -> Result<Self, PolicyError> {
        let policy: Self = toml::from_str(input)?;
        validate_connection(&policy)?;
        Ok(policy)
    }
}

pub fn build_pipeline(policy: &PolicyFile) -> Result<Pipeline, PolicyError> {
    let mut builder = Pipeline::builder().detector(RegexDetector::emails()?);

    if let Some(model_dir) = &policy.ner.model_dir {
        let detector = NerDetector::load_with_options(
            model_dir,
            NerOptions {
                locale: policy.ner.locale.clone(),
                threshold: 0.3,
            },
        )
        .map_err(|err| {
            PolicyError::Pipeline(gaze::Error::Policy(gaze::PolicyError::NerLoad(
                err.to_string(),
            )))
        })?;
        builder = builder.detector(detector);
    }

    for rule in &policy.policy.database.column_rules {
        let action = parse_action(rule.action.as_deref().unwrap_or("tokenize"), &rule.column)?;
        let class = parse_class(&rule.class, &rule.column)?;
        builder = builder
            .rule(GazeColumnRule::new(&rule.column, action))
            .rule(GazeClassRule::new(class, action));
    }

    if let Some(logs) = &policy.policy.logs {
        for (index, pattern) in logs.strip_patterns.iter().enumerate() {
            builder = builder.detector(RegexDetector::with_source(
                pattern,
                gaze::PiiClass::custom("log_strip"),
                &format!("log-strip-{index}"),
            )?);
        }
    }

    Ok(builder.rule(DefaultRule::new(Action::Preserve)).build()?)
}

fn parse_action(raw: &str, column: &str) -> Result<Action, PolicyError> {
    match raw {
        "tokenize" => Ok(Action::Tokenize),
        "redact" => Ok(Action::Redact),
        "format_preserve" => Ok(Action::FormatPreserve),
        "generalize" => Ok(Action::Generalize),
        "preserve" => Ok(Action::Preserve),
        other => Err(PolicyError::InvalidAction {
            column: column.to_string(),
            action: other.to_string(),
        }),
    }
}

fn validate_connection(policy: &PolicyFile) -> Result<(), PolicyError> {
    if policy.connection.len() != 1 {
        return Err(PolicyError::ConnectionCount {
            found: policy.connection.len(),
        });
    }
    let (name, _) = policy.connection.iter().next().expect("len checked");
    if name != "production" {
        return Err(PolicyError::NonProductionConnection { name: name.clone() });
    }
    Ok(())
}

fn parse_class(raw: &str, column: &str) -> Result<PiiClass, PolicyError> {
    Ok(match raw {
        "email" => PiiClass::Email,
        "name" => PiiClass::Name,
        "location" => PiiClass::Location,
        "organization" => PiiClass::Organization,
        custom if !custom.trim().is_empty() => PiiClass::custom(custom),
        _ => {
            return Err(PolicyError::UnknownPiiClass {
                column: column.to_string(),
                class: raw.to_string(),
            })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_builds_pipeline_with_ner_locale() {
        let policy = PolicyFile::from_toml(
            r#"
            [connection.production]
            kind = "mysql"
            ssh_host = "deploy@example.com"
            local_port = 13306
            remote_host = "127.0.0.1"
            remote_port = 3306
            database = "app"
            user = "gaze_ro"
            password_env = "GAZE_DB_PASSWORD"

            [ner]
            locale = "de"

            [policy.database]

            [[policy.database.columns]]
            column = "email"
            class = "email"
            action = "tokenize"
            "#,
        )
        .expect("policy");

        build_pipeline(&policy).expect("pipeline");
    }

    #[test]
    fn invalid_action_is_rejected() {
        let policy = PolicyFile::from_toml(
            r#"
            [connection.production]
            kind = "mysql"
            ssh_host = "deploy@example.com"
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
            action = "explode"
            "#,
        )
        .expect("policy");

        match build_pipeline(&policy) {
            Ok(_) => panic!("expected invalid action"),
            Err(err) => assert!(matches!(err, PolicyError::InvalidAction { .. })),
        }
    }

    #[test]
    fn policy_rejects_missing_production_connection() {
        let err = PolicyFile::from_toml(
            r#"
            [policy.database]

            [[policy.database.columns]]
            column = "email"
            class = "email"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, PolicyError::ConnectionCount { found: 0 }));
    }
}
