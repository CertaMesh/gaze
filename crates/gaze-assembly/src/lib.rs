use std::collections::BTreeSet;

use gaze::{
    ClassRule, ColumnRule, Context, DefaultRule, LocaleChain, Pipeline, RuleSpec, Rulepack,
};

mod class_map;
mod detector_wiring;
mod error;
mod locale;
mod ner;
mod template;

pub use error::BuildError;
pub(crate) use locale::merged_locale_vocab;

pub fn build_pipeline(
    policy: &gaze::Policy,
    context: &Context,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
    ner_threshold: Option<f32>,
) -> Result<Pipeline, BuildError> {
    let mut builder = Pipeline::builder();
    let mut registered_dictionaries = BTreeSet::<String>::new();
    let locale_vocab = merged_locale_vocab(rulepacks, active_locales);

    builder = detector_wiring::register_policy_detectors(
        builder,
        policy,
        context,
        &mut registered_dictionaries,
    )?;
    builder = detector_wiring::register_rulepack_recognizers(
        builder,
        policy,
        context,
        rulepacks,
        active_locales,
        &locale_vocab,
        &mut registered_dictionaries,
    )?;
    builder = detector_wiring::register_context_dictionaries(
        builder,
        policy,
        context,
        &registered_dictionaries,
    )?;

    let has_policy_detector = !policy.detectors.is_empty();
    let has_enabled_rulepack_recognizer = rulepacks.iter().any(|rulepack| {
        rulepack
            .recognizers
            .iter()
            .any(|recognizer| recognizer.enabled && active_locales.intersects(&recognizer.locales))
    });
    let has_usable_ner = policy
        .ner
        .as_ref()
        .is_some_and(|ner| ner.model_dir.is_some());
    let has_context_dictionary = context
        .dictionaries
        .keys()
        .any(|name| !registered_dictionaries.contains(name));

    if !has_policy_detector
        && !has_enabled_rulepack_recognizer
        && !has_usable_ner
        && !has_context_dictionary
    {
        return Err(BuildError::NoRecognizers);
    }

    for rule in &policy.rules {
        builder = match rule {
            RuleSpec::Class { class, action } => {
                builder.rule(ClassRule::new(class.clone(), *action))
            }
            RuleSpec::Column { column, action } => builder.rule(ColumnRule::new(column, *action)),
            RuleSpec::Default { action } => builder.rule(DefaultRule::new(*action)),
        };
    }

    builder = ner::register_ner(builder, policy, ner_threshold)?;

    Ok(builder.build()?)
}

#[cfg(test)]
mod tests;
