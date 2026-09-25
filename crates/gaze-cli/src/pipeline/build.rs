use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use gaze::{
    Action, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline, PipelineBuilder, Policy,
    PolicyError, RedactionEntry, RedactionLogError, RedactionLogger, RuleSpec, Rulepack,
    RulepackSource, SessionPolicy, SessionScope, TypedContext,
};

use crate::clean_overrides::CleanOverrides;
use crate::error::CliError;

pub(crate) struct ResolvedPipeline {
    pub(crate) pipeline: Pipeline,
    pub(crate) policy: Policy,
    pub(crate) rulepacks: Vec<Rulepack>,
    pub(crate) locale_chain: LocaleChain,
    pub(crate) dictionaries: DictionaryBundle,
}

/// Diagnostic for a loaded policy whose fall-through action preserves detected values.
pub(crate) fn policy_warnings(policy: &Policy, pipeline: &Pipeline) -> Vec<String> {
    let mut warnings = Vec::new();
    if !policy
        .rulepacks
        .bundled
        .iter()
        .any(|id| matches!(id.as_str(), "core" | "core-extended"))
    {
        warnings.push("notice: core rulepack floor is off".to_string());
    }
    let mut classes: BTreeSet<PiiClass> = pipeline
        .registry()
        .recognizer_ids()
        .filter_map(|id| pipeline.registry().recognizer(id))
        .flat_map(|recognizer| recognizer.possible_classes())
        .collect();
    classes.extend(
        pipeline
            .registry()
            .family_policy()
            .families()
            .map(PiiClass::family),
    );
    let mut unruled = Vec::new();
    let mut generalized = Vec::new();
    for class in classes {
        let first_action = first_policy_action(policy, &class);
        if let Some(family) = class.as_family_name() {
            if !matches!(first_action, Some((true, _))) {
                let own = first_action.map_or(Action::Preserve, |(_, action)| action);
                let derived = pipeline
                    .registry()
                    .family_member_classes(family)
                    .into_iter()
                    .map(|member| {
                        first_policy_action(policy, &member)
                            .map_or(Action::Preserve, |(_, action)| action)
                    })
                    .chain(std::iter::once(own))
                    .max_by_key(|action| action.strictness_rank())
                    .unwrap_or(own);
                if derived == Action::Preserve {
                    unruled.push(class.to_canonical_str());
                }
                continue;
            }
        }
        match first_action {
            Some((true, Action::Generalize)) => generalized.push(class.to_canonical_str()),
            Some((false, Action::Preserve)) | None => unruled.push(class.to_canonical_str()),
            _ => {}
        }
    }

    if !unruled.is_empty() {
        warnings.push(format!(
            "warning: policy preserves {} detected class{} without a reachable class rule: {}; values can reach the model raw. Back up custom rules, then run gaze setup --force, or set the default action to \"tokenize\".",
            unruled.len(),
            if unruled.len() == 1 { "" } else { "es" },
            unruled.join(", ")
        ));
    }
    if !generalized.is_empty() {
        warnings.push(format!(
            "warning: class generalize is one-way (no restore token) for: {}.",
            generalized.join(", ")
        ));
    }
    warnings
}

fn first_policy_action(policy: &Policy, class: &PiiClass) -> Option<(bool, Action)> {
    policy.rules.iter().find_map(|rule| match rule {
        RuleSpec::Class {
            class: named,
            action,
        } if named == class => Some((true, *action)),
        RuleSpec::Default { action } => Some((false, *action)),
        _ => None,
    })
}

/// [`ResolvedPipeline`] before `build()`, for a verb that layers recognizers on top.
pub(crate) struct ResolvedPipelineBuilder {
    pub(crate) builder: PipelineBuilder,
    pub(crate) policy: Policy,
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
    let resolved = resolve_pipeline_builder(
        policy_path,
        overrides,
        cli_locales,
        cli_ner_threshold,
        context.as_ref(),
    )?;
    let pipeline = resolved.builder.build().map_err(map_pipeline_error)?;
    let pipeline = match logger {
        Some(logger) => pipeline.with_redaction_logger(ArcLogger(logger)),
        None => pipeline,
    };

    Ok(ResolvedPipeline {
        pipeline,
        policy: resolved.policy,
        rulepacks: resolved.rulepacks,
        locale_chain: resolved.locale_chain,
        dictionaries: resolved.dictionaries,
    })
}

/// [`resolve_pipeline`] up to, not including, `build()`.
pub(crate) fn resolve_pipeline_builder(
    policy_path: Option<&Path>,
    overrides: &CleanOverrides,
    cli_locales: &[String],
    cli_ner_threshold: Option<f32>,
    context: Option<&TypedContext>,
) -> std::result::Result<ResolvedPipelineBuilder, CliError> {
    let policy = match policy_path {
        Some(path) => overrides.apply_to(&Policy::load_for_cli(path).map_err(map_policy_error)?),
        None => policy_less_policy(overrides)?,
    };
    if policy.detectors.is_empty()
        && policy.rulepacks.bundled.is_empty()
        && policy.rulepacks.paths.is_empty()
    {
        return Err(CliError::PolicyConfigDetail(
            "no detectors or rulepacks configured".to_string(),
        ));
    }
    let cli_locales = parse_cli_locales(cli_locales)?;
    let inputs = gaze_assembly::resolve_policy_inputs(
        &policy,
        context,
        cli_locales.as_deref(),
        cli_ner_threshold,
    )
    .map_err(map_build_error)?;

    let builder = pipeline_builder_from_policy(
        &policy,
        &inputs.rulepacks,
        context,
        &inputs.locale_chain,
        inputs.ner_threshold,
    )?;

    Ok(ResolvedPipelineBuilder {
        builder,
        policy,
        rulepacks: inputs.rulepacks,
        locale_chain: inputs.locale_chain,
        dictionaries: inputs.dictionaries,
    })
}

/// Synthesizes the policy for a run without `--policy`.
///
/// Rulepack flags override their matching policy fields. Every
/// activated class tokenizes and the default rule tokenizes too, so spans with
/// no class rule (context dictionaries, NER) fail closed instead of passing
/// through raw. This matches `gaze_assembly::CorePipelineConfig`, which backs
/// `gaze mcp serve` and policy-less `gaze proxy`.
fn policy_less_policy(overrides: &CleanOverrides) -> std::result::Result<Policy, CliError> {
    let mut session = SessionPolicy::default();
    session.scope = SessionScope::Persistent;
    session.ttl_secs = Some(86_400);

    let mut base = Policy::default();
    base.session = session;
    base.rulepacks.bundled = gaze::RulepackPolicy::default_bundled();
    let mut policy = overrides.apply_to(&base);

    let mut rules = class_rules_for_rulepacks(&policy.rulepacks.bundled, &policy.rulepacks.paths)?;
    rules.push(RuleSpec::Default {
        action: Action::Tokenize,
    });
    policy.rules = rules;
    Ok(policy)
}

fn class_rules_for_rulepacks(
    bundled: &[String],
    paths: &[std::path::PathBuf],
) -> std::result::Result<Vec<RuleSpec>, CliError> {
    let mut classes = std::collections::BTreeSet::new();
    for bundle in bundled {
        let contents = gaze_recognizers::embedded(bundle).ok_or_else(|| {
            CliError::PolicyConfigDetail(format!("unknown bundled rulepack: {bundle}"))
        })?;
        let rulepack = Rulepack::load(RulepackSource::Embedded(contents)).map_err(|err| {
            CliError::PolicyConfigDetail(format!("embedded rulepack '{bundle}': {err}"))
        })?;
        classes.extend(rulepack.activated_classes());
    }
    for path in paths {
        let rulepack = Rulepack::load(RulepackSource::Path(path.clone()))
            .map_err(|err| map_pipeline_error(gaze::Error::Rulepack(err)))?;
        classes.extend(rulepack.activated_classes());
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

fn pipeline_builder_from_policy(
    policy: &Policy,
    rulepacks: &[Rulepack],
    context: Option<&TypedContext>,
    locale_chain: &LocaleChain,
    ner_threshold: f32,
) -> std::result::Result<PipelineBuilder, CliError> {
    let empty_context = TypedContext {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    };
    gaze_assembly::build_pipeline_builder(
        policy,
        context.unwrap_or(&empty_context),
        rulepacks,
        locale_chain,
        Some(ner_threshold),
    )
    .map_err(map_build_error)
}

fn map_build_error(err: gaze_assembly::BuildError) -> CliError {
    match err {
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
        err @ (gaze_assembly::BuildError::NymFeatureDisabled
        | gaze_assembly::BuildError::NymModelDirMissing
        | gaze_assembly::BuildError::NymNotAttached
        | gaze_assembly::BuildError::NymBundle(_)) => {
            CliError::SafetyNetPolicyConfigDetail(err.to_string())
        }
    }
}

/// Emit a stderr notice for each collision-family fallback class the policy
/// shows intent about without naming reachably (see
/// [`gaze_assembly::uncovered_collision_family_classes`]). The span does not
/// necessarily stay protected: it takes the strictest action among its member
/// classes' rules and the default, which can still be `preserve`. The separate
/// policy diagnostic names raw family classes. This notice tells the adopter
/// which class the token will carry and how to set its action explicitly.
pub(crate) fn warn_uncovered_collision_families(
    policy: &Policy,
    rulepacks: &[Rulepack],
    locale_chain: &LocaleChain,
) {
    for family_class in
        gaze_assembly::uncovered_collision_family_classes(policy, rulepacks, locale_chain)
    {
        eprintln!(
            "warning: policy names a member class of '{family_class}' but no reachable rule \
             names the family class itself; a span the family cannot settle (no anchor cue, \
             or a precedence tie) is emitted as '{family_class}' and takes the strictest \
             action among its member classes' rules and the default rule. To set it \
             directly, add BEFORE your default rule: [[rule]] kind = \"class\" class = \
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

/// Adapter that lets `PipelineBuilder::redaction_logger` (which takes ownership
/// of a concrete `RedactionLogger`) accept a shared `Arc<dyn RedactionLogger>`.
/// The Arc keeps the handle alive for post-redact counter inspection.
pub(crate) struct ArcLogger(pub(crate) Arc<dyn RedactionLogger>);

impl RedactionLogger for ArcLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.0.log(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_less_selection_matches_core_pipeline_config() {
        let resolved = resolve_pipeline_builder(None, &CleanOverrides::default(), &[], None, None)
            .expect("default CLI pipeline resolves");
        let cli_ids: Vec<_> = resolved
            .rulepacks
            .iter()
            .map(|pack| pack.rulepack_id.as_str())
            .collect();
        let config = gaze_assembly::CorePipelineConfig::new();
        let config_ids = config.bundled_rulepack_ids();
        let config_pack_ids: Vec<_> = config_ids
            .iter()
            .map(|id| {
                let contents = gaze_recognizers::embedded(id).expect("embedded rulepack exists");
                Rulepack::load(RulepackSource::Embedded(contents))
                    .expect("embedded rulepack loads")
                    .rulepack_id
            })
            .collect();

        assert_eq!(resolved.policy.rulepacks.bundled, config_ids);
        assert_eq!(cli_ids, config_pack_ids);
    }
}
