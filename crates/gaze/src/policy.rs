use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::{Action, LocaleTag, PiiClass};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub session: SessionPolicy,
    pub detectors: Vec<DetectorSpec>,
    pub rules: Vec<RuleSpec>,
    pub ner: Option<NerPolicy>,
    pub rulepacks: RulepackPolicy,
    pub locale: Option<Vec<LocaleTag>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPolicy {
    pub scope: SessionScope,
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionScope {
    Ephemeral,
    Conversation,
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectorSpec {
    pub kind: DetectorKind,
    pub name: String,
    pub pattern: String,
    pub class: PiiClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectorKind {
    Regex,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NerPolicy {
    pub model_dir: Option<PathBuf>,
    pub locale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulepackPolicy {
    pub bundled: Vec<String>,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSpec {
    Class { class: PiiClass, action: Action },
    Column { column: String, action: Action },
    Default { action: Action },
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("failed to parse policy.toml: {0}")]
    TomlParse(#[source] toml::de::Error),
    #[error("failed to read policy file: {0}")]
    Io(#[source] std::io::Error),
    #[error("unknown pii class: {0}")]
    UnknownClass(String),
    #[error("invalid regex for detector '{name}': {source}")]
    BadRegex {
        name: String,
        #[source]
        source: regex::Error,
    },
    #[error("session.ttl_secs is required when session.scope = \"persistent\"")]
    MissingTtl,
    #[error("invalid session.ttl_secs: {0}")]
    BadTtl(String),
    #[error("policy must define at least one rule")]
    NoRules,
    #[error("policy must define at least one detector")]
    NoDetectors,
    #[error("ner load error: {0}")]
    NerLoad(String),
    #[error("{0}")]
    UnsupportedRuleKind(String),
}

impl Policy {
    pub fn load(path: &Path) -> Result<Policy, PolicyError> {
        let raw = fs::read_to_string(path).map_err(PolicyError::Io)?;
        let raw: RawPolicy = toml::from_str(&raw).map_err(PolicyError::TomlParse)?;
        raw.try_into()
    }

    pub fn load_for_cli(path: &Path) -> Result<Policy, PolicyError> {
        let policy = Self::load(path)?;
        if policy
            .rules
            .iter()
            .any(|rule| matches!(rule, RuleSpec::Column { .. }))
        {
            return Err(PolicyError::UnsupportedRuleKind(
                "column rules not supported in CLI mode".to_string(),
            ));
        }
        Ok(policy)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    session: RawSessionPolicy,
    #[serde(rename = "detector", default)]
    detectors: Vec<RawDetectorSpec>,
    #[serde(rename = "rule", default)]
    rules: Vec<RawRuleSpec>,
    #[serde(default)]
    ner: Option<RawNerPolicy>,
    #[serde(default)]
    locale: Option<RawLocalePolicy>,
    #[serde(default)]
    policy: Option<RawPolicyTables>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSessionPolicy {
    scope: String,
    ttl_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDetectorSpec {
    kind: String,
    name: String,
    pattern: String,
    class: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNerPolicy {
    model_dir: Option<String>,
    locale: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocalePolicy {
    #[serde(default)]
    active: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicyTables {
    #[serde(default)]
    rulepacks: Option<RawRulepackPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRulepackPolicy {
    #[serde(default)]
    bundled: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRuleSpec {
    kind: String,
    class: Option<String>,
    column: Option<String>,
    action: String,
}

impl TryFrom<RawPolicy> for Policy {
    type Error = PolicyError;

    fn try_from(raw: RawPolicy) -> Result<Self, Self::Error> {
        let session = parse_session(raw.session)?;

        let mut detectors = Vec::with_capacity(raw.detectors.len());
        for detector in raw.detectors {
            detectors.push(parse_detector(detector)?);
        }
        let rulepacks = raw
            .policy
            .and_then(|policy| policy.rulepacks)
            .map(parse_rulepack_policy)
            .transpose()?
            .unwrap_or_else(|| RulepackPolicy {
                bundled: vec!["core".to_string()],
                paths: Vec::new(),
            });

        if detectors.is_empty() && rulepacks.bundled.is_empty() && rulepacks.paths.is_empty() {
            return Err(PolicyError::NoDetectors);
        }

        let mut rules = Vec::with_capacity(raw.rules.len());
        for rule in raw.rules {
            rules.push(parse_rule(rule)?);
        }
        if rules.is_empty() {
            return Err(PolicyError::NoRules);
        }

        let ner = raw.ner.map(parse_ner).transpose()?;
        let locale = raw.locale.map(parse_locale_policy).transpose()?.flatten();

        Ok(Self {
            session,
            detectors,
            rules,
            ner,
            rulepacks,
            locale,
        })
    }
}

fn parse_session(raw: RawSessionPolicy) -> Result<SessionPolicy, PolicyError> {
    let scope = match raw.scope.as_str() {
        "ephemeral" => SessionScope::Ephemeral,
        "conversation" => SessionScope::Conversation,
        "persistent" => SessionScope::Persistent,
        other => {
            return Err(PolicyError::BadTtl(format!(
                "unknown session.scope '{other}'"
            )))
        }
    };

    match scope {
        SessionScope::Persistent => match raw.ttl_secs {
            Some(0) => Err(PolicyError::BadTtl(
                "session.ttl_secs must be greater than zero".to_string(),
            )),
            Some(ttl_secs) => Ok(SessionPolicy {
                scope,
                ttl_secs: Some(ttl_secs),
            }),
            None => Err(PolicyError::MissingTtl),
        },
        _ => {
            if raw.ttl_secs == Some(0) {
                return Err(PolicyError::BadTtl(
                    "session.ttl_secs must be greater than zero".to_string(),
                ));
            }
            Ok(SessionPolicy {
                scope,
                ttl_secs: raw.ttl_secs,
            })
        }
    }
}

fn parse_detector(raw: RawDetectorSpec) -> Result<DetectorSpec, PolicyError> {
    regex::Regex::new(&raw.pattern).map_err(|source| PolicyError::BadRegex {
        name: raw.name.clone(),
        source,
    })?;

    Ok(DetectorSpec {
        kind: match raw.kind.as_str() {
            "regex" => DetectorKind::Regex,
            other => DetectorKind::Unknown(other.to_string()),
        },
        name: raw.name,
        pattern: raw.pattern,
        class: parse_class(&raw.class)?,
    })
}

fn parse_rule(raw: RawRuleSpec) -> Result<RuleSpec, PolicyError> {
    let action = parse_action(&raw.action)?;
    match raw.kind.as_str() {
        "class" => {
            let class = raw
                .class
                .ok_or_else(|| PolicyError::UnknownClass("missing rule.class".to_string()))?;
            Ok(RuleSpec::Class {
                class: parse_class(&class)?,
                action,
            })
        }
        "column" => Ok(RuleSpec::Column {
            column: raw
                .column
                .ok_or_else(|| PolicyError::BadTtl("missing rule.column".to_string()))?,
            action,
        }),
        "default" => Ok(RuleSpec::Default { action }),
        other => Err(PolicyError::BadTtl(format!("unknown rule.kind '{other}'"))),
    }
}

fn parse_ner(raw: RawNerPolicy) -> Result<NerPolicy, PolicyError> {
    Ok(NerPolicy {
        model_dir: raw.model_dir.map(expand_home).transpose()?,
        locale: raw.locale,
    })
}

fn parse_locale_policy(raw: RawLocalePolicy) -> Result<Option<Vec<LocaleTag>>, PolicyError> {
    if raw.active.is_empty() {
        return Ok(None);
    }
    raw.active
        .into_iter()
        .map(|locale| {
            LocaleTag::parse(&locale)
                .map_err(|_| PolicyError::BadTtl(format!("unsupported locale tag '{locale}'")))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn parse_rulepack_policy(raw: RawRulepackPolicy) -> Result<RulepackPolicy, PolicyError> {
    Ok(RulepackPolicy {
        bundled: raw.bundled,
        paths: raw
            .paths
            .into_iter()
            .map(expand_home)
            .collect::<Result<_, _>>()?,
    })
}

fn expand_home(path: String) -> Result<PathBuf, PolicyError> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = env::var("HOME")
            .map_err(|_| PolicyError::BadTtl("HOME is not set for ~/ expansion".to_string()))?;
        Ok(PathBuf::from(home).join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

fn parse_class(input: &str) -> Result<PiiClass, PolicyError> {
    let lower = input.trim().to_ascii_lowercase();
    match lower.as_str() {
        "email" => Ok(PiiClass::Email),
        "name" => Ok(PiiClass::Name),
        "location" => Ok(PiiClass::Location),
        "organization" => Ok(PiiClass::Organization),
        custom if custom.starts_with("custom:") => {
            let name = input
                .trim()
                .split_once(':')
                .map(|(_, name)| name)
                .unwrap_or_default();
            if name.trim().is_empty() {
                return Err(PolicyError::UnknownClass(input.to_string()));
            }
            Ok(PiiClass::custom(name))
        }
        _ => Err(PolicyError::UnknownClass(input.to_string())),
    }
}

fn parse_action(input: &str) -> Result<Action, PolicyError> {
    match input {
        "tokenize" => Ok(Action::Tokenize),
        "redact" => Ok(Action::Redact),
        "format_preserve" => Ok(Action::FormatPreserve),
        "generalize" => Ok(Action::Generalize),
        "preserve" => Ok(Action::Preserve),
        other => Err(PolicyError::BadTtl(format!(
            "unknown rule.action '{other}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn loads_policy_and_expands_home() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("policy.toml");
        fs::write(
            &path,
            r#"
[session]
scope = "persistent"
ttl_secs = 86400

[[detector]]
kind = "regex"
name = "emails"
pattern = '(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'
class = "email"

[ner]
model_dir = "~/.cache/gaze/model"
locale = "de"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
        )
        .unwrap();

        let old_home = env::var_os("HOME");
        env::set_var("HOME", "/tmp/gaze-home");
        let policy = Policy::load(&path).unwrap();
        match old_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }

        assert_eq!(policy.session.scope, SessionScope::Persistent);
        assert_eq!(policy.session.ttl_secs, Some(86400));
        assert_eq!(policy.detectors.len(), 1);
        assert_eq!(policy.rules.len(), 2);
        assert_eq!(
            policy.ner.unwrap().model_dir,
            Some(PathBuf::from("/tmp/gaze-home/.cache/gaze/model"))
        );
    }

    #[test]
    fn rejects_unknown_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("policy.toml");
        fs::write(
            &path,
            r#"
[session]
scope = "ephemeral"
bogus = true

[[detector]]
kind = "regex"
name = "emails"
pattern = ".+"
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#,
        )
        .unwrap();

        assert!(matches!(
            Policy::load(&path),
            Err(PolicyError::TomlParse(_))
        ));
    }
}
