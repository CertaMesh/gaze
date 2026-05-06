use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;
use thiserror::Error;

use crate::{Action, LocaleTag, PiiClass, RulepackDict};

pub const DEFAULT_NER_THRESHOLD: f32 = 0.3;

#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Policy {
    pub session: SessionPolicy,
    pub detectors: Vec<DetectorSpec>,
    pub dictionaries: Vec<RulepackDict>,
    pub rules: Vec<RuleSpec>,
    pub ner: Option<NerPolicy>,
    pub rulepacks: RulepackPolicy,
    pub locale: Option<Vec<LocaleTag>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionPolicy {
    pub scope: SessionScope,
    pub ttl_secs: Option<u64>,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            scope: SessionScope::Ephemeral,
            ttl_secs: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionScope {
    Ephemeral,
    Conversation,
    Persistent,
}

impl SessionScope {
    pub fn parse(value: &str) -> Result<Self, PolicyError> {
        match value {
            "ephemeral" => Ok(SessionScope::Ephemeral),
            "conversation" => Ok(SessionScope::Conversation),
            "persistent" => Ok(SessionScope::Persistent),
            other => Err(PolicyError::SessionScopeUnknown {
                value: other.to_string(),
            }),
        }
    }
}

impl FromStr for SessionScope {
    type Err = PolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DetectorSpec {
    pub kind: DetectorKind,
    pub name: String,
    pub pattern: Option<String>,
    pub class: PiiClass,
    pub dictionary_name: Option<String>,
    pub case_sensitive: bool,
    pub token_family: String,
}

impl Default for DetectorSpec {
    fn default() -> Self {
        Self {
            kind: DetectorKind::Regex,
            name: String::new(),
            pattern: None,
            class: PiiClass::Email,
            dictionary_name: None,
            case_sensitive: false,
            token_family: "counter".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DetectorKind {
    Regex,
    Dictionary,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct NerPolicy {
    pub model_dir: Option<PathBuf>,
    pub locale: Option<String>,
    pub threshold: f32,
}

impl Default for NerPolicy {
    fn default() -> Self {
        Self {
            model_dir: None,
            locale: None,
            threshold: DEFAULT_NER_THRESHOLD,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RulepackPolicy {
    pub bundled: Vec<String>,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuleSpec {
    Class { class: PiiClass, action: Action },
    Column { column: String, action: Action },
    Default { action: Action },
}

#[derive(Debug, Error)]
#[non_exhaustive]
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
    #[error("invalid dictionary detector '{name}': {reason}")]
    BadDictionary { name: String, reason: String },
    #[error("session.ttl_secs is required when session.scope = \"persistent\"")]
    MissingTtl,
    #[error("invalid session.ttl_secs: {0}")]
    BadTtl(String),
    #[error("policy must define at least one rule")]
    NoRules,
    #[error("policy must define at least one detector")]
    NoDetectors,
    #[error(
        "legacy [[detector]] is unsupported in v0.4; migrate to [[policy.custom_recognizers]]: {0}"
    )]
    LegacyDetectorUnsupported(&'static str),
    #[error("ner load error: {0}")]
    NerLoad(String),
    #[error("ner.threshold must be between 0.0 and 1.0 inclusive, got {value}")]
    NerThresholdOutOfRange { value: f32 },
    #[error("session.scope must be one of ephemeral, conversation, persistent, got {value}")]
    SessionScopeUnknown { value: String },
    #[error("ner.locale must be a BCP47 locale tag, got {value}")]
    NerLocaleUnsupported { value: String },
    #[error("unknown bundled rulepack: {value}")]
    BundledRulepackUnknown { value: String },
    #[error("unknown locale bucket: {name}")]
    UnknownLocaleBucket { name: String },
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
    pattern: Option<String>,
    class: String,
    dictionary: Option<String>,
    #[serde(default)]
    terms: Vec<String>,
    terms_file: Option<String>,
    terms_from_context: Option<String>,
    #[serde(default)]
    case_sensitive: bool,
    token_family: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNerPolicy {
    model_dir: Option<String>,
    locale: Option<String>,
    #[serde(default)]
    threshold: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocalePolicy {
    #[serde(default)]
    active: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicyTables {
    #[serde(default)]
    rulepacks: Option<RawRulepackPolicy>,
    #[serde(default)]
    custom_recognizers: Vec<RawDetectorSpec>,
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

        if !raw.detectors.is_empty() {
            return Err(PolicyError::LegacyDetectorUnsupported(
                "https://github.com/piinuts/gaze/blob/main/docs/policy.md#migrating-detector",
            ));
        }

        let policy_tables = raw.policy.unwrap_or_default();
        let RawPolicyTables {
            rulepacks: raw_rulepacks,
            custom_recognizers,
        } = policy_tables;

        let mut detectors = Vec::with_capacity(custom_recognizers.len());
        let mut dictionaries = Vec::new();
        for detector in custom_recognizers {
            let (detector, dictionary) = parse_detector(detector)?;
            if let Some(dictionary) = dictionary {
                dictionaries.push(dictionary);
            }
            detectors.push(detector);
        }
        let rulepacks = raw_rulepacks
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
            dictionaries,
            rules,
            ner,
            rulepacks,
            locale,
        })
    }
}

fn parse_session(raw: RawSessionPolicy) -> Result<SessionPolicy, PolicyError> {
    let scope = SessionScope::parse(&raw.scope)?;

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

fn parse_detector(
    raw: RawDetectorSpec,
) -> Result<(DetectorSpec, Option<RulepackDict>), PolicyError> {
    let class = parse_class(&raw.class)?;
    match raw.kind.as_str() {
        "regex" => parse_regex_detector(raw, class),
        "dictionary" => parse_dictionary_detector(raw, class),
        other => Ok((
            DetectorSpec {
                kind: DetectorKind::Unknown(other.to_string()),
                name: raw.name,
                pattern: raw.pattern,
                class,
                dictionary_name: None,
                case_sensitive: raw.case_sensitive,
                token_family: raw.token_family.unwrap_or_else(|| "counter".to_string()),
            },
            None,
        )),
    }
}

fn parse_regex_detector(
    raw: RawDetectorSpec,
    class: PiiClass,
) -> Result<(DetectorSpec, Option<RulepackDict>), PolicyError> {
    let pattern = raw.pattern.ok_or_else(|| PolicyError::BadDictionary {
        name: raw.name.clone(),
        reason: "regex recognizers require pattern".to_string(),
    })?;
    regex::Regex::new(&pattern).map_err(|source| PolicyError::BadRegex {
        name: raw.name.clone(),
        source,
    })?;

    Ok((
        DetectorSpec {
            kind: DetectorKind::Regex,
            name: raw.name,
            pattern: Some(pattern),
            class,
            dictionary_name: None,
            case_sensitive: false,
            token_family: raw.token_family.unwrap_or_else(|| "counter".to_string()),
        },
        None,
    ))
}

fn parse_dictionary_detector(
    raw: RawDetectorSpec,
    class: PiiClass,
) -> Result<(DetectorSpec, Option<RulepackDict>), PolicyError> {
    if raw.pattern.is_some() {
        return Err(PolicyError::BadDictionary {
            name: raw.name,
            reason: "dictionary recognizers must not set pattern".to_string(),
        });
    }

    let dictionary_name = raw
        .terms_from_context
        .clone()
        .or(raw.dictionary.clone())
        .unwrap_or_else(|| raw.name.clone());
    let mut terms = raw.terms;
    if let Some(path) = raw.terms_file {
        let path = expand_home(path)?;
        let file = fs::read_to_string(&path).map_err(PolicyError::Io)?;
        terms.extend(
            file.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_string),
        );
    }

    let dictionary = if raw.terms_from_context.is_some() {
        if !terms.is_empty() {
            return Err(PolicyError::BadDictionary {
                name: raw.name.clone(),
                reason: "terms_from_context cannot be combined with terms or terms_file"
                    .to_string(),
            });
        }
        None
    } else {
        if terms.is_empty() {
            return Err(PolicyError::BadDictionary {
                name: raw.name.clone(),
                reason: "dictionary recognizers require terms, terms_file, or terms_from_context"
                    .to_string(),
            });
        }
        if !raw.case_sensitive && terms.iter().any(|term| !term.is_ascii()) {
            return Err(PolicyError::BadDictionary {
                name: raw.name.clone(),
                reason:
                    "unicode dictionary insensitive matching unsupported in v0.4.0, use case_sensitive = true"
                        .to_string(),
            });
        }
        Some(RulepackDict {
            name: dictionary_name.clone(),
            terms,
            case_sensitive: raw.case_sensitive,
        })
    };

    Ok((
        DetectorSpec {
            kind: DetectorKind::Dictionary,
            name: raw.name,
            pattern: None,
            class,
            dictionary_name: Some(dictionary_name),
            case_sensitive: raw.case_sensitive,
            token_family: raw.token_family.unwrap_or_else(|| "counter".to_string()),
        },
        dictionary,
    ))
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
    let threshold = raw.threshold.unwrap_or(DEFAULT_NER_THRESHOLD);
    if !(0.0..=1.0).contains(&threshold) {
        return Err(PolicyError::NerThresholdOutOfRange { value: threshold });
    }
    if let Some(locale) = &raw.locale {
        validate_ner_locale(locale)?;
    }
    Ok(NerPolicy {
        model_dir: raw.model_dir.map(expand_home).transpose()?,
        locale: raw.locale,
        threshold,
    })
}

pub fn validate_ner_locale(locale: &str) -> Result<(), PolicyError> {
    LocaleTag::parse(locale)
        .map(|_| ())
        .map_err(|_| PolicyError::NerLocaleUnsupported {
            value: locale.to_string(),
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

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = '(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'
class = "email"

[ner]
model_dir = "~/.cache/gaze/model"
locale = "de"
threshold = 0.4

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
        let ner = policy.ner.unwrap();
        assert_eq!(
            ner.model_dir,
            Some(PathBuf::from("/tmp/gaze-home/.cache/gaze/model"))
        );
        assert_eq!(ner.threshold, 0.4);
    }

    #[test]
    fn rejects_ner_threshold_out_of_range() {
        let raw = r#"
[session]
scope = "ephemeral"

[ner]
threshold = 1.1

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = ".+"
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#;
        let raw: RawPolicy = toml::from_str(raw).expect("raw policy");

        assert!(matches!(
            Policy::try_from(raw),
            Err(PolicyError::NerThresholdOutOfRange { value }) if value == 1.1
        ));
    }

    #[test]
    fn accepts_bcp47_ner_locale_hints() {
        for locale in ["de", "en-US", "pt-BR", "zh-Hant"] {
            assert!(
                validate_ner_locale(locale).is_ok(),
                "NER locale hints should accept BCP47-shaped tag {locale}"
            );
        }

        assert!(matches!(
            validate_ner_locale("bad locale!"),
            Err(PolicyError::NerLocaleUnsupported { value }) if value == "bad locale!"
        ));
    }

    #[test]
    fn rejects_unknown_session_scope_with_typed_error() {
        let raw = r#"
[session]
scope = "forever"

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = ".+"
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#;

        let raw = toml::from_str::<RawPolicy>(raw).unwrap();
        let err = Policy::try_from(raw).unwrap_err();

        assert!(matches!(
            err,
            PolicyError::SessionScopeUnknown { value } if value == "forever"
        ));
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

[[policy.custom_recognizers]]
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

    #[test]
    fn loads_dictionary_custom_recognizer_terms() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("policy.toml");
        fs::write(
            &path,
            r#"
[session]
scope = "ephemeral"

[[policy.custom_recognizers]]
kind = "dictionary"
name = "songs"
class = "custom:song"
terms = ["Song A"]
case_sensitive = true

[[rule]]
kind = "class"
class = "custom:song"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#,
        )
        .unwrap();

        let policy = Policy::load(&path).unwrap();
        assert_eq!(policy.detectors[0].kind, DetectorKind::Dictionary);
        assert_eq!(
            policy.detectors[0].dictionary_name.as_deref(),
            Some("songs")
        );
        assert_eq!(policy.dictionaries[0].terms, vec!["Song A"]);
    }
}
