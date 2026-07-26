use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use regex::Regex;
use serde::Deserialize;
use thiserror::Error;

use crate::{CollisionMembership, LocaleBasis, LocaleTag, PiiClass, SafetyTier};

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
#[non_exhaustive]
pub struct RecognizerSpec {
    pub id: String,
    pub class: PiiClass,
    pub cooperates_with: Vec<String>,
    pub collision: Option<CollisionMembership>,
    pub enabled: bool,
    pub safety_tier: SafetyTier,
    pub locales: Vec<LocaleTag>,
    pub locale_basis: LocaleBasis,
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
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
pub enum NameShape {
    PersonName,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
#[non_exhaustive]
pub enum CuePosition {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
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
#[non_exhaustive]
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
    pub cues: HashMap<String, LocaleCueBundle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleBucket {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LocaleCueBundle {
    pub names: Vec<String>,
    pub window_chars: Option<u16>,
}

impl LocaleCueBundle {
    /// Builds a locale cue bundle used by mandatory-anchor resolution.
    pub fn new(names: Vec<String>, window_chars: Option<u16>) -> Self {
        Self {
            names,
            window_chars,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RulepackSource {
    Embedded(&'static str),
    Path(PathBuf),
}

#[derive(Debug, Error)]
#[non_exhaustive]
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
    #[error("unsupported locale_basis: {value}")]
    UnsupportedLocaleBasis { value: String },
    #[error("bundled recognizer '{recognizer_id}' must declare locale_basis explicitly")]
    MissingBundledLocaleBasis { recognizer_id: String },
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
    #[error("unsupported safety_tier: {value}")]
    UnsupportedSafetyTier { value: String },
    #[error("unsupported rule spec variant: {variant}")]
    UnsupportedRuleSpec { variant: String },
    #[error("duplicate recognizer id '{id}' in rulepacks '{first_pack}' and '{second_pack}'")]
    DuplicateId {
        id: String,
        first_pack: String,
        second_pack: String,
    },
    #[error("regex recognizer '{id}' must define exactly one of pattern or pattern_template")]
    RegexPatternChoice { id: String },
    #[error("invalid regex for recognizer '{id}': {source}")]
    RegexCompile {
        id: String,
        #[source]
        source: regex::Error,
    },
    #[error(
        "regex recognizer '{id}' shadows Gaze token shape sample '{shadowed_shape}' with pattern '{pattern}'"
    )]
    TokenShapeShadow {
        id: String,
        pattern: String,
        shadowed_shape: String,
    },
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
    #[error(
        "recognizers {recognizer_ids:?} share class {class:?} with equivalent regex shape and overlapping locale projection {locale_overlap:?}"
    )]
    ConflictingLocaleProjection {
        class: PiiClass,
        recognizer_ids: Vec<String>,
        locale_overlap: Vec<LocaleTag>,
    },
    #[error("recognizer '{recognizer_id}' has invalid collision {field} '{value}'")]
    InvalidCollisionName {
        recognizer_id: String,
        field: &'static str,
        value: String,
    },
    #[error(
        "recognizers '{first_recognizer_id}' and '{second_recognizer_id}' declare collision family '{family}' variant '{variant}' with inconsistent precedence {first_precedence} and {second_precedence}"
    )]
    InconsistentCollisionVariant {
        family: String,
        variant: String,
        first_recognizer_id: String,
        first_precedence: u32,
        second_recognizer_id: String,
        second_precedence: u32,
    },
    #[error(
        "collision family '{family}' variants '{first_variant}' and '{second_variant}' share ambiguous precedence {precedence}"
    )]
    AmbiguousFamilyPrecedence {
        family: String,
        first_variant: String,
        second_variant: String,
        precedence: u32,
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
        let (raw, lint) = extract_recognizer_lint_config(raw);
        let raw: RawRulepack = toml::from_str(&raw).map_err(RulepackError::Toml)?;
        RawRulepackWithLint {
            raw,
            lint,
            require_explicit_locale_basis: false,
        }
        .try_into()
    }

    /// Parses an official bundled rulepack and rejects ambiguous locale-basis omission.
    ///
    /// Adopter rulepacks should use [`Self::parse`], whose legacy default remains
    /// [`LocaleBasis::Document`].
    pub fn parse_bundled(raw: &str) -> Result<Rulepack, RulepackError> {
        let (raw, lint) = extract_recognizer_lint_config(raw);
        let raw: RawRulepack = toml::from_str(&raw).map_err(RulepackError::Toml)?;
        RawRulepackWithLint {
            raw,
            lint,
            require_explicit_locale_basis: true,
        }
        .try_into()
    }

    pub fn activated_classes(&self) -> BTreeSet<PiiClass> {
        let mut classes = BTreeSet::new();
        for recognizer in self
            .recognizers
            .iter()
            .filter(|recognizer| recognizer.enabled)
        {
            classes.insert(recognizer.class.clone());
            if let Some(family_class) = recognizer
                .collision
                .as_ref()
                .and_then(|collision| {
                    collision
                        .mandatory_anchor
                        .as_ref()
                        .map(|_| &collision.family)
                })
                .map(|family| PiiClass::Custom(format!("family:{family}")))
            {
                classes.insert(family_class);
            }
        }
        classes
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

#[derive(Debug, Default)]
struct RawRecognizerLintConfig {
    strict_locale_overlap: bool,
}

#[derive(Debug)]
struct RawRulepackWithLint {
    raw: RawRulepack,
    lint: RawRecognizerLintConfig,
    require_explicit_locale_basis: bool,
}

#[derive(Debug, Deserialize)]
struct RawLocaleData {
    #[serde(default)]
    cues: HashMap<String, RawLocaleCueBundle>,
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
struct RawLocaleCueBundle {
    names: Vec<String>,
    #[serde(default)]
    window_chars: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRecognizerSpec {
    id: String,
    class: String,
    #[serde(default)]
    cooperates_with: Vec<String>,
    #[serde(default)]
    collision: Option<RawCollisionSpec>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    safety_tier: Option<String>,
    #[serde(default)]
    locales: Vec<String>,
    #[serde(default)]
    locale_basis: Option<String>,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCollisionSpec {
    pub(crate) family: String,
    pub(crate) variant: String,
    #[serde(default = "default_collision_precedence")]
    pub(crate) precedence: u32,
    #[serde(default)]
    pub(crate) mandatory_anchor: Option<String>,
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
        RawRulepackWithLint {
            raw,
            lint: RawRecognizerLintConfig::default(),
            require_explicit_locale_basis: false,
        }
        .try_into()
    }
}

impl TryFrom<RawRulepackWithLint> for Rulepack {
    type Error = RulepackError;

    fn try_from(raw_with_lint: RawRulepackWithLint) -> Result<Self, Self::Error> {
        let raw = raw_with_lint.raw;
        if !raw.schema_version.starts_with(SUPPORTED_SCHEMA_MAJOR_MINOR) {
            return Err(RulepackError::SchemaVersion {
                found: raw.schema_version,
                supported: "~0.1.x".to_string(),
            });
        }

        if raw_with_lint.require_explicit_locale_basis {
            if let Some(recognizer) = raw
                .recognizers
                .iter()
                .find(|recognizer| recognizer.locale_basis.is_none())
            {
                return Err(RulepackError::MissingBundledLocaleBasis {
                    recognizer_id: recognizer.id.clone(),
                });
            }
        }

        let default_locales = parse_locales(raw.default_locales)?;
        let mut recognizers = raw
            .recognizers
            .into_iter()
            .map(|recognizer| parse_recognizer(recognizer, &default_locales))
            .collect::<Result<Vec<_>, _>>()?;
        validate_collision_memberships(&recognizers)?;
        apply_collision_family_cooperation(&mut recognizers);
        validate_rulepack_recognizers(&recognizers, &default_locales, &raw_with_lint.lint)?;
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

fn extract_recognizer_lint_config(raw: &str) -> (String, RawRecognizerLintConfig) {
    let mut sanitized = String::with_capacity(raw.len());
    let mut lint = RawRecognizerLintConfig::default();
    let mut in_lint = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed == "[recognizers.lint]" {
            in_lint = true;
            continue;
        }
        if in_lint && trimmed.starts_with('[') {
            in_lint = false;
        }
        if in_lint {
            if let Some((key, value)) = trimmed.split_once('=') {
                if key.trim() == "strict_locale_overlap" {
                    lint.strict_locale_overlap = value.trim().eq_ignore_ascii_case("true");
                }
            }
            continue;
        }
        sanitized.push_str(line);
        sanitized.push('\n');
    }

    (sanitized, lint)
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
            cues: raw
                .cues
                .into_iter()
                .map(|(name, bundle)| {
                    (
                        name,
                        LocaleCueBundle::new(bundle.names, bundle.window_chars),
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
    let collision = raw
        .collision
        .map(|collision| parse_collision_membership(&raw.id, collision))
        .transpose()?;
    let safety_tier = raw
        .safety_tier
        .as_deref()
        .map(SafetyTier::parse)
        .transpose()
        .map_err(|err| RulepackError::UnsupportedSafetyTier {
            value: err.value().to_string(),
        })?
        .unwrap_or_default();
    let locale_basis = raw
        .locale_basis
        .as_deref()
        .map(LocaleBasis::parse)
        .transpose()
        .map_err(|err| RulepackError::UnsupportedLocaleBasis {
            value: err.value().to_string(),
        })?
        .unwrap_or_default();

    Ok(RecognizerSpec {
        id: raw.id,
        class: parse_class(&raw.class)?,
        cooperates_with: raw.cooperates_with,
        collision,
        enabled: raw.enabled,
        safety_tier,
        locales,
        locale_basis,
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

pub(crate) fn parse_collision_membership(
    recognizer_id: &str,
    raw: RawCollisionSpec,
) -> Result<CollisionMembership, RulepackError> {
    validate_collision_name(recognizer_id, "family", &raw.family)?;
    validate_collision_name(recognizer_id, "variant", &raw.variant)?;
    Ok(CollisionMembership::new(
        raw.family,
        raw.variant,
        raw.precedence,
        raw.mandatory_anchor,
    ))
}

fn validate_collision_name(
    recognizer_id: &str,
    field: &'static str,
    value: &str,
) -> Result<(), RulepackError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(RulepackError::InvalidCollisionName {
            recognizer_id: recognizer_id.to_string(),
            field,
            value: value.to_string(),
        })
    }
}

fn validate_collision_memberships(recognizers: &[RecognizerSpec]) -> Result<(), RulepackError> {
    let mut variant_precedence: HashMap<(&str, &str), (&str, u32)> = HashMap::new();
    let mut family_precedence: HashMap<(&str, u32), &str> = HashMap::new();

    for recognizer in recognizers {
        let Some(collision) = &recognizer.collision else {
            continue;
        };

        if let Some((first_id, first_precedence)) =
            variant_precedence.get(&(collision.family.as_str(), collision.variant.as_str()))
        {
            if *first_precedence != collision.precedence {
                return Err(RulepackError::InconsistentCollisionVariant {
                    family: collision.family.clone(),
                    variant: collision.variant.clone(),
                    first_recognizer_id: (*first_id).to_string(),
                    first_precedence: *first_precedence,
                    second_recognizer_id: recognizer.id.clone(),
                    second_precedence: collision.precedence,
                });
            }
        } else {
            variant_precedence.insert(
                (collision.family.as_str(), collision.variant.as_str()),
                (recognizer.id.as_str(), collision.precedence),
            );
        }

        if let Some(first_variant) =
            family_precedence.get(&(collision.family.as_str(), collision.precedence))
        {
            if *first_variant != collision.variant {
                return Err(RulepackError::AmbiguousFamilyPrecedence {
                    family: collision.family.clone(),
                    first_variant: (*first_variant).to_string(),
                    second_variant: collision.variant.clone(),
                    precedence: collision.precedence,
                });
            }
        } else {
            family_precedence.insert(
                (collision.family.as_str(), collision.precedence),
                collision.variant.as_str(),
            );
        }
    }

    Ok(())
}

fn apply_collision_family_cooperation(recognizers: &mut [RecognizerSpec]) {
    let mut family_members: HashMap<String, Vec<String>> = HashMap::new();
    for recognizer in recognizers.iter() {
        let Some(collision) = &recognizer.collision else {
            continue;
        };
        family_members
            .entry(collision.family.clone())
            .or_default()
            .push(recognizer.id.clone());
    }

    for recognizer in recognizers {
        let Some(collision) = &recognizer.collision else {
            continue;
        };
        let Some(siblings) = family_members.get(&collision.family) else {
            continue;
        };
        for sibling in siblings {
            if sibling != &recognizer.id && !recognizer.cooperates_with.contains(sibling) {
                recognizer.cooperates_with.push(sibling.clone());
            }
        }
    }
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
            if let Some(pattern) = pattern {
                let compiled =
                    Regex::new(pattern).map_err(|source| RulepackError::RegexCompile {
                        id: raw.id.clone(),
                        source,
                    })?;
                crate::token_shape::reject_if_shadows_token_shape(&compiled, &raw.id).map_err(
                    |shadow| RulepackError::TokenShapeShadow {
                        id: shadow.recognizer_id,
                        pattern: shadow.offending_pattern,
                        shadowed_shape: shadow.shadowed_shape,
                    },
                )?;
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

fn validate_rulepack_recognizers(
    recognizers: &[RecognizerSpec],
    active_locales: &[LocaleTag],
    lint: &RawRecognizerLintConfig,
) -> Result<(), RulepackError> {
    recognizer_composition_validator(recognizers)?;
    lint_locale_projection_collisions(recognizers, active_locales, lint)?;
    lint_global_naked_patterns(recognizers);
    Ok(())
}

fn lint_locale_projection_collisions(
    recognizers: &[RecognizerSpec],
    active_locales: &[LocaleTag],
    lint: &RawRecognizerLintConfig,
) -> Result<(), RulepackError> {
    for (index, first) in recognizers.iter().enumerate() {
        if !first.enabled {
            continue;
        }
        let Some(first_shape) = regex_structural_shape(&first.matcher) else {
            continue;
        };
        if !is_truly_naked_numeric(&first.matcher) {
            continue;
        }
        let first_projection = locale_projection(first, active_locales);
        if first_projection.is_empty() {
            continue;
        }

        for second in recognizers.iter().skip(index + 1) {
            if !second.enabled || first.class != second.class {
                continue;
            }
            if !is_truly_naked_numeric(&second.matcher) {
                continue;
            }
            if regex_structural_shape(&second.matcher).as_ref() != Some(&first_shape) {
                continue;
            }
            let second_projection = locale_projection(second, active_locales);
            if second_projection.is_empty() {
                continue;
            }

            let recognizer_ids = vec![first.id.clone(), second.id.clone()];
            let locale_overlap = merged_locale_projection(&first_projection, &second_projection);
            if lint.strict_locale_overlap {
                return Err(RulepackError::ConflictingLocaleProjection {
                    class: first.class.clone(),
                    recognizer_ids,
                    locale_overlap,
                });
            }
            tracing::warn!(
                class = %first.class.class_name(),
                recognizer_ids = ?recognizer_ids,
                locale_overlap = ?locale_overlap,
                "recognizers share class with naked-shape regex and non-disjoint locale projection"
            );
        }
    }
    Ok(())
}

fn lint_global_naked_patterns(recognizers: &[RecognizerSpec]) {
    for recognizer in recognizers {
        if !recognizer.enabled
            || (recognizer.locale_basis != LocaleBasis::Format
                && recognizer.locales != [LocaleTag::Global])
        {
            continue;
        }
        let Some(shape) = regex_structural_shape(&recognizer.matcher) else {
            continue;
        };
        let RawMatch::Regex {
            pattern: Some(pattern),
            ..
        } = &recognizer.matcher
        else {
            continue;
        };
        if shape.minimum_match_len < 6 && !has_regex_separator(pattern) {
            tracing::warn!(
                recognizer_id = %recognizer.id,
                class = %recognizer.class.class_name(),
                minimum_match_len = shape.minimum_match_len,
                "global recognizer uses short naked regex shape"
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegexStructuralShape {
    minimum_match_len: usize,
    character_class: RegexCharacterClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RegexCharacterClass {
    Digit,
}

fn regex_structural_shape(matcher: &RawMatch) -> Option<RegexStructuralShape> {
    let RawMatch::Regex {
        pattern: Some(pattern),
        pattern_template: None,
        ..
    } = matcher
    else {
        return None;
    };
    if has_unescaped_line_anchor(pattern) {
        return None;
    }
    digit_quantifier_minimum(pattern).map(|minimum_match_len| RegexStructuralShape {
        minimum_match_len,
        character_class: RegexCharacterClass::Digit,
    })
}

fn is_truly_naked_numeric(matcher: &RawMatch) -> bool {
    let RawMatch::Regex {
        pattern: Some(pattern),
        ..
    } = matcher
    else {
        return false;
    };

    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            chars.next();
            continue;
        }
        if ch.is_ascii_alphabetic() {
            return false;
        }
    }
    true
}

fn has_unescaped_line_anchor(pattern: &str) -> bool {
    let mut escaped = false;
    let mut in_class = false;
    for ch in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => in_class = true,
            ']' => in_class = false,
            '^' | '$' if !in_class => return true,
            _ => {}
        }
    }
    false
}

fn digit_quantifier_minimum(pattern: &str) -> Option<usize> {
    find_digit_quantifier(pattern, r"\d{")
        .or_else(|| find_digit_quantifier(pattern, "[0-9]{"))
        .or_else(|| find_digit_quantifier(pattern, "[[:digit:]]{"))
}

fn find_digit_quantifier(pattern: &str, needle: &str) -> Option<usize> {
    let start = pattern.find(needle)? + needle.len();
    let rest = &pattern[start..];
    let digits = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn locale_projection(recognizer: &RecognizerSpec, active_locales: &[LocaleTag]) -> Vec<LocaleTag> {
    if recognizer.locale_basis == LocaleBasis::Format {
        return vec![LocaleTag::Global];
    }

    let mut projection = Vec::new();
    for locale in &recognizer.locales {
        if *locale == LocaleTag::Global {
            projection.push(LocaleTag::Global);
        } else if active_locales.iter().any(|active| active == locale) {
            projection.push(locale.clone());
        }
    }
    projection
}

fn merged_locale_projection(left: &[LocaleTag], right: &[LocaleTag]) -> Vec<LocaleTag> {
    let mut merged = Vec::new();
    for locale in left.iter().chain(right) {
        if !merged.iter().any(|existing| existing == locale) {
            merged.push(locale.clone());
        }
    }
    merged
}

fn has_regex_separator(pattern: &str) -> bool {
    pattern.contains('-')
        || pattern.contains('/')
        || pattern.contains('.')
        || pattern.contains('+')
        || pattern.contains("\\s")
        || pattern.contains("[:space:]")
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
            if name.starts_with("family:") {
                Ok(PiiClass::Custom(name.to_string()))
            } else {
                Ok(PiiClass::custom(name))
            }
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

fn default_collision_precedence() -> u32 {
    100
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
pattern = '''(?i)(?-u:\b)[a-z0-9_][a-z0-9._%+\-]*@(?:(?:[a-z0-9\-]+\.)*example\.invalid|test\.local|(?:[a-z0-9](?:[a-z0-9\-]*[a-z0-9])?\.)+(?:[a-z]{2,3}|[a-su-z][a-z]{3}|t[a-df-z][a-z]{2}|te[a-rt-z][a-z]|tes[a-su-z]|[a-z]{5,6}|[a-hj-z][a-z]{6}|i[a-mo-z][a-z]{5}|in[a-uw-z][a-z]{4}|inv[b-z][a-z]{3}|inva[a-km-z][a-z]{2}|inval[a-hj-z][a-z]|invali[a-ce-z]|[a-z]{8,}))(?-u:\b)'''

[recognizers.context]
exclusions = ["test.local"]

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
        assert_eq!(recognizer.locale_basis, LocaleBasis::Document);
        assert_eq!(recognizer.scoring.priority, 90);
        assert!(matches!(recognizer.matcher, RawMatch::Regex { .. }));
    }

    #[test]
    fn rulepack_parses_explicit_format_locale_basis() {
        let raw = CORE.replace(
            "id = \"email.global\"\nclass = \"Email\"\nenabled = true\nlocales = [\"global\"]",
            "id = \"email.global\"\nclass = \"Email\"\nenabled = true\nlocales = [\"global\"]\nlocale_basis = \"format\"",
        );
        let rulepack = Rulepack::parse(&raw).expect("format locale basis");

        assert_eq!(rulepack.recognizers[0].locale_basis, LocaleBasis::Format);
    }

    #[test]
    fn rulepack_rejects_unknown_locale_basis() {
        let raw = CORE.replace(
            "id = \"email.global\"\nclass = \"Email\"\nenabled = true\nlocales = [\"global\"]",
            "id = \"email.global\"\nclass = \"Email\"\nenabled = true\nlocales = [\"global\"]\nlocale_basis = \"request\"",
        );
        let err = Rulepack::parse(&raw).expect_err("locale basis is a closed enum");

        assert!(matches!(
            err,
            RulepackError::UnsupportedLocaleBasis { value } if value == "request"
        ));
    }

    #[test]
    fn bundled_rulepack_rejects_omitted_locale_basis() {
        let err = Rulepack::parse_bundled(CORE)
            .expect_err("official bundles must declare locale basis explicitly");

        assert!(matches!(
            err,
            RulepackError::MissingBundledLocaleBasis { recognizer_id }
                if recognizer_id == "email.global"
        ));
    }

    #[test]
    fn parses_nested_locale_cue_bundles() {
        let rulepack = Rulepack::parse(
            r#"
schema_version = "0.1.0"
rulepack_id = "locale-cues"
rulepack_version = "0.7.0"
default_locales = ["en-US"]

[locale.forward_markers]
names = ["Forwarded message"]

[locale.cues.iban]
names = ["IBAN:", "Account"]
window_chars = 48
"#,
        )
        .expect("locale cues rulepack");

        let locale = rulepack.locale.expect("locale data");
        assert_eq!(
            locale.buckets["forward_markers"].names,
            vec!["Forwarded message"]
        );
        assert_eq!(locale.cues["iban"].names, vec!["IBAN:", "Account"]);
        assert_eq!(locale.cues["iban"].window_chars, Some(48));
    }

    #[test]
    fn locale_cue_bundle_constructor_builds_public_type() {
        let bundle = LocaleCueBundle::new(vec!["IBAN".to_string()], Some(64));

        assert_eq!(bundle.names, vec!["IBAN"]);
        assert_eq!(bundle.window_chars, Some(64));
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
            BTreeSet::from([
                PiiClass::Email,
                PiiClass::Name,
                PiiClass::custom("phone"),
                PiiClass::custom("iban"),
                PiiClass::Custom("family:payment-card-or-iban".to_string()),
                PiiClass::custom("credit_card"),
                PiiClass::custom("ip_address"),
                PiiClass::custom("eth_address"),
                PiiClass::custom("url"),
                PiiClass::custom("security_token"),
                PiiClass::custom("aadhaar"),
                PiiClass::custom("nir"),
                PiiClass::custom("steuer_id"),
                PiiClass::custom("vat_id"),
                PiiClass::custom("bsn"),
                PiiClass::custom("cpf"),
                PiiClass::custom("cnpj"),
                PiiClass::custom("nhs_number"),
                PiiClass::custom("ssn"),
                PiiClass::custom("nino"),
                PiiClass::custom("pan"),
                PiiClass::custom("postal_code"),
            ])
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
    fn embedded_core_extended_alias_activated_classes_match_core() {
        let rulepack = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded("core-extended").expect("core-extended rulepack"),
        ))
        .expect("embedded core-extended rulepack");
        let core = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded("core").expect("core rulepack"),
        ))
        .expect("embedded core rulepack");

        assert_eq!(rulepack.activated_classes(), core.activated_classes());
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
    fn rulepack_accepts_collision_membership_and_extends_family_cooperation() {
        let rulepack = Rulepack::parse(&collision_rulepack(
            r#"
[recognizers.collision]
family = "tenant-document"
variant = "alpha"
precedence = 10
"#,
            r#"
[recognizers.collision]
family = "tenant-document"
variant = "beta"
precedence = 20
"#,
        ))
        .expect("collision membership should parse");

        assert_eq!(
            rulepack.recognizers[0].collision,
            Some(CollisionMembership::new(
                "tenant-document",
                "alpha",
                10,
                None,
            ))
        );
        assert_eq!(rulepack.recognizers[0].cooperates_with, vec!["tenant.beta"]);
        assert_eq!(
            rulepack.recognizers[1].cooperates_with,
            vec!["tenant.alpha"]
        );
    }

    #[test]
    fn rulepack_rejects_inconsistent_collision_variant_precedence() {
        let err = Rulepack::parse(&collision_rulepack(
            r#"
[recognizers.collision]
family = "tenant-document"
variant = "alpha"
precedence = 10
"#,
            r#"
[recognizers.collision]
family = "tenant-document"
variant = "alpha"
precedence = 20
"#,
        ))
        .expect_err("same variant must use one precedence");

        assert!(matches!(
            err,
            RulepackError::InconsistentCollisionVariant {
                family,
                variant,
                first_precedence: 10,
                second_precedence: 20,
                ..
            } if family == "tenant-document" && variant == "alpha"
        ));
    }

    #[test]
    fn rulepack_rejects_ambiguous_family_precedence() {
        let err = Rulepack::parse(&collision_rulepack(
            r#"
[recognizers.collision]
family = "tenant-document"
variant = "alpha"
precedence = 10
"#,
            r#"
[recognizers.collision]
family = "tenant-document"
variant = "beta"
precedence = 10
"#,
        ))
        .expect_err("distinct variants cannot share precedence");

        assert!(matches!(
            err,
            RulepackError::AmbiguousFamilyPrecedence {
                family,
                first_variant,
                second_variant,
                precedence: 10,
            } if family == "tenant-document"
                && first_variant == "alpha"
                && second_variant == "beta"
        ));
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
    fn rulepack_load_accepts_fixture_email_regex() {
        let raw = r#"
schema_version = "0.1.0"
rulepack_id = "custom-email"
rulepack_version = "0.7.0"
default_locales = ["global"]

[[recognizers]]
id = "custom.email"
class = "Email"
enabled = true

[recognizers.match]
kind = "regex"
pattern = '''alice@example\.invalid'''
"#;

        let rulepack = Rulepack::parse(raw).expect("standard email regex should load");

        assert_eq!(rulepack.recognizers.len(), 1);
        assert_eq!(rulepack.recognizers[0].id, "custom.email");
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
pattern = "BAD_EMAIL_FIXTURE"

{extra}
"#
        )
    }

    fn collision_rulepack(first_collision: &str, second_collision: &str) -> String {
        format!(
            r#"
schema_version = "0.1.0"
rulepack_id = "collision-test"
rulepack_version = "0.7.0"
default_locales = ["global"]

[[recognizers]]
id = "tenant.alpha"
class = "custom:tenant_alpha"
enabled = true

[recognizers.match]
kind = "regex"
pattern = "ALPHA-[0-9]+"

{first_collision}

[[recognizers]]
id = "tenant.beta"
class = "custom:tenant_beta"
enabled = true

[recognizers.match]
kind = "regex"
pattern = "BETA-[0-9]+"

{second_collision}
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
