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
    ClassRule, ColumnRule, Context, DefaultRule, LocaleChain, Pipeline, RuleSpec, Rulepack,
};

mod class_map;
pub mod defaults;
mod detector_wiring;
mod error;
mod locale;
mod ner;
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
    builder = register_anchor_cue_bundles(builder, rulepacks, active_locales);

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
            _ => {
                return Err(
                    gaze::PolicyError::BadTtl("unsupported rule variant".to_string()).into(),
                )
            }
        };
    }

    builder = ner::register_ner(builder, policy, ner_threshold)?;

    Ok(builder.build()?)
}

#[cfg(test)]
mod tests;
