use std::path::Path;
use std::sync::Arc;

use gaze::{
    dictionary_bundle_from_context, Action, ClassRule, DefaultRule, DictionaryBundle, LocaleChain,
    LocaleTag, PiiClass, Pipeline, Policy, PolicyError, RawMatch, RedactionEntry,
    RedactionLogError, RedactionLogger, Result as GazeResult, RuleSpec, Rulepack, RulepackDict,
    RulepackSource, SessionPolicy, SessionScope, TypedContext, DEFAULT_NER_THRESHOLD,
};
use gaze_recognizers::{DictionaryRecognizer, RegexDetector};

use crate::clean_overrides::CleanOverrides;
use crate::error::CliError;

pub(crate) struct ResolvedPipeline {
    pub(crate) pipeline: Pipeline,
    pub(crate) policy: Option<Policy>,
    pub(crate) rulepacks: Vec<Rulepack>,
    pub(crate) locale_chain: LocaleChain,
    pub(crate) dictionaries: DictionaryBundle,
}

/// Resolves every policy-derived input before constructing the pipeline.
///
/// The order is load-bearing: overrides, rulepacks, dictionaries, locale chain,
/// auto-activation, then pipeline assembly. All CLI entry points must use this
/// sequence so a policy has one detection surface regardless of the verb.
pub(crate) fn resolve_pipeline(
    policy_path: Option<&Path>,
    overrides: &CleanOverrides,
    cli_locales: &[String],
    cli_ner_threshold: Option<f32>,
    context: Option<TypedContext>,
    logger: Option<Arc<dyn RedactionLogger>>,
) -> std::result::Result<ResolvedPipeline, CliError> {
    let loaded_policy = policy_path
        .map(Policy::load_for_cli)
        .transpose()
        .map_err(map_policy_error)?
        .map(|policy| overrides.apply_to(&policy));
    let cli_rulepack_policy = if loaded_policy.is_none() && has_rulepack_overrides(overrides) {
        Some(policy_for_rulepack_overrides(overrides)?)
    } else {
        None
    };
    let policy = loaded_policy.or(cli_rulepack_policy);
    let rulepacks = policy
        .as_ref()
        .map(load_rulepacks)
        .transpose()
        .map_err(map_pipeline_error)?
        .unwrap_or_default();

    let context_bundle = context
        .as_ref()
        .map(dictionary_bundle_from_context)
        .unwrap_or_default();
    let rulepack_dictionaries =
        dictionary_terms_from_rulepacks(&rulepacks).map_err(map_pipeline_error)?;
    let policy_bundle = policy
        .as_ref()
        .map(|policy| {
            let mut dictionaries = policy.dictionaries.clone();
            dictionaries.extend(rulepack_dictionaries);
            DictionaryBundle::from_rulepack_terms(&dictionaries)
        })
        .unwrap_or_default();
    let dictionaries = DictionaryBundle::merge(policy_bundle, context_bundle);

    let mut rulepack_default_locales = merged_rulepack_default_locales(&rulepacks);
    if policy
        .as_ref()
        .is_some_and(|policy| policy.rulepacks.auto_activate_locale_gated)
    {
        for locale in gaze_assembly::locale_gated_activation_locales(&rulepacks) {
            if !rulepack_default_locales.contains(&locale) {
                rulepack_default_locales.push(locale);
            }
        }
    }
    let cli_locales = parse_cli_locales(cli_locales)?;
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(
        cli_locales.as_deref(),
        policy.as_ref().and_then(|policy| policy.locale.as_deref()),
        Some(&rulepack_default_locales),
    );
    let ner_threshold = resolve_ner_threshold(cli_ner_threshold, policy.as_ref());

    let pipeline = match policy.as_ref() {
        Some(policy) => build_pipeline_from_policy(
            policy,
            &rulepacks,
            context.as_ref(),
            &locale_chain,
            ner_threshold,
        )?,
        None if context.is_some() => {
            build_context_pipeline(context.as_ref().expect("checked context")).map_err(|err| {
                CliError::PolicyConfigDetail(format!("context pipeline build: {err}"))
            })?
        }
        None => {
            tracing::warn!("gaze clean running with stub pipeline because --policy was omitted");
            build_stub_pipeline().map_err(|err| {
                CliError::PolicyConfigDetail(format!("stub pipeline build: {err}"))
            })?
        }
    };
    let pipeline = match logger {
        Some(logger) => pipeline.with_redaction_logger(ArcLogger(logger)),
        None => pipeline,
    };

    Ok(ResolvedPipeline {
        pipeline,
        policy,
        rulepacks,
        locale_chain,
        dictionaries,
    })
}

fn has_rulepack_overrides(overrides: &CleanOverrides) -> bool {
    overrides.rulepack_bundled.is_some() || !overrides.rulepack_paths.is_empty()
}

fn policy_for_rulepack_overrides(
    overrides: &CleanOverrides,
) -> std::result::Result<Policy, CliError> {
    let mut rules = class_rules_for_bundled_overrides(overrides)?;
    rules.push(RuleSpec::Default {
        action: Action::Preserve,
    });
    let mut session = SessionPolicy::default();
    session.scope = SessionScope::Persistent;
    session.ttl_secs = Some(86_400);

    let mut base = Policy::default();
    base.session = session;
    base.rules = rules;
    Ok(overrides.apply_to(&base))
}

fn class_rules_for_bundled_overrides(
    overrides: &CleanOverrides,
) -> std::result::Result<Vec<RuleSpec>, CliError> {
    let Some(bundled) = &overrides.rulepack_bundled else {
        return Ok(Vec::new());
    };
    let mut classes = std::collections::BTreeSet::new();
    for bundle in bundled {
        let contents = gaze_recognizers::embedded(bundle).ok_or_else(|| {
            CliError::PolicyConfigDetail(format!("unknown bundled rulepack: {bundle}"))
        })?;
        let rulepack = Rulepack::load(RulepackSource::Embedded(contents)).map_err(|err| {
            CliError::PolicyConfigDetail(format!("embedded rulepack '{bundle}': {err}"))
        })?;
        classes.extend(rulepack.activated_classes());
        classes.extend(
            rulepack
                .recognizers
                .iter()
                .filter(|recognizer| recognizer.enabled)
                .filter_map(|recognizer| {
                    recognizer
                        .collision
                        .as_ref()
                        .and_then(|collision| {
                            collision
                                .mandatory_anchor
                                .as_ref()
                                .map(|_| &collision.family)
                        })
                        .map(|family| PiiClass::family(family))
                }),
        );
    }
    Ok(classes
        .into_iter()
        .map(|class| RuleSpec::Class {
            class,
            action: Action::Tokenize,
        })
        .collect())
}

pub(crate) fn parse_cli_locales(
    raw: &[String],
) -> std::result::Result<Option<Vec<LocaleTag>>, CliError> {
    if raw.is_empty() {
        return Ok(None);
    }
    raw.iter()
        .map(|locale| {
            LocaleTag::parse(locale).map_err(|err| {
                CliError::PolicyConfigDetail(format!("invalid --locale '{locale}': {err}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map(Some)
}

pub(crate) fn map_policy_error(err: PolicyError) -> CliError {
    match err {
        PolicyError::Io(_) => CliError::PolicyOpen,
        PolicyError::UnsupportedRuleKind(_) => {
            CliError::PolicyConfigDetail("column rules not supported in CLI mode".to_string())
        }
        PolicyError::PolicySchemaUnsupported { found, supported } => {
            CliError::PolicySchemaUnsupported { found, supported }
        }
        other => CliError::PolicyConfigDetail(other.to_string()),
    }
}

pub(crate) fn map_pipeline_error(err: gaze::Error) -> CliError {
    match err {
        gaze::Error::Policy(policy_err) => map_policy_error(policy_err),
        gaze::Error::Rulepack(rulepack_err) => {
            CliError::PolicyConfigDetail(format!("rulepack error: {rulepack_err}"))
        }
        _ => CliError::Pipeline,
    }
}

pub(crate) fn build_pipeline_from_policy(
    policy: &Policy,
    rulepacks: &[Rulepack],
    context: Option<&TypedContext>,
    locale_chain: &LocaleChain,
    ner_threshold: f32,
) -> std::result::Result<Pipeline, CliError> {
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
        gaze_assembly::BuildError::NoRecognizers => map_policy_error(PolicyError::NoDetectors),
        gaze_assembly::BuildError::Policy(err) => map_policy_error(err),
        gaze_assembly::BuildError::Rulepack(err) => map_pipeline_error(gaze::Error::Rulepack(err)),
        gaze_assembly::BuildError::Pipeline(err) => map_pipeline_error(err),
        gaze_assembly::BuildError::UnknownLocaleBucket { bucket, .. } => {
            map_policy_error(PolicyError::UnknownLocaleBucket { name: bucket })
        }
        gaze_assembly::BuildError::Recognizer(err) => {
            CliError::PolicyConfigDetail(format!("recognizer error: {err}"))
        }
    })
}

/// Emit a stderr warning for each collision-family fallback class that the
/// policy leaves to a non-protective default action (see
/// [`gaze_assembly::uncovered_collision_family_classes`]). Surfaces the silent
/// PII leak described in issue #360 at clean time.
pub(crate) fn warn_uncovered_collision_families(
    policy: &Policy,
    rulepacks: &[Rulepack],
    locale_chain: &LocaleChain,
) {
    for family_class in
        gaze_assembly::uncovered_collision_family_classes(policy, rulepacks, locale_chain)
    {
        eprintln!(
            "warning: detection class '{family_class}' has no matching policy rule and the \
             default action preserves it; ambiguous spans will be left unredacted (potential \
             leak). Add BEFORE your default rule: [[rule]] kind = \"class\" class = \
             \"{family_class}\" action = \"tokenize\""
        );
    }
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
            dictionaries.push(RulepackDict::new(
                recognizer.id.clone(),
                all_terms,
                *case_sensitive,
            ));
        }
    }
    Ok(dictionaries)
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

/// Stub pipeline used until the policy.toml loader (issue #3) lands.
/// Ships only a regex email detector + tokenize rule so the CLI contract can
/// be exercised end-to-end; richer detectors arrive with the loader.
fn build_stub_pipeline() -> GazeResult<Pipeline> {
    Pipeline::builder()
        .detector(RegexDetector::emails().map_err(map_recognizer_error)?)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
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
        _ => gaze::Error::Rulepack(gaze::RulepackError::UnsupportedMatcher(
            "unsupported recognizer error variant".to_string(),
        )),
    }
}

fn build_context_pipeline(context: &TypedContext) -> GazeResult<Pipeline> {
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
    builder.rule(DefaultRule::new(Action::Preserve)).build()
}

/// Adapter that lets `PipelineBuilder::redaction_logger` (which takes ownership
/// of a concrete `RedactionLogger`) accept a shared `Arc<dyn RedactionLogger>`.
/// The Arc keeps the handle alive for post-redact counter inspection.
pub(crate) struct ArcLogger(pub(crate) Arc<dyn RedactionLogger>);

impl RedactionLogger for ArcLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.0.log(entry)
    }
}
