#![cfg_attr(docsrs, feature(doc_cfg))]

//! Policy-to-pipeline builder using bundled defaults.
//!
//! Provides [`CorePipelineConfig`], the recommended entry point for Rust adopters
//! who want the `core` rulepack and locale-aware recognizers without manually wiring
//! recognizer, rulepack, policy, and pipeline crates.
//!
//! # Quickstart
//!
//! ```toml
//! [dependencies]
//! gaze = "0.6"
//! gaze-assembly = "0.7"
//! ```
//!
//! ```rust,no_run
//! use gaze::{CleanDocument, RawDocument, Scope, Session};
//! use gaze_assembly::CorePipelineConfig;
//!
//! let core = CorePipelineConfig::new().build()?;
//! let session = Session::new(Scope::Conversation("s1".into()))?;
//! let CleanDocument::Text(_clean) = core.pipeline().redact(
//!     &session,
//!     RawDocument::Text("alice@example.invalid".into()), // fixture-cited(crates/gaze-assembly/src/lib.rs:tests::core_pipeline_config_tokenizes_synthetic_email)
//! )? else {
//!     panic!("text variant expected");
//! };
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! For custom recognizer topology, use [`gaze::Pipeline::builder`] directly.
//!
use std::collections::BTreeSet;

use gaze::{
    Action, ClassRule, ColumnRule, Context, DefaultRule, LocaleChain, PiiClass, Pipeline, RuleSpec,
    Rulepack,
};

mod class_map;
pub mod defaults;
mod detector_wiring;
mod error;
mod locale;
mod ner;
mod registration;
mod template;

pub use defaults::CorePipeline;
/// Configuration builder for the bundled-default pipeline.
///
/// Activates the `core` rulepack and registers locale-aware recognizers. Use this
/// for the common case; drop to [`gaze::Pipeline::builder`] only when you need a
/// custom recognizer topology or non-bundled rulepack.
pub use defaults::CorePipelineConfig;
pub use error::BuildError;
pub(crate) use locale::{merged_locale_vocab, register_anchor_cue_bundles};

/// Assemble a pipeline from a loaded [`gaze::Policy`], matching the CLI code path.
///
/// Use this when you load a policy file programmatically and want to mirror the
/// exact assembly the `gaze` binary uses, including locale chain resolution and
/// rulepack loading.
///
/// Fails closed with [`BuildError::NoRecognizers`] when nothing was actually
/// registered: policy detectors, rulepack recognizers admitted by the
/// safety-tier + locale predicate (and not skipped for a missing optional cue
/// bucket), context dictionaries, and a loaded NER model all
/// count. The guard reads the registration count rather than re-deriving
/// eligibility, so a pipeline that would preserve every byte cannot build.
pub fn build_pipeline(
    policy: &gaze::Policy,
    context: &Context,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
    ner_threshold: Option<f32>,
) -> Result<Pipeline, BuildError> {
    let mut builder = registration::AssemblyBuilder::default();
    let mut registered_dictionaries = BTreeSet::<String>::new();
    let locale_vocab = merged_locale_vocab(rulepacks, active_locales);

    detector_wiring::register_policy_detectors(
        &mut builder,
        policy,
        context,
        &mut registered_dictionaries,
    )?;
    detector_wiring::register_rulepack_recognizers(
        &mut builder,
        policy,
        context,
        rulepacks,
        active_locales,
        &locale_vocab,
        &mut registered_dictionaries,
    )?;
    detector_wiring::register_context_dictionaries(
        &mut builder,
        policy,
        context,
        &registered_dictionaries,
    )?;
    register_anchor_cue_bundles(&mut builder, rulepacks, active_locales);
    ner::register_ner(&mut builder, policy, ner_threshold)?;

    if builder.registered_recognizers() == 0 {
        return Err(BuildError::NoRecognizers);
    }

    for rule in &policy.rules {
        match rule {
            RuleSpec::Class { class, action } => {
                builder.rule(ClassRule::new(class.clone(), *action))
            }
            RuleSpec::Column { column, action } => builder.rule(ColumnRule::new(column, *action)),
            RuleSpec::Default { action } => builder.rule(DefaultRule::new(*action)),
            _ => {
                return Err(
                    gaze::PolicyError::BadTtl("unsupported rule variant".to_string()).into(),
                )
            }
        }
    }

    Ok(builder.into_inner().build()?)
}

/// Collision-family fallback classes that loaded recognizers can emit but the
/// policy would silently preserve — a never-leak fail-open.
///
/// When a collision-family recognizer (for example the `payment-card-or-iban`
/// IBAN recognizer) cannot resolve its mandatory anchor or hits a precedence
/// tie, it emits one family-level token with class
/// `custom:family:<family>` instead of the narrow variant class
/// (`custom:iban`). A policy keyed on the variant class with a non-protective
/// default (`preserve`, or no default rule at all) then matches no rule for the
/// family class and leaves the detected span **unredacted** — a silent PII
/// leak (north-star axis 1).
///
/// This returns the `custom:family:<family>` class strings that are:
/// 1. declared by an enabled, mandatory-anchor recognizer whose locales
///    intersect `active_locales` (rulepack recognizers or policy custom
///    recognizers), and
/// 2. not covered by an explicit `[[rule]] kind = "class"` rule declared
///    *before* the first `default` rule (rules after an unconditional
///    `default` are unreachable at runtime), and
/// 3. left to a non-protective default action.
///
/// Scope is limited to families with a `mandatory_anchor` member because those
/// emit the family fallback class *systematically* whenever the anchor cue pack
/// is not loaded — the reported failure mode. Families that only fall back on a
/// rare precedence tie are excluded to keep the warning low-noise.
///
/// The result is empty when the default action tokenizes/redacts (no leak is
/// possible) or when every such family already has an explicit rule. Callers
/// should surface each entry as a policy warning so adopters can add a covering
/// rule. See `docs/explanation/detection/anchor-resolution.md`.
pub fn uncovered_collision_family_classes(
    policy: &gaze::Policy,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
) -> Vec<String> {
    // `action_for` in the pipeline takes the first matching rule and a
    // `Default` rule matches unconditionally, so the first `Default` rule is
    // the effective default AND everything declared after it is dead code;
    // absence of one falls through to `Action::Preserve`.
    let default_index = policy
        .rules
        .iter()
        .position(|rule| matches!(rule, RuleSpec::Default { .. }));
    let default_action = default_index.map(|index| match &policy.rules[index] {
        RuleSpec::Default { action } => *action,
        _ => unreachable!("position() matched a Default rule"),
    });
    let default_is_protective = matches!(
        default_action,
        Some(Action::Tokenize | Action::Redact | Action::FormatPreserve | Action::Generalize)
    );
    if default_is_protective {
        return Vec::new();
    }

    // A covering class rule only reaches the runtime matcher when it appears
    // BEFORE the first `Default` rule — a rule pasted after it is silently
    // shadowed, which is precisely the leak this function exists to flag.
    let live_rules = &policy.rules[..default_index.unwrap_or(policy.rules.len())];
    mandatory_anchor_families(policy, rulepacks, active_locales)
        .into_iter()
        .filter(|family| {
            let family_class = PiiClass::Custom(format!("family:{family}"));
            !live_rules
                .iter()
                .any(|rule| matches!(rule, RuleSpec::Class { class, .. } if *class == family_class))
        })
        .map(|family| format!("custom:family:{family}"))
        .collect()
}

/// Family names declared by an enabled, mandatory-anchor recognizer active under
/// `active_locales` — the recognizers that emit a `custom:family:<family>`
/// fallback token when their anchor cue is unavailable.
fn mandatory_anchor_families(
    policy: &gaze::Policy,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
) -> BTreeSet<String> {
    let rulepack_families = rulepacks
        .iter()
        .flat_map(|rulepack| &rulepack.recognizers)
        .filter(|recognizer| {
            detector_wiring::recognizer_activates(recognizer, policy, active_locales)
        })
        .filter_map(|recognizer| recognizer.collision.as_ref());
    let policy_families = policy
        .detectors
        .iter()
        .filter_map(|detector| detector.collision.as_ref());

    rulepack_families
        .chain(policy_families)
        .filter(|collision| collision.mandatory_anchor.is_some())
        .map(|collision| collision.family.clone())
        .collect()
}

#[cfg(test)]
mod tests;
