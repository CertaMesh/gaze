use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use crate::{LocaleTag, PiiClass};

const SUPPORTED_SCHEMA_MAJOR_MINOR: &str = "0.1.";

#[derive(Debug, Clone, PartialEq)]
pub struct Rulepack {
    pub schema_version: String,
    pub rulepack_id: String,
    pub rulepack_version: String,
    pub default_locales: Vec<LocaleTag>,
    pub recognizers: Vec<RecognizerSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecognizerSpec {
    pub id: String,
    pub class: PiiClass,
    pub enabled: bool,
    pub locales: Vec<LocaleTag>,
    pub matcher: RawMatch,
    pub context: Option<ContextSpec>,
    pub validator: Option<ValidatorSpec>,
    pub normalizer: Option<NormalizerSpec>,
    pub scoring: ScoringSpec,
    pub token: TokenSpec,
    pub source: Option<SourceSpec>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum RawMatch {
    Regex { pattern: String },
    Dictionary {
        #[serde(default)]
        terms: Vec<String>,
        #[serde(default)]
        terms_file: Option<String>,
        #[serde(default)]
        terms_from_context: Option<String>,
        #[serde(default)]
        case_sensitive: bool,
    },
    Ner { model_ref: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextSpec {
    pub hotwords: Vec<String>,
    pub window: u16,
    pub boost: f32,
    pub exclusions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatorSpec {
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizerSpec {
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoringSpec {
    pub base: f32,
    pub priority: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenSpec {
    pub family: String,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceSpec {
    pub origin: String,
    pub from: Option<String>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulepackSource {
    Embedded(&'static str),
    Path(PathBuf),
}

#[derive(Debug, Error)]
pub enum RulepackError {
    #[error("failed to read rulepack: {0}")]
    Io(#[source] std::io::Error),
    #[error("failed to parse rulepack TOML: {0}")]
    Toml(#[source] toml::de::Error),
    #[error("unsupported rulepack schema_version {found}; supported {supported}")]
    SchemaVersion { found: String, supported: String },
    #[error("unknown pii class: {0}")]
    UnknownClass(String),
    #[error("unknown locale: {0}")]
    UnknownLocale(String),
}

impl Rulepack {
    pub fn load(source: RulepackSource) -> Result<Rulepack, RulepackError> {
        let raw = match source {
            RulepackSource::Embedded(contents) => contents.to_string(),
            RulepackSource::Path(path) => std::fs::read_to_string(path).map_err(RulepackError::Io)?,
        };
        Self::parse(&raw)
    }

    pub fn parse(raw: &str) -> Result<Rulepack, RulepackError> {
        let raw: RawRulepack = toml::from_str(raw).map_err(RulepackError::Toml)?;
        raw.try_into()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRulepack {
    schema_version: String,
    rulepack_id: String,
    rulepack_version: String,
    #[serde(default)]
    default_locales: Vec<String>,
    #[serde(default)]
    recognizers: Vec<RawRecognizerSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRecognizerSpec {
    id: String,
    class: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    locales: Vec<String>,
    #[serde(rename = "match")]
    matcher: RawMatch,
    #[serde(default)]
    context: Option<RawContextSpec>,
    #[serde(default)]
    validator: Option<RawValidatorSpec>,
    #[serde(default)]
    normalizer: Option<RawNormalizerSpec>,
    #[serde(default)]
    scoring: Option<RawScoringSpec>,
    token: RawTokenSpec,
    #[serde(default)]
    source: Option<RawSourceSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContextSpec {
    #[serde(default)]
    hotwords: Vec<String>,
    #[serde(default)]
    window: u16,
    #[serde(default)]
    boost: f32,
    #[serde(default)]
    exclusions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawValidatorSpec {
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNormalizerSpec {
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScoringSpec {
    #[serde(default = "default_base_score")]
    base: f32,
    #[serde(default)]
    priority: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTokenSpec {
    family: String,
    format: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSourceSpec {
    origin: String,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    license: Option<String>,
}

impl TryFrom<RawRulepack> for Rulepack {
    type Error = RulepackError;

    fn try_from(raw: RawRulepack) -> Result<Self, Self::Error> {
        if !raw.schema_version.starts_with(SUPPORTED_SCHEMA_MAJOR_MINOR) {
            return Err(RulepackError::SchemaVersion {
                found: raw.schema_version,
                supported: "~0.1.x".to_string(),
            });
        }

        let default_locales = parse_locales(raw.default_locales)?;
        let recognizers = raw
            .recognizers
            .into_iter()
            .map(|recognizer| parse_recognizer(recognizer, &default_locales))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            schema_version: raw.schema_version,
            rulepack_id: raw.rulepack_id,
            rulepack_version: raw.rulepack_version,
            default_locales,
            recognizers,
        })
    }
}

fn parse_recognizer(
    raw: RawRecognizerSpec,
    default_locales: &[LocaleTag],
) -> Result<RecognizerSpec, RulepackError> {
    let locales = if raw.locales.is_empty() {
        default_locales.to_vec()
    } else {
        parse_locales(raw.locales)?
    };

    Ok(RecognizerSpec {
        id: raw.id,
        class: parse_class(&raw.class)?,
        enabled: raw.enabled,
        locales,
        matcher: raw.matcher,
        context: raw.context.map(|context| ContextSpec {
            hotwords: context.hotwords,
            window: context.window,
            boost: context.boost,
            exclusions: context.exclusions,
        }),
        validator: raw.validator.map(|validator| ValidatorSpec {
            kind: validator.kind,
        }),
        normalizer: raw.normalizer.map(|normalizer| NormalizerSpec {
            kind: normalizer.kind,
        }),
        scoring: raw.scoring.map_or_else(
            || ScoringSpec {
                base: default_base_score(),
                priority: 0,
            },
            |scoring| ScoringSpec {
                base: scoring.base,
                priority: scoring.priority,
            },
        ),
        token: TokenSpec {
            family: raw.token.family,
            format: raw.token.format,
        },
        source: raw.source.map(|source| SourceSpec {
            origin: source.origin,
            from: source.from,
            license: source.license,
        }),
    })
}

pub fn parse_class(input: &str) -> Result<PiiClass, RulepackError> {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "email" => Ok(PiiClass::Email),
        "name" => Ok(PiiClass::Name),
        "location" => Ok(PiiClass::Location),
        "organization" => Ok(PiiClass::Organization),
        custom if custom.starts_with("custom:") => {
            let name = trimmed
                .split_once(':')
                .map(|(_, name)| name)
                .unwrap_or_default();
            if name.trim().is_empty() {
                return Err(RulepackError::UnknownClass(input.to_string()));
            }
            Ok(PiiClass::custom(name))
        }
        _ => Err(RulepackError::UnknownClass(input.to_string())),
    }
}

fn parse_locales(locales: Vec<String>) -> Result<Vec<LocaleTag>, RulepackError> {
    locales
        .into_iter()
        .map(|locale| {
            LocaleTag::parse(&locale).map_err(|_| RulepackError::UnknownLocale(locale.clone()))
        })
        .collect()
}

fn default_true() -> bool {
    true
}

fn default_base_score() -> f32 {
    0.70
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORE: &str = r#"
schema_version = "0.1.0"
rulepack_id = "gaze-core"
rulepack_version = "0.4.0"
default_locales = ["global"]

[[recognizers]]
id = "email.global"
class = "Email"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = '''(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'''

[recognizers.context]
hotwords = ["email", "e-mail", "mail"]
window = 12
boost = 0.10
exclusions = ["example.com"]

[recognizers.validator]
kind = "email_rfc"

[recognizers.normalizer]
kind = "email_canonical"

[recognizers.scoring]
base = 0.70
priority = 90

[recognizers.token]
family = "email.counter"
format = "Email_{n}"

[recognizers.source]
origin = "ported"
from = "presidio"
license = "Apache-2.0"
"#;

    #[test]
    fn parses_core_rulepack_end_to_end() {
        let rulepack = Rulepack::parse(CORE).expect("core rulepack");

        assert_eq!(rulepack.rulepack_id, "gaze-core");
        assert_eq!(rulepack.default_locales, vec![LocaleTag::Global]);
        assert_eq!(rulepack.recognizers.len(), 1);
        let recognizer = &rulepack.recognizers[0];
        assert_eq!(recognizer.id, "email.global");
        assert_eq!(recognizer.class, PiiClass::Email);
        assert_eq!(recognizer.scoring.priority, 90);
        assert!(matches!(recognizer.matcher, RawMatch::Regex { .. }));
    }

    #[test]
    fn rejects_unknown_fields_with_parent_table_context() {
        let err = Rulepack::parse(
            r#"
schema_version = "0.1.0"
rulepack_id = "bad"
rulepack_version = "0.4.0"
default_locales = ["global"]
bogus = true
"#,
        )
        .expect_err("unknown field must fail");

        assert!(matches!(err, RulepackError::Toml(_)));
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let err = Rulepack::parse(
            r#"
schema_version = "0.2.0"
rulepack_id = "bad"
rulepack_version = "0.4.0"
"#,
        )
        .expect_err("unsupported schema");

        assert!(matches!(err, RulepackError::SchemaVersion { .. }));
    }

    #[test]
    fn class_spelling_accepts_pascal_case_and_custom_names() {
        assert_eq!(parse_class("Email").unwrap(), PiiClass::Email);
        assert_eq!(
            parse_class("custom:Order_ID").unwrap(),
            PiiClass::Custom("order_id".to_string())
        );
    }
}
