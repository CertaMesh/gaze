use std::sync::{Arc, OnceLock};

use gaze::{
    Action, ClassRule, DefaultRule, LocaleChain, LocaleTag, PiiClass, Pipeline, Policy,
    PolicyError, RawMatch, RedactionEntry, RedactionLogger, Result as GazeResult, Rulepack,
    RulepackDict, RulepackSource, TypedContext, DEFAULT_NER_THRESHOLD,
};
use gaze_recognizers::{DictionaryRecognizer, RegexDetector};

use crate::error::CliError;

pub(crate) fn map_policy_error(err: PolicyError) -> CliError {
    match err {
        PolicyError::Io(_) => CliError::PolicyOpen,
        PolicyError::UnsupportedRuleKind(_) => {
            CliError::PolicyConfigDetail("column rules not supported in CLI mode".to_string())
        }
        _ => CliError::PolicyConfig,
    }
}

pub(crate) fn map_pipeline_error(err: gaze::Error) -> CliError {
    match err {
        gaze::Error::Policy(policy_err) => map_policy_error(policy_err),
        gaze::Error::Rulepack(_) => CliError::PolicyConfig,
        _ => CliError::Pipeline,
    }
}

pub(crate) fn build_pipeline_from_policy(
    policy: &Policy,
    rulepacks: &[Rulepack],
    context: Option<&TypedContext>,
    locale_chain: &LocaleChain,
    ner_threshold: f32,
) -> GazeResult<Pipeline> {
    let empty_context = TypedContext {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    };
    gaze_assembly::build_pipeline(
        policy,
        context.unwrap_or(&empty_context),
        rulepacks,
        locale_chain,
        Some(ner_threshold),
    )
    .map_err(|err| match err {
        gaze_assembly::BuildError::Policy(err) => gaze::Error::Policy(err),
        gaze_assembly::BuildError::Rulepack(err) => gaze::Error::Rulepack(err),
        gaze_assembly::BuildError::Pipeline(err) => err,
        gaze_assembly::BuildError::UnknownLocaleBucket { bucket, .. } => {
            gaze::Error::Policy(PolicyError::UnknownLocaleBucket { name: bucket })
        }
    })
}

pub(crate) fn validate_ner_threshold(threshold: f32) -> std::result::Result<f32, PolicyError> {
    if (0.0..=1.0).contains(&threshold) {
        Ok(threshold)
    } else {
        Err(PolicyError::NerThresholdOutOfRange { value: threshold })
    }
}

pub(crate) fn resolve_ner_threshold(cli_threshold: Option<f32>, policy: Option<&Policy>) -> f32 {
    cli_threshold
        .or_else(|| policy.and_then(|policy| policy.ner.as_ref().map(|ner| ner.threshold)))
        .unwrap_or(DEFAULT_NER_THRESHOLD)
}

pub(crate) fn load_rulepacks(policy: &Policy) -> GazeResult<Vec<Rulepack>> {
    let mut rulepacks = Vec::new();
    for bundled in &policy.rulepacks.bundled {
        let contents = load_embedded_rulepack_contents(bundled)?;
        rulepacks.push(Rulepack::load(RulepackSource::Embedded(contents))?);
    }
    for path in &policy.rulepacks.paths {
        rulepacks.push(Rulepack::load(RulepackSource::Path(path.clone()))?);
    }
    Ok(rulepacks)
}

fn load_embedded_rulepack_contents(id: &str) -> GazeResult<&'static str> {
    gaze_recognizers::embedded(id).ok_or_else(|| {
        gaze::Error::Policy(PolicyError::BundledRulepackUnknown {
            value: id.to_string(),
        })
    })
}

pub(crate) fn dictionary_terms_from_rulepacks(
    rulepacks: &[Rulepack],
) -> GazeResult<Vec<RulepackDict>> {
    let mut dictionaries = Vec::new();
    for rulepack in rulepacks {
        for recognizer in &rulepack.recognizers {
            let RawMatch::Dictionary {
                terms,
                terms_file,
                terms_from_context,
                case_sensitive,
            } = &recognizer.matcher
            else {
                continue;
            };
            if terms_from_context.is_some() {
                continue;
            }
            let mut all_terms = terms.clone();
            if let Some(path) = terms_file {
                let file = std::fs::read_to_string(path).map_err(|err| {
                    gaze::Error::Policy(PolicyError::BadDictionary {
                        name: recognizer.id.clone(),
                        reason: format!("failed to read terms_file: {err}"),
                    })
                })?;
                all_terms.extend(
                    file.lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .map(str::to_string),
                );
            }
            if all_terms.is_empty() {
                return Err(gaze::Error::Policy(PolicyError::BadDictionary {
                    name: recognizer.id.clone(),
                    reason: "dictionary matcher requires terms, terms_file, or terms_from_context"
                        .to_string(),
                }));
            }
            if !case_sensitive && all_terms.iter().any(|term| !term.is_ascii()) {
                return Err(gaze::Error::Policy(PolicyError::BadDictionary {
                    name: recognizer.id.clone(),
                    reason:
                        "unicode dictionary insensitive matching unsupported in v0.4.0, use case_sensitive = true"
                            .to_string(),
                }));
            }
            dictionaries.push(RulepackDict {
                name: recognizer.id.clone(),
                terms: all_terms,
                case_sensitive: *case_sensitive,
            });
        }
    }
    Ok(dictionaries)
}

pub(crate) fn empty_fields() -> &'static serde_json::Map<String, serde_json::Value> {
    static EMPTY_FIELDS: OnceLock<serde_json::Map<String, serde_json::Value>> = OnceLock::new();
    EMPTY_FIELDS.get_or_init(serde_json::Map::new)
}

pub(crate) fn merged_rulepack_default_locales(rulepacks: &[Rulepack]) -> Vec<LocaleTag> {
    let mut locales = Vec::new();
    for rulepack in rulepacks {
        for locale in &rulepack.default_locales {
            if !locales.iter().any(|existing| existing == locale) {
                locales.push(locale.clone());
            }
        }
    }
    locales
}

/// Stub pipeline used until the policy.toml loader (solo #3) lands.
/// Ships only a regex email detector + tokenize rule so the CLI contract can
/// be exercised end-to-end; richer detectors arrive with the loader.
pub(crate) fn build_stub_pipeline(logger: Arc<dyn RedactionLogger>) -> GazeResult<Pipeline> {
    Pipeline::builder()
        .detector(RegexDetector::emails().map_err(map_recognizer_error)?)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(ArcLogger(logger))
        .build()
}

fn map_recognizer_error(err: gaze_recognizers::RecognizerError) -> gaze::Error {
    match err {
        gaze_recognizers::RecognizerError::InvalidRegex(err) => gaze::Error::InvalidRegex(err),
        gaze_recognizers::RecognizerError::UnsupportedValidator { kind } => {
            gaze::Error::Rulepack(gaze::RulepackError::UnsupportedValidator { kind })
        }
        gaze_recognizers::RecognizerError::UnsupportedNormalizer { kind } => {
            gaze::Error::Rulepack(gaze::RulepackError::UnsupportedNormalizer { kind })
        }
    }
}

pub(crate) fn build_context_pipeline(
    context: &TypedContext,
    logger: Arc<dyn RedactionLogger>,
) -> GazeResult<Pipeline> {
    let mut builder = Pipeline::builder();
    for name in context.dictionaries.keys() {
        let class = context
            .class_map
            .get(name)
            .cloned()
            .unwrap_or_else(|| PiiClass::custom(name));
        builder = builder
            .recognizer(DictionaryRecognizer::new(
                format!("context/{name}"),
                class.clone(),
                name,
                context.dictionaries[name].case_sensitive,
                "counter",
            ))
            .rule(ClassRule::new(class, Action::Tokenize));
    }
    builder
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(ArcLogger(logger))
        .build()
}

/// Adapter that lets `PipelineBuilder::redaction_logger` (which takes ownership
/// of a concrete `RedactionLogger`) accept a shared `Arc<dyn RedactionLogger>`.
/// The Arc keeps the handle alive for post-redact counter inspection.
pub(crate) struct ArcLogger(pub(crate) Arc<dyn RedactionLogger>);

impl RedactionLogger for ArcLogger {
    fn log(&self, entry: &RedactionEntry) -> GazeResult<()> {
        self.0.log(entry)
    }
}
