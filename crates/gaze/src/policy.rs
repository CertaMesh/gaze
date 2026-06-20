use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;
use thiserror::Error;

use crate::{
    Action, CollisionMembership, LocaleTag, PiiClass, RulepackDict, SafetyTier,
    RESERVED_BUNDLED_FAMILIES,
};

pub const DEFAULT_NER_THRESHOLD: f32 = 0.3;

/// `major.minor` prefix of the policy schema versions this build accepts.
///
/// A policy.toml `schema_version` (or the [`DEFAULT_POLICY_SCHEMA_VERSION`]
/// soft default applied when the field is omitted) must start with this string.
/// Anything else fails closed at load with
/// [`PolicyError::PolicySchemaUnsupported`], so operators upgrading the binary
/// learn about a contract break instead of silently mis-loading the policy.
///
/// Mirrors the rulepack-side `SUPPORTED_SCHEMA_MAJOR_MINOR` in
/// [`crate::rulepack`].
pub const SUPPORTED_POLICY_SCHEMA_MAJOR_MINOR: &str = "0.1";

/// Schema version stamped on policy.toml documents that omit the field.
///
/// Existing 0.6.x / 0.7.x policies were written before `schema_version` was
/// introduced; soft-defaulting them to `"0.1.0"` keeps the loader backward
/// compatible. New policies should declare `schema_version = "0.1.0"`
/// explicitly so future migrations can be detected.
pub const DEFAULT_POLICY_SCHEMA_VERSION: &str = "0.1.0";

/// Loaded redaction policy from a TOML configuration file.
///
/// Defines which rulepacks activate, which recognizers are enabled, and the locale chain.
/// Load with [`Policy::load`] for library use or [`Policy::load_for_cli`] for CLI hosts.
/// Both signatures take `&std::path::Path`.
///
/// Production deployments **must** use a policy -- the no-policy builder path is for
/// development smoke-testing only and has an unauditable detection posture.
///
/// See `docs/reference/policy.md` in the repository for the full TOML schema reference.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Policy {
    pub session: SessionPolicy,
    pub detectors: Vec<DetectorSpec>,
    pub dictionaries: Vec<RulepackDict>,
    pub rules: Vec<RuleSpec>,
    pub ner: Option<NerPolicy>,
    pub rulepacks: RulepackPolicy,
    pub locale: Option<Vec<LocaleTag>>,
    /// Declared policy schema version (e.g. `"0.1.0"`).
    ///
    /// Populated from policy.toml's top-level `schema_version` field. Loader
    /// requires the `major.minor` prefix to match
    /// [`SUPPORTED_POLICY_SCHEMA_MAJOR_MINOR`]; documents that omit the field
    /// soft-default to [`DEFAULT_POLICY_SCHEMA_VERSION`] for backward
    /// compatibility with policies written before v0.7.2.
    pub schema_version: String,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            session: SessionPolicy::default(),
            detectors: Vec::new(),
            dictionaries: Vec::new(),
            rules: Vec::new(),
            ner: None,
            rulepacks: RulepackPolicy::default(),
            locale: None,
            schema_version: DEFAULT_POLICY_SCHEMA_VERSION.to_string(),
        }
    }
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
    pub collision: Option<CollisionMembership>,
    pub safety_tier: SafetyTier,
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
            collision: None,
            safety_tier: SafetyTier::OptIn,
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
    pub auto_activate_locale_gated: bool,
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
    #[error(
        "regex detector '{name}' shadows Gaze token shape sample '{shadowed_shape}' with pattern '{pattern}'"
    )]
    TokenShapeShadow {
        name: String,
        pattern: String,
        shadowed_shape: String,
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
    #[error("reserved collision family '{family}' cannot be used by policy custom recognizers")]
    ReservedCollisionFamily { family: String },
    #[error("invalid collision metadata for custom recognizer '{name}': {reason}")]
    InvalidCollisionMetadata { name: String, reason: String },
    #[error("{0}")]
    UnsupportedRuleKind(String),
    #[error("unsupported policy schema_version {found}; supported {supported}")]
    PolicySchemaUnsupported {
        found: String,
        supported: &'static str,
    },
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
    #[serde(default = "default_raw_schema_version")]
    schema_version: String,
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

fn default_raw_schema_version() -> String {
    DEFAULT_POLICY_SCHEMA_VERSION.to_string()
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
    #[serde(default)]
    collision: Option<crate::rulepack::RawCollisionSpec>,
    #[serde(default)]
    safety_tier: Option<String>,
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
        if !raw
            .schema_version
            .starts_with(SUPPORTED_POLICY_SCHEMA_MAJOR_MINOR)
        {
            return Err(PolicyError::PolicySchemaUnsupported {
                found: raw.schema_version,
                supported: SUPPORTED_POLICY_SCHEMA_MAJOR_MINOR,
            });
        }
        let schema_version = raw.schema_version;
        let session = parse_session(raw.session)?;

        if !raw.detectors.is_empty() {
            return Err(PolicyError::LegacyDetectorUnsupported(
                "https://github.com/EmpireTwo/gaze/blob/main/docs/reference/policy.md#migrating-detector",
            ));
        }

        let policy_tables = raw.policy.unwrap_or_default();
        let RawPolicyTables {
            rulepacks: raw_rulepacks,
            custom_recognizers,
        } = policy_tables;

        let ner = raw.ner.map(parse_ner).transpose()?;
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
                auto_activate_locale_gated: false,
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

        let locale = raw.locale.map(parse_locale_policy).transpose()?.flatten();

        Ok(Self {
            session,
            detectors,
            dictionaries,
            rules,
            ner,
            rulepacks,
            locale,
            schema_version,
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
    let collision = parse_detector_collision(&raw)?;
    let safety_tier = parse_custom_safety_tier(raw.safety_tier.as_deref())?;
    match raw.kind.as_str() {
        "regex" => parse_regex_detector(raw, class, collision, safety_tier),
        "dictionary" => parse_dictionary_detector(raw, class, collision, safety_tier),
        other => Ok((
            DetectorSpec {
                kind: DetectorKind::Unknown(other.to_string()),
                name: raw.name,
                pattern: raw.pattern,
                class,
                dictionary_name: None,
                case_sensitive: raw.case_sensitive,
                token_family: raw.token_family.unwrap_or_else(|| "counter".to_string()),
                collision,
                safety_tier,
            },
            None,
        )),
    }
}

fn parse_detector_collision(
    raw: &RawDetectorSpec,
) -> Result<Option<CollisionMembership>, PolicyError> {
    let Some(collision) = raw.collision.clone() else {
        return Ok(None);
    };
    if RESERVED_BUNDLED_FAMILIES
        .iter()
        .any(|family| *family == collision.family)
    {
        return Err(PolicyError::ReservedCollisionFamily {
            family: collision.family,
        });
    }
    crate::rulepack::parse_collision_membership(&raw.name, collision)
        .map(Some)
        .map_err(|err| PolicyError::InvalidCollisionMetadata {
            name: raw.name.clone(),
            reason: err.to_string(),
        })
}

fn parse_regex_detector(
    raw: RawDetectorSpec,
    class: PiiClass,
    collision: Option<CollisionMembership>,
    safety_tier: SafetyTier,
) -> Result<(DetectorSpec, Option<RulepackDict>), PolicyError> {
    let pattern = raw.pattern.ok_or_else(|| PolicyError::BadDictionary {
        name: raw.name.clone(),
        reason: "regex recognizers require pattern".to_string(),
    })?;
    let compiled = regex::Regex::new(&pattern).map_err(|source| PolicyError::BadRegex {
        name: raw.name.clone(),
        source,
    })?;
    crate::token_shape::reject_if_shadows_token_shape(&compiled, &raw.name).map_err(|shadow| {
        PolicyError::TokenShapeShadow {
            name: shadow.recognizer_id,
            pattern: shadow.offending_pattern,
            shadowed_shape: shadow.shadowed_shape,
        }
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
            collision,
            safety_tier,
        },
        None,
    ))
}

fn parse_dictionary_detector(
    raw: RawDetectorSpec,
    class: PiiClass,
    collision: Option<CollisionMembership>,
    safety_tier: SafetyTier,
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
        Some(RulepackDict::new(
            dictionary_name.clone(),
            terms,
            raw.case_sensitive,
        ))
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
            collision,
            safety_tier,
        },
        dictionary,
    ))
}

fn parse_custom_safety_tier(raw: Option<&str>) -> Result<SafetyTier, PolicyError> {
    raw.map(SafetyTier::parse)
        .transpose()
        .map_err(|err| PolicyError::BadTtl(err.to_string()))
        .map(|tier| tier.unwrap_or(SafetyTier::OptIn))
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
    let (bundled, auto_activate_locale_gated) = normalize_bundled_rulepacks(raw.bundled);
    Ok(RulepackPolicy {
        bundled,
        paths: raw
            .paths
            .into_iter()
            .map(expand_home)
            .collect::<Result<_, _>>()?,
        auto_activate_locale_gated,
    })
}

fn normalize_bundled_rulepacks(raw: Vec<String>) -> (Vec<String>, bool) {
    let mut bundled = Vec::with_capacity(raw.len());
    let mut auto_activate_locale_gated = false;
    for bundle in raw {
        if bundle == "core-extended" {
            auto_activate_locale_gated = true;
            tracing::warn!(
                "`core-extended` bundled rulepack is deprecated since v0.8.0; use `core` with an explicit locale"
            );
            if !bundled.iter().any(|existing| existing == "core") {
                bundled.push("core".to_string());
            }
        } else if !bundled.iter().any(|existing| existing == &bundle) {
            bundled.push(bundle);
        }
    }
    (bundled, auto_activate_locale_gated)
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
            if name.starts_with("family:") {
                Ok(PiiClass::Custom(name.to_string()))
            } else {
                Ok(PiiClass::custom(name))
            }
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
pattern = 'alice@example\.invalid'
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
    fn custom_email_recognizer_loads_under_preservation() {
        let raw = r#"
[session]
scope = "ephemeral"

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'alice@example\.invalid'
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#;

        let raw = toml::from_str::<RawPolicy>(raw).unwrap();
        let policy = Policy::try_from(raw).unwrap();

        assert_eq!(policy.detectors.len(), 1);
        assert_eq!(policy.detectors[0].name, "emails");
    }

    #[test]
    fn policy_with_custom_collision_family_parses() {
        let raw = r#"
[session]
scope = "ephemeral"

[[policy.custom_recognizers]]
kind = "regex"
name = "tenant.order"
pattern = 'ORD-[0-9]+'
class = "custom:tenant_ref"

[policy.custom_recognizers.collision]
family = "tenant-orders"
variant = "order-id"
precedence = 50

[[rule]]
kind = "default"
action = "preserve"
"#;

        let raw = toml::from_str::<RawPolicy>(raw).unwrap();
        let policy = Policy::try_from(raw).unwrap();

        let collision = policy.detectors[0].collision.as_ref().expect("collision");
        assert_eq!(collision.family, "tenant-orders");
        assert_eq!(collision.variant, "order-id");
        assert_eq!(collision.precedence, 50);
    }

    #[test]
    fn policy_with_reserved_family_rejected() {
        let raw = r#"
[session]
scope = "ephemeral"

[[policy.custom_recognizers]]
kind = "regex"
name = "tenant.card"
pattern = '[0-9]+'
class = "custom:tenant_card"

[policy.custom_recognizers.collision]
family = "payment-card-or-iban"
variant = "tenant-card"
precedence = 50

[[rule]]
kind = "default"
action = "preserve"
"#;

        let raw = toml::from_str::<RawPolicy>(raw).unwrap();
        let err = Policy::try_from(raw).unwrap_err();

        assert!(matches!(
            err,
            PolicyError::ReservedCollisionFamily { family }
                if family == "payment-card-or-iban"
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

    fn minimal_policy_body(schema_line: &str) -> String {
        format!(
            r#"{schema_line}
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = 'a@b'
class = "email"

[[rule]]
kind = "default"
action = "preserve"
"#
        )
    }

    fn write_policy(dir: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("policy.toml");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn schema_version_omitted_soft_defaults_to_supported() {
        let dir = tempdir().unwrap();
        let path = write_policy(&dir, &minimal_policy_body(""));
        let policy = Policy::load(&path).expect("missing schema_version should soft-default");
        assert_eq!(policy.schema_version, DEFAULT_POLICY_SCHEMA_VERSION);
    }

    #[test]
    fn schema_version_explicit_matching_major_minor_is_accepted() {
        let dir = tempdir().unwrap();
        let path = write_policy(&dir, &minimal_policy_body(r#"schema_version = "0.1.7""#));
        let policy = Policy::load(&path).expect("0.1.x must be accepted");
        assert_eq!(policy.schema_version, "0.1.7");
    }

    #[test]
    fn schema_version_unsupported_major_fails_closed() {
        let dir = tempdir().unwrap();
        let path = write_policy(&dir, &minimal_policy_body(r#"schema_version = "0.2.0""#));
        let err = Policy::load(&path).expect_err("0.2.x must be rejected");
        match err {
            PolicyError::PolicySchemaUnsupported { found, supported } => {
                assert_eq!(found, "0.2.0");
                assert_eq!(supported, SUPPORTED_POLICY_SCHEMA_MAJOR_MINOR);
            }
            other => panic!("expected PolicySchemaUnsupported, got {other:?}"),
        }
    }

    #[test]
    fn schema_version_unsupported_fails_before_other_validation() {
        // Body has zero rules — would normally trip NoRules — but the schema
        // gate must fire first so adopters see the version mismatch, not a
        // downstream validation surprise.
        let dir = tempdir().unwrap();
        let path = write_policy(
            &dir,
            r#"
schema_version = "9.9.9"
[session]
scope = "persistent"
ttl_secs = 86400
"#,
        );
        let err = Policy::load(&path).expect_err("must reject schema first");
        assert!(
            matches!(err, PolicyError::PolicySchemaUnsupported { .. }),
            "expected PolicySchemaUnsupported, got {err:?}"
        );
    }
}
