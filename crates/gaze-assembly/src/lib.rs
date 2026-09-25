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
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::path::Path;

use gaze::{
    ClassRule, ColumnRule, Context, DefaultRule, LocaleChain, PiiClass, Pipeline, PipelineBuilder,
    RuleSpec, Rulepack,
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
pub use locale::locale_gated_activation_locales;
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
    let pipeline =
        build_pipeline_builder(policy, context, rulepacks, active_locales, ner_threshold)?
            .build()?;
    if policy.safety_net.backend == gaze::SafetyNetPolicyBackend::Nym {
        attach_nym_safety_net(pipeline, policy, None, None, None)
    } else {
        Ok(pipeline)
    }
}

/// Attach the Nym net with the same validation used by policy-driven Rust assembly and the CLI.
/// `model_dir_override` takes precedence over the policy path; environment lookup belongs to
/// the CLI caller, not to library assembly.
#[cfg(feature = "safety-net-nym")]
pub fn attach_nym_safety_net(
    pipeline: Pipeline,
    policy: &gaze::Policy,
    model_dir_override: Option<&Path>,
    max_input_bytes: Option<usize>,
    intra_threads: Option<NonZeroUsize>,
) -> Result<Pipeline, BuildError> {
    use gaze_recognizers::safety_net::nym::{
        verify_nym_bundle, NymConfig, NymSafetyNet, REQUIRED_NYM_SMALL_ARTIFACTS,
    };

    let model_dir = model_dir_override
        .or(policy.safety_net.nym_model_dir.as_deref())
        .ok_or(BuildError::NymModelDirMissing)?;
    if !model_dir.exists() {
        let name = model_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model");
        return Err(BuildError::NymBundle(
            gaze::SafetyNetError::WeightsMissing {
                path: format!("<missing:{name}>"),
            },
        ));
    }
    for name in REQUIRED_NYM_SMALL_ARTIFACTS {
        if !model_dir.join(name).exists() {
            return Err(BuildError::NymBundle(
                gaze::SafetyNetError::WeightsMissing {
                    path: format!("<missing:{name}>"),
                },
            ));
        }
    }
    verify_nym_bundle(model_dir).map_err(BuildError::NymBundle)?;
    let mut config = NymConfig::new(model_dir)
        .with_operating_point(policy.safety_net.nym.clone().unwrap_or_default());
    if let Some(bytes) = max_input_bytes {
        config = config.with_max_input_bytes(bytes);
    }
    if let Some(threads) = intra_threads {
        config = config.with_intra_threads(threads);
    }
    let net = NymSafetyNet::new(config);
    net.preload().map_err(BuildError::NymBundle)?;
    attach_preloaded_safety_net(pipeline, net)
}

#[cfg(feature = "safety-net-nym")]
fn attach_preloaded_safety_net<N: gaze::SafetyNet + 'static>(
    pipeline: Pipeline,
    net: N,
) -> Result<Pipeline, BuildError> {
    attach_safety_net_checked(
        pipeline,
        |pipeline| Ok(pipeline.with_safety_net(net)),
        BuildError::NymNotAttached,
    )
}

/// Attach one safety net and fail closed if it does not increase the pipeline's count.
/// The attachment closure owns registration so callers cannot omit the count check.
pub fn attach_safety_net_checked<E>(
    pipeline: Pipeline,
    attach: impl FnOnce(Pipeline) -> Result<Pipeline, E>,
    not_attached: E,
) -> Result<Pipeline, E> {
    let before = pipeline.safety_net_count();
    let pipeline = attach(pipeline)?;
    require_net_added(before, pipeline.safety_net_count(), not_attached)?;
    Ok(pipeline)
}

fn require_net_added<E>(before: usize, after: usize, not_attached: E) -> Result<(), E> {
    if before.checked_add(1) != Some(after) {
        return Err(not_attached);
    }
    Ok(())
}

/// A policy requesting Nym cannot silently lose its safety net in a build without the feature.
#[cfg(not(feature = "safety-net-nym"))]
pub fn attach_nym_safety_net(
    _pipeline: Pipeline,
    _policy: &gaze::Policy,
    _model_dir_override: Option<&Path>,
    _max_input_bytes: Option<usize>,
    _intra_threads: Option<NonZeroUsize>,
) -> Result<Pipeline, BuildError> {
    Err(BuildError::NymFeatureDisabled)
}

/// [`build_pipeline`] without the final `build()`, for a caller that layers its own
/// recognizers or safety net on the exact policy assembly (`gaze index ingest` adds its
/// NER bundle and field detector on top of the `gaze clean` floor).
///
/// The policy rules are already registered. Rule order is first match, so an added
/// recognizer's class falls through to the policy's default rule.
pub fn build_pipeline_builder(
    policy: &gaze::Policy,
    context: &Context,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
    ner_threshold: Option<f32>,
) -> Result<PipelineBuilder, BuildError> {
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

    Ok(builder.into_inner())
}

/// Collision-family fallback classes (`custom:family:<name>`) that an active
/// mandatory-anchor recognizer can emit and that the policy shows intent about
/// without naming reachably: some rule names one of the family's member
/// classes (before or after the first `Default` rule), or a rule names the
/// family class only after the first `Default` rule, where it is never reached.
///
/// Such a token does not leak: it takes the strictest action among its member
/// classes' rules and the default (`gaze::Action::strictness_rank`). The list
/// is informational. It tells the adopter that the token class they will see
/// differs from the member class they named, and that a rule for the family
/// class declared before the default rule sets the action directly.
///
/// Scope is limited to families with a `mandatory_anchor` member because those
/// emit the family class *systematically* whenever the anchor cue pack is not
/// loaded. Families that only fall back on a rare precedence tie are excluded
/// to keep the notice low-noise. A policy that names neither the family nor a
/// member (a bare `default` rule) is not flagged.
pub fn uncovered_collision_family_classes(
    policy: &gaze::Policy,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
) -> Vec<String> {
    // `rule::resolve` in the pipeline takes the first matching rule and a
    // `Default` rule matches unconditionally, so everything declared after the
    // first `Default` rule is dead code.
    let default_index = policy
        .rules
        .iter()
        .position(|rule| matches!(rule, RuleSpec::Default { .. }));
    let live_rules = &policy.rules[..default_index.unwrap_or(policy.rules.len())];
    let names = |rules: &[RuleSpec], class: &PiiClass| {
        rules
            .iter()
            .any(|rule| matches!(rule, RuleSpec::Class { class: named, .. } if named == class))
    };

    mandatory_anchor_families(policy, rulepacks, active_locales)
        .into_iter()
        .filter(|(family, members)| {
            let family_class = PiiClass::family(family);
            // Intent counts wherever it is declared: a member or family rule
            // placed after the default rule is dead, and the adopter who wrote
            // it is exactly who needs to hear the family class falls to the
            // default.
            !names(live_rules, &family_class)
                && (members.iter().any(|member| names(&policy.rules, member))
                    || names(&policy.rules, &family_class))
        })
        .map(|(family, _)| format!("custom:family:{family}"))
        .collect()
}

/// Member classes, by family name, of every collision family that an enabled
/// recognizer active under `active_locales` declares with a `mandatory_anchor`:
/// the families that emit a `custom:family:<family>` fallback token when their
/// anchor cue is unavailable.
fn mandatory_anchor_families(
    policy: &gaze::Policy,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
) -> BTreeMap<String, BTreeSet<PiiClass>> {
    let rulepack_members = rulepacks
        .iter()
        .flat_map(|rulepack| &rulepack.recognizers)
        .filter(|recognizer| {
            detector_wiring::recognizer_activates(recognizer, policy, active_locales)
        })
        .filter_map(|recognizer| Some((recognizer.collision.as_ref()?, &recognizer.class)));
    let policy_members = policy
        .detectors
        .iter()
        .filter_map(|detector| Some((detector.collision.as_ref()?, &detector.class)));

    let mut families = BTreeMap::<String, (bool, BTreeSet<PiiClass>)>::new();
    for (collision, class) in rulepack_members.chain(policy_members) {
        let entry = families.entry(collision.family.clone()).or_default();
        entry.0 |= collision.mandatory_anchor.is_some();
        entry.1.insert(class.clone());
    }
    families
        .into_iter()
        .filter(|(_, (anchored, _))| *anchored)
        .map(|(family, (_, members))| (family, members))
        .collect()
}

#[cfg(test)]
mod tests;
