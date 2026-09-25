//! Policy-derived inputs shared by CLI and benchmark assembly.

use gaze::{
    dictionary_bundle_from_context, DictionaryBundle, LocaleChain, LocaleTag, Policy, PolicyError,
    RawMatch, Rulepack, RulepackDict, RulepackSource, TypedContext, DEFAULT_NER_THRESHOLD,
};

use crate::BuildError;

pub struct ResolvedPolicyInputs {
    pub rulepacks: Vec<Rulepack>,
    pub locale_chain: LocaleChain,
    pub dictionaries: DictionaryBundle,
    pub ner_threshold: f32,
}

/// Resolve the inputs that `gaze clean --policy` passes to pipeline assembly.
pub fn resolve_policy_inputs(
    policy: &Policy,
    context: Option<&TypedContext>,
    cli_locales: Option<&[LocaleTag]>,
    cli_ner_threshold: Option<f32>,
) -> Result<ResolvedPolicyInputs, BuildError> {
    let mut rulepacks = Vec::new();
    for id in &policy.rulepacks.bundled {
        let contents = gaze_recognizers::embedded(id)
            .ok_or_else(|| PolicyError::BundledRulepackUnknown { value: id.clone() })?;
        rulepacks.push(Rulepack::load(RulepackSource::Embedded(contents))?);
    }
    for path in &policy.rulepacks.paths {
        rulepacks.push(Rulepack::load(RulepackSource::Path(path.clone()))?);
    }

    let context_bundle = context
        .map(dictionary_bundle_from_context)
        .unwrap_or_default();
    let mut policy_dictionaries = policy.dictionaries.clone();
    policy_dictionaries.extend(dictionary_terms_from_rulepacks(&rulepacks)?);
    let policy_bundle = DictionaryBundle::from_rulepack_terms(&policy_dictionaries);
    let dictionaries = DictionaryBundle::merge(policy_bundle, context_bundle);

    let mut defaults = Vec::new();
    for rulepack in &rulepacks {
        for locale in &rulepack.default_locales {
            if !defaults.contains(locale) {
                defaults.push(locale.clone());
            }
        }
    }
    if policy.rulepacks.auto_activate_locale_gated {
        for locale in crate::locale_gated_activation_locales(&rulepacks) {
            if !defaults.contains(&locale) {
                defaults.push(locale);
            }
        }
    }
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(
        cli_locales,
        policy.locale.as_deref(),
        Some(&defaults),
    );
    let ner_threshold = cli_ner_threshold
        .or_else(|| policy.ner.as_ref().map(|ner| ner.threshold))
        .unwrap_or(DEFAULT_NER_THRESHOLD);
    Ok(ResolvedPolicyInputs {
        rulepacks,
        locale_chain,
        dictionaries,
        ner_threshold,
    })
}

fn dictionary_terms_from_rulepacks(
    rulepacks: &[Rulepack],
) -> Result<Vec<RulepackDict>, BuildError> {
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
                let file =
                    std::fs::read_to_string(path).map_err(|err| PolicyError::BadDictionary {
                        name: recognizer.id.clone(),
                        reason: format!("failed to read terms_file: {err}"),
                    })?;
                all_terms.extend(
                    file.lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .map(str::to_string),
                );
            }
            if all_terms.is_empty() {
                return Err(PolicyError::BadDictionary {
                    name: recognizer.id.clone(),
                    reason: "dictionary matcher requires terms, terms_file, or terms_from_context"
                        .into(),
                }
                .into());
            }
            if !case_sensitive && all_terms.iter().any(|term| !term.is_ascii()) {
                return Err(PolicyError::BadDictionary {
                    name: recognizer.id.clone(),
                    reason: "unicode dictionary insensitive matching unsupported in v0.4.0, use case_sensitive = true".into(),
                }.into());
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
