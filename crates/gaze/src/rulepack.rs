use std::collections::{BTreeSet, HashMap};
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
    pub locale: Option<LocaleData>,
    pub recognizers: Vec<RecognizerSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecognizerSpec {
    pub id: String,
    pub class: PiiClass,
    pub cooperates_with: Vec<String>,
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
    Regex {
        #[serde(default)]
        pattern: Option<String>,
        #[serde(default)]
        pattern_template: Option<String>,
        #[serde(default)]
        capture_groups: Option<Vec<u32>>,
    },
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
    Ner {
        model_ref: String,
    },
    AnchoredMatch {
        cues_bucket: String,
        boundary: String,
        right_window_chars: u16,
        name_shape: String,
        cue_position: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum AnchoredBoundary {
    Punctuation,
    Whitespace,
    LineEnd,
}

/// Closed enum for the shape of token sequences `anchored_match` extracts.
///
/// v0.6 ships a single `PersonName` variant. Future variants
/// (e.g. `Organization`, `Address`, `LegalEntity`) must justify
/// why they aren't a locale-bucket lookup before being added here.
/// Adding variants without that justification regresses the
/// principle drawer `session-2026-04-25-no-codified-domain-concerns`
/// (see also `lower_email_header_pattern_template` in pre-v0.4.1 history).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum NameShape {
    PersonName,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum CuePosition {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextSpec {
    pub hotwords: Vec<String>,
    pub window: Option<u16>,
    pub boost: Option<f32>,
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
    pub family: Option<String>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceSpec {
    pub origin: String,
    pub from: Option<String>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocaleData {
    pub buckets: HashMap<String, LocaleBucket>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleBucket {
    pub names: Vec<String>,
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
    #[error("unsupported matcher kind: {0}")]
    UnsupportedMatcher(String),
    #[error("unsupported anchored_match field '{field}' value '{value}'")]
    UnsupportedAnchoredMatch { field: String, value: String },
    #[error("unsupported rulepack field '{field}' in B1; planned for {planned_version}")]
    UnsupportedField {
        field: String,
        planned_version: &'static str,
    },
    #[error("unsupported validator kind: {kind}")]
    UnsupportedValidator { kind: String },
    #[error("unsupported normalizer kind: {kind}")]
    UnsupportedNormalizer { kind: String },
    #[error("duplicate recognizer id '{id}' in rulepacks '{first_pack}' and '{second_pack}'")]
    DuplicateId {
        id: String,
        first_pack: String,
        second_pack: String,
    },
    #[error("regex recognizer '{id}' must define exactly one of pattern or pattern_template")]
    RegexPatternChoice { id: String },
    #[error("unknown pattern_template placeholder '{placeholder}' in recognizer '{id}'")]
    UnknownPatternTemplatePlaceholder { id: String, placeholder: String },
    #[error(
        "context class_map override for dictionary '{dict}' changes {old_class:?} to {new_class:?}, but {uncovered_rule}"
    )]
    ClassMapOverrideClash {
        dict: String,
        old_class: PiiClass,
        new_class: PiiClass,
        uncovered_rule: String,
    },
    #[error(
        "same-class recognizers '{recognizer_a}' and '{recognizer_b}' both emit {class:?} but neither declares cooperates_with"
    )]
    SameClassWithoutCooperation {
        class: PiiClass,
        recognizer_a: String,
        recognizer_b: String,
    },
}

impl Rulepack {
    pub fn load(source: RulepackSource) -> Result<Rulepack, RulepackError> {
        let raw = match source {
            RulepackSource::Embedded(contents) => contents.to_string(),
            RulepackSource::Path(path) => {
                std::fs::read_to_string(path).map_err(RulepackError::Io)?
            }
        };
        Self::parse(&raw)
    }

    pub fn parse(raw: &str) -> Result<Rulepack, RulepackError> {
        let raw: RawRulepack = toml::from_str(raw).map_err(RulepackError::Toml)?;
        raw.try_into()
    }

    pub fn activated_classes(&self) -> BTreeSet<PiiClass> {
        self.recognizers
            .iter()
            .filter(|recognizer| recognizer.enabled)
            .map(|recognizer| recognizer.class.clone())
            .collect()
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
    locale: Option<RawLocaleData>,
    #[serde(default)]
    recognizers: Vec<RawRecognizerSpec>,
}

#[derive(Debug, Deserialize)]
struct RawLocaleData {
    #[serde(flatten)]
    buckets: HashMap<String, RawLocaleBucket>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocaleBucket {
    names: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRecognizerSpec {
    id: String,
    class: String,
    #[serde(default)]
    cooperates_with: Vec<String>,
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
    #[serde(default)]
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
    window: Option<u16>,
    #[serde(default)]
    boost: Option<f32>,
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

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTokenSpec {
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    format: Option<String>,
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
        recognizer_composition_validator(&recognizers)?;
        let locale = raw.locale.map(LocaleData::from);
        reject_anchored_match_ellipsis_cues(&recognizers, locale.as_ref())?;

        Ok(Self {
            schema_version: raw.schema_version,
            rulepack_id: raw.rulepack_id,
            rulepack_version: raw.rulepack_version,
            default_locales,
            locale,
            recognizers,
        })
    }
}

impl From<RawLocaleData> for LocaleData {
    fn from(raw: RawLocaleData) -> Self {
        Self {
            buckets: raw
                .buckets
                .into_iter()
                .map(|(name, bucket)| {
                    (
                        name,
                        LocaleBucket {
                            names: bucket.names,
                        },
                    )
                })
                .collect(),
        }
    }
}

fn parse_recognizer(
    raw: RawRecognizerSpec,
    default_locales: &[LocaleTag],
) -> Result<RecognizerSpec, RulepackError> {
    reject_unshipped_fields(&raw)?;
    validate_matcher(&raw)?;
    let locales = if raw.locales.is_empty() {
        default_locales.to_vec()
    } else {
        parse_locales(raw.locales)?
    };

    Ok(RecognizerSpec {
        id: raw.id,
        class: parse_class(&raw.class)?,
        cooperates_with: raw.cooperates_with,
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

fn validate_matcher(raw: &RawRecognizerSpec) -> Result<(), RulepackError> {
    match &raw.matcher {
        RawMatch::Regex {
            pattern,
            pattern_template,
            ..
        } => {
            if pattern.is_some() == pattern_template.is_some() {
                return Err(RulepackError::RegexPatternChoice { id: raw.id.clone() });
            }
        }
        RawMatch::AnchoredMatch {
            cues_bucket,
            boundary,
            right_window_chars,
            name_shape,
            cue_position,
            ..
        } => {
            if cues_bucket.trim().is_empty() {
                return Err(RulepackError::UnsupportedAnchoredMatch {
                    field: "cues_bucket".to_string(),
                    value: cues_bucket.clone(),
                });
            }
            if !(1..=512).contains(right_window_chars) {
                return Err(RulepackError::UnsupportedAnchoredMatch {
                    field: "right_window_chars".to_string(),
                    value: right_window_chars.to_string(),
                });
            }
            if !matches!(boundary.as_str(), "punctuation" | "whitespace" | "line_end") {
                return Err(RulepackError::UnsupportedAnchoredMatch {
                    field: "boundary".to_string(),
                    value: boundary.clone(),
                });
            }
            if name_shape != "person_name" {
                return Err(RulepackError::UnsupportedAnchoredMatch {
                    field: "name_shape".to_string(),
                    value: name_shape.clone(),
                });
            }
            if !matches!(cue_position.as_str(), "before" | "after") {
                return Err(RulepackError::UnsupportedAnchoredMatch {
                    field: "cue_position".to_string(),
                    value: cue_position.clone(),
                });
            }
        }
        RawMatch::Dictionary { .. } | RawMatch::Ner { .. } => {}
    }
    Ok(())
}

fn reject_anchored_match_ellipsis_cues(
    recognizers: &[RecognizerSpec],
    locale: Option<&LocaleData>,
) -> Result<(), RulepackError> {
    let Some(locale) = locale else {
        return Ok(());
    };
    for recognizer in recognizers {
        let RawMatch::AnchoredMatch { cues_bucket, .. } = &recognizer.matcher else {
            continue;
        };
        let Some(bucket) = locale.buckets.get(cues_bucket) else {
            continue;
        };
        if let Some(cue) = bucket.names.iter().find(|cue| cue.contains("...")) {
            return Err(RulepackError::UnsupportedAnchoredMatch {
                field: format!("locale.{cues_bucket}.names"),
                value: cue.clone(),
            });
        }
    }
    Ok(())
}

fn reject_unshipped_fields(raw: &RawRecognizerSpec) -> Result<(), RulepackError> {
    const PLANNED_VERSION: &str = "v0.4.1";

    if raw
        .token
        .format
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        return Err(RulepackError::UnsupportedField {
            field: "token.format".to_string(),
            planned_version: PLANNED_VERSION,
        });
    }
    if let Some(context) = &raw.context {
        if !context.hotwords.is_empty() {
            return Err(RulepackError::UnsupportedField {
                field: "context.hotwords".to_string(),
                planned_version: PLANNED_VERSION,
            });
        }
        if context.boost.is_some() {
            return Err(RulepackError::UnsupportedField {
                field: "context.boost".to_string(),
                planned_version: PLANNED_VERSION,
            });
        }
        if context.window.is_some() {
            return Err(RulepackError::UnsupportedField {
                field: "context.window".to_string(),
                planned_version: PLANNED_VERSION,
            });
        }
    }
    Ok(())
}

pub fn recognizer_composition_validator(
    recognizers: &[RecognizerSpec],
) -> Result<(), RulepackError> {
    for (index, first) in recognizers.iter().enumerate() {
        for second in recognizers.iter().skip(index + 1) {
            if first.class != second.class {
                continue;
            }
            if first.cooperates_with.iter().any(|id| id == &second.id)
                || second.cooperates_with.iter().any(|id| id == &first.id)
            {
                continue;
            }
            return Err(RulepackError::SameClassWithoutCooperation {
                class: first.class.clone(),
                recognizer_a: first.id.clone(),
                recognizer_b: second.id.clone(),
            });
        }
    }
    Ok(())
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

[locale.email_headers]
names = ["From", "To", "Cc", "Bcc", "Reply-To", "Sender"]

[[recognizers]]
id = "email.global"
class = "Email"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = '''(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'''

[recognizers.context]
exclusions = ["example.com"]

[recognizers.validator]
kind = "email_rfc"

[recognizers.normalizer]
kind = "email_canonical"

[recognizers.scoring]
base = 0.70
priority = 90

[recognizers.token]

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
        let header_names = &rulepack
            .locale
            .as_ref()
            .and_then(|locale| locale.buckets.get("email_headers"))
            .expect("email headers")
            .names;
        assert_eq!(
            header_names,
            &vec!["From", "To", "Cc", "Bcc", "Reply-To", "Sender"]
        );
        assert_eq!(rulepack.recognizers.len(), 1);
        let recognizer = &rulepack.recognizers[0];
        assert_eq!(recognizer.id, "email.global");
        assert_eq!(recognizer.class, PiiClass::Email);
        assert_eq!(recognizer.scoring.priority, 90);
        assert!(matches!(recognizer.matcher, RawMatch::Regex { .. }));
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn embedded_core_activated_classes_match_rulepack_classes() {
        let rulepack = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded("core").expect("core rulepack"),
        ))
        .expect("embedded core rulepack");

        assert_eq!(
            rulepack.activated_classes(),
            BTreeSet::from([PiiClass::Email, PiiClass::Name])
        );
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn embedded_core_loads_full_name_recognizer_cooperation_matrix() {
        let rulepack = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded("core").expect("core rulepack"),
        ))
        .expect("embedded core rulepack");
        let name_recognizers = rulepack
            .recognizers
            .iter()
            .filter(|recognizer| recognizer.class == PiiClass::Name)
            .collect::<Vec<_>>();

        assert_eq!(name_recognizers.len(), 5);
        for recognizer in &name_recognizers {
            for peer in &name_recognizers {
                if recognizer.id == peer.id {
                    continue;
                }
                assert!(
                    recognizer.cooperates_with.contains(&peer.id),
                    "{} missing cooperates_with {}",
                    recognizer.id,
                    peer.id
                );
            }
        }
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn embedded_core_extended_activated_classes_match_rulepack_classes() {
        let rulepack = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded("core-extended").expect("core-extended rulepack"),
        ))
        .expect("embedded core-extended rulepack");

        assert_eq!(
            rulepack.activated_classes(),
            BTreeSet::from([
                PiiClass::custom("phone"),
                PiiClass::custom("iban"),
                PiiClass::custom("credit_card"),
                PiiClass::custom("ip_address"),
                PiiClass::custom("postal_code"),
            ])
        );
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn activated_classes_include_new_rulepack_recognizer_class() {
        let raw = format!(
            r#"{}

[[recognizers]]
id = "test.only"
class = "custom:test_only"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = "TEST_ONLY"

[recognizers.scoring]
base = 0.70
priority = 1
"#,
            gaze_recognizers::embedded("core-extended").expect("core-extended rulepack")
        );
        let rulepack = Rulepack::parse(&raw).expect("core-extended with synthetic recognizer");

        assert!(
            rulepack
                .activated_classes()
                .contains(&PiiClass::custom("test_only")),
            "new recognizer class must be derived from rulepack data"
        );
    }

    #[test]
    fn rulepack_accepts_token_family() {
        let rulepack = Rulepack::parse(&unsupported_field_rulepack(
            "[recognizers.token]\nfamily = \"email.formatpreserve\"\n",
        ))
        .expect("token family is active in v0.4.1");

        assert_eq!(
            rulepack.recognizers[0].token.family.as_deref(),
            Some("email.formatpreserve")
        );
    }

    #[test]
    fn rulepack_rejects_unsupported_token_format() {
        let err = Rulepack::parse(&unsupported_field_rulepack(
            "[recognizers.token]\nformat = \"Customer_{n}\"\n",
        ))
        .expect_err("token format is reserved for v0.4.1");

        assert_unsupported_field(err, "token.format");
    }

    #[test]
    fn rulepack_rejects_unsupported_context_hotwords() {
        let err = Rulepack::parse(&unsupported_field_rulepack(
            "[recognizers.context]\nhotwords = [\"foo\"]\n",
        ))
        .expect_err("context hotwords are reserved for v0.4.1");

        assert_unsupported_field(err, "context.hotwords");
    }

    #[test]
    fn rulepack_rejects_unsupported_context_boost() {
        let err = Rulepack::parse(&unsupported_field_rulepack(
            "[recognizers.context]\nboost = 0.10\n",
        ))
        .expect_err("context boost is reserved for v0.4.1");

        assert_unsupported_field(err, "context.boost");
    }

    #[test]
    fn rulepack_rejects_unsupported_context_window() {
        let err = Rulepack::parse(&unsupported_field_rulepack(
            "[recognizers.context]\nwindow = 12\n",
        ))
        .expect_err("context window is reserved for v0.4.1");

        assert_unsupported_field(err, "context.window");
    }

    #[test]
    fn rulepack_accepts_default_token_fields() {
        let rulepack = Rulepack::parse(CORE).expect("reserved token/context fields are unset");
        let recognizer = &rulepack.recognizers[0];

        assert_eq!(recognizer.token.family, None);
        assert_eq!(recognizer.token.format, None);
        assert!(recognizer.context.as_ref().unwrap().hotwords.is_empty());
        assert_eq!(recognizer.context.as_ref().unwrap().boost, None);
        assert_eq!(recognizer.context.as_ref().unwrap().window, None);
    }

    #[test]
    fn pattern_template_with_pattern_both_present_fails_closed() {
        let err = Rulepack::parse(&unsupported_field_rulepack(
            "pattern_template = \"{locale_email_headers}: (.+)\"\n",
        ))
        .expect_err("pattern and pattern_template are mutually exclusive");

        assert!(matches!(
            err,
            RulepackError::RegexPatternChoice { id } if id == "bad.email"
        ));
    }

    #[test]
    fn regex_pattern_or_template_is_required() {
        let raw = r#"
schema_version = "0.1.0"
rulepack_id = "bad"
rulepack_version = "0.4.0"
default_locales = ["global"]

[[recognizers]]
id = "bad.email"
class = "Email"
enabled = true

[recognizers.match]
kind = "regex"
"#;
        let err = Rulepack::parse(raw).expect_err("regex pattern is required");

        assert!(matches!(
            err,
            RulepackError::RegexPatternChoice { id } if id == "bad.email"
        ));
    }

    #[test]
    fn anchored_match_accepts_valid_schema() {
        let rulepack = Rulepack::parse(&anchored_match_rulepack("")).expect("anchored_match");
        assert!(matches!(
            rulepack.recognizers[0].matcher,
            RawMatch::AnchoredMatch { .. }
        ));
    }

    #[test]
    fn anchored_match_rejects_unknown_boundary() {
        let err = Rulepack::parse(&anchored_match_rulepack("boundary = \"paragraph\"\n"))
            .expect_err("unknown boundary fails closed");

        assert_unsupported_anchored_match(err, "boundary", "paragraph");
    }

    #[test]
    fn anchored_match_rejects_unknown_name_shape() {
        let err = Rulepack::parse(&anchored_match_rulepack("name_shape = \"organization\"\n"))
            .expect_err("unknown name_shape fails closed");

        assert_unsupported_anchored_match(err, "name_shape", "organization");
    }

    #[test]
    fn anchored_match_rejects_unknown_cue_position() {
        let err = Rulepack::parse(&anchored_match_rulepack("cue_position = \"around\"\n"))
            .expect_err("unknown cue_position fails closed");

        assert_unsupported_anchored_match(err, "cue_position", "around");
    }

    #[test]
    fn anchored_match_rejects_missing_cues_bucket() {
        let err = Rulepack::parse(&anchored_match_rulepack("cues_bucket = \"\"\n"))
            .expect_err("missing cues_bucket fails closed");

        assert_unsupported_anchored_match(err, "cues_bucket", "");
    }

    #[test]
    fn anchored_match_rejects_ellipsis_in_cue_values() {
        let err = Rulepack::parse(
            r#"
schema_version = "0.1.0"
rulepack_id = "anchored"
rulepack_version = "0.6.0"
default_locales = ["global"]

[locale.forward_markers]
names = ["Forwarded ... message"]

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "forward_markers"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"
"#,
        )
        .expect_err("ellipsis cue fails closed");

        assert_unsupported_anchored_match(
            err,
            "locale.forward_markers.names",
            "Forwarded ... message",
        );
    }

    #[test]
    fn anchored_match_rejects_invalid_window_bounds() {
        for (value, expected) in [("0", "0"), ("513", "513")] {
            let err = Rulepack::parse(&anchored_match_rulepack(&format!(
                "right_window_chars = {value}\n"
            )))
            .expect_err("invalid right_window_chars fails closed");

            assert_unsupported_anchored_match(err, "right_window_chars", expected);
        }
    }

    #[test]
    fn rulepack_load_fails_when_two_name_recognizers_omit_cooperates_with() {
        let err = Rulepack::parse(
            r#"
schema_version = "0.1.0"
rulepack_id = "bad-composition"
rulepack_version = "0.4.1"
default_locales = ["global"]

[[recognizers]]
id = "email.header.name"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern = "From: ([A-Z][a-z]+)"

[[recognizers]]
id = "salutation.name"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern = "Dear ([A-Z][a-z]+)"
"#,
        )
        .expect_err("same-class recognizers must explicitly cooperate");

        assert!(matches!(
            err,
            RulepackError::SameClassWithoutCooperation {
                class: PiiClass::Name,
                recognizer_a,
                recognizer_b,
            } if recognizer_a == "email.header.name" && recognizer_b == "salutation.name"
        ));
    }

    #[test]
    fn rulepack_load_accepts_same_class_pair_with_cooperates_with() {
        let rulepack = Rulepack::parse(
            r#"
schema_version = "0.1.0"
rulepack_id = "cooperating-composition"
rulepack_version = "0.4.1"
default_locales = ["global"]

[[recognizers]]
id = "email.header.name"
class = "Name"
cooperates_with = ["salutation.name"]
enabled = true

[recognizers.match]
kind = "regex"
pattern = "From: ([A-Z][a-z]+)"

[[recognizers]]
id = "salutation.name"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern = "Dear ([A-Z][a-z]+)"
"#,
        )
        .expect("cooperates_with unblocks same-class recognizers");

        assert_eq!(rulepack.recognizers.len(), 2);
        assert_eq!(
            rulepack.recognizers[0].cooperates_with,
            vec!["salutation.name"]
        );
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
            parse_class("custom:Class_Alpha").unwrap(),
            PiiClass::Custom("class_alpha".to_string())
        );
    }

    fn unsupported_field_rulepack(extra: &str) -> String {
        format!(
            r#"
schema_version = "0.1.0"
rulepack_id = "bad"
rulepack_version = "0.4.0"
default_locales = ["global"]

[[recognizers]]
id = "bad.email"
class = "Email"
enabled = true

[recognizers.match]
kind = "regex"
pattern = ".+"

{extra}
"#
        )
    }

    fn anchored_match_rulepack(override_line: &str) -> String {
        let cues_bucket = if override_line.starts_with("cues_bucket") {
            override_line.to_string()
        } else {
            "cues_bucket = \"forward_markers\"\n".to_string()
        };
        let boundary = if override_line.starts_with("boundary") {
            override_line.to_string()
        } else {
            "boundary = \"punctuation\"\n".to_string()
        };
        let right_window_chars = if override_line.starts_with("right_window_chars") {
            override_line.to_string()
        } else {
            "right_window_chars = 64\n".to_string()
        };
        let name_shape = if override_line.starts_with("name_shape") {
            override_line.to_string()
        } else {
            "name_shape = \"person_name\"\n".to_string()
        };
        let cue_position = if override_line.starts_with("cue_position") {
            override_line.to_string()
        } else {
            "cue_position = \"before\"\n".to_string()
        };
        format!(
            r#"
schema_version = "0.1.0"
rulepack_id = "anchored"
rulepack_version = "0.6.0"
default_locales = ["global"]

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
{cues_bucket}{boundary}{right_window_chars}{name_shape}{cue_position}
"#
        )
    }

    fn assert_unsupported_field(err: RulepackError, field: &str) {
        assert!(matches!(
            err,
            RulepackError::UnsupportedField {
                field: ref actual,
                planned_version: "v0.4.1",
            } if actual == field
        ));
    }

    fn assert_unsupported_anchored_match(err: RulepackError, field: &str, value: &str) {
        assert!(matches!(
            err,
            RulepackError::UnsupportedAnchoredMatch {
                field: ref actual_field,
                value: ref actual_value,
            } if actual_field == field && actual_value == value
        ));
    }
}
