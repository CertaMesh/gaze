//! Host-only detection inputs. These types have no wire representation.
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

/// Explicit host selection for one dispatch. Never populate this from model content.
#[derive(Clone, Default)]
pub enum HostContextSelection {
    /// No private record for this call.
    #[default]
    Absent,
    /// Original JSON, parsed by the shared Context parser inside dispatch.
    Present(HostRecordContext),
}

/// Opaque original JSON. Debug never exposes its contents.
/// Clones share a bounded, single-profile pipeline cache. Reuse this handle across
/// calls to reuse assembly; changing session or assembly inputs replaces the slot.
#[derive(Clone)]
pub struct HostRecordContext(Arc<SecretJson>);

struct SecretJson {
    raw: zeroize::Zeroizing<String>,
    profile: Mutex<Option<Arc<PrivatePipelineProfile>>>,
    #[cfg(test)]
    constructions: std::sync::atomic::AtomicUsize,
}

struct PrivatePipelineProfile {
    session_id: String,
    policy: gaze::Policy,
    rulepacks: Vec<gaze::Rulepack>,
    locales: Vec<gaze::LocaleTag>,
    resident: gaze::Pipeline,
    pipeline: OnceLock<Result<gaze::Pipeline, HostContextError>>,
}

impl HostContextSelection {
    /// Preserve original bytes, including duplicate keys, for validation at dispatch.
    pub fn present(raw: String) -> Self {
        Self::Present(HostRecordContext(Arc::new(SecretJson {
            raw: zeroize::Zeroizing::new(raw),
            profile: Mutex::new(None),
            #[cfg(test)]
            constructions: std::sync::atomic::AtomicUsize::new(0),
        })))
    }
}

impl fmt::Debug for HostRecordContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HostRecordContext([redacted])")
    }
}
impl fmt::Debug for HostContextSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => f.write_str("Absent"),
            Self::Present(_) => f.write_str("Present([redacted])"),
        }
    }
}

/// Host-owned per-call guess budget. Never derive these limits from a record.
/// Each original/decoded argument view must fit both raw and normalized byte
/// limits, including keys, and the scalar limit. Every visited key, value or
/// container also consumes one entry, capped by `max_argument_bytes` per view.
/// Defaults suit small lookup tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostContextPolicy {
    /// Maximum aggregate bytes per view, before and after normalization.
    /// Also caps visited entries (keys, scalar values and containers) per view.
    pub max_argument_bytes: usize,
    /// Maximum scalar values per view, including decoded JSON leaves.
    pub max_argument_scalars: usize,
}

impl Default for HostContextPolicy {
    fn default() -> Self {
        Self {
            max_argument_bytes: 4096,
            max_argument_scalars: 32,
        }
    }
}

/// Value-free context failures; no underlying diagnostic or source chain escapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HostContextError {
    /// Caller confirmed sensitivity but the record matcher cannot safely match it.
    #[error("{0}")]
    Untransformable(gaze::UntransformableReason),
    /// Shared context parser rejected the original JSON.
    #[error("invalid host context")]
    Invalid,
    /// Arguments exceeded a host-owned, record-independent guess budget.
    #[error("host context argument limit exceeded")]
    ArgumentLimit,
    /// The host has not supplied matching resident policy inputs.
    #[error("host context unsupported")]
    Unsupported,
    /// Context cannot be safely assembled against the resident policy.
    #[error("host context policy rejected")]
    Policy,
}

pub(crate) fn parse(
    selection: &HostContextSelection,
) -> Result<Option<gaze::Context>, HostContextError> {
    match selection {
        HostContextSelection::Absent => Ok(None),
        HostContextSelection::Present(raw) => gaze::Context::from_json_str(&raw.0.raw)
            .map_err(|_| HostContextError::Invalid)
            .and_then(|context| match context.untransformable_reason() {
                Some(reason) => Err(HostContextError::Untransformable(reason)),
                None => Ok(Some(context)),
            }),
    }
}

// The original JSON belongs to this handle and never changes. This is called
// before per-call guess exclusions, so the full record is validated on a miss
// and an excluded record can never become the cached detection profile.
pub(crate) fn response_pipeline(
    selection: &HostContextSelection,
    session: &gaze::Session,
    resident: &gaze::Pipeline,
    policy: &gaze::Policy,
    context: &gaze::Context,
    rulepacks: &[gaze::Rulepack],
    locales: &[gaze::LocaleTag],
) -> Result<gaze::Pipeline, HostContextError> {
    let HostContextSelection::Present(source) = selection else {
        return Err(HostContextError::Unsupported);
    };
    let profile = {
        let mut slot = source
            .0
            .profile
            .lock()
            .map_err(|_| HostContextError::Policy)?;
        let matches = slot.as_ref().is_some_and(|profile| {
            profile.session_id == session.audit_session_id()
                && std::ptr::eq(profile.resident.registry(), resident.registry())
                && profile.policy == *policy
                && profile.rulepacks == rulepacks
                && profile.locales == locales
        });
        if !matches {
            *slot = Some(Arc::new(PrivatePipelineProfile {
                session_id: session.audit_session_id().to_owned(),
                policy: policy.clone(),
                rulepacks: rulepacks.to_vec(),
                locales: locales.to_vec(),
                resident: resident.clone(),
                pipeline: OnceLock::new(),
            }));
        }
        slot.as_ref().ok_or(HostContextError::Policy)?.clone()
    };
    let detection = profile.pipeline.get_or_init(|| {
        #[cfg(test)]
        source
            .0
            .constructions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        gaze_assembly::pipeline_with_private_context(
            resident,
            policy,
            context,
            rulepacks,
            &gaze::LocaleChain::from(locales),
        )
        .map_err(|_| HostContextError::Policy)
    });
    // Keep this call's models, safety nets and loggers even on a cache hit.
    detection
        .as_ref()
        .map(|detection| resident.clone().with_detection_policy_from(detection))
        .map_err(|error| *error)
}

#[cfg(test)]
pub(crate) fn construction_count(selection: &HostContextSelection) -> usize {
    match selection {
        HostContextSelection::Absent => 0,
        HostContextSelection::Present(source) => source
            .0
            .constructions
            .load(std::sync::atomic::Ordering::Relaxed),
    }
}

const CLASS_NAMESPACE: &str = "gaze-mcp/host-context/class/v1";
const DICTIONARY_NAMESPACE: &str = "gaze-mcp/host-context/dictionary/v1";

pub(crate) fn validate_session(session: &gaze::Session) -> Result<(), HostContextError> {
    if session.classes().iter().any(|class| {
            matches!(class, gaze::PiiClass::Custom(name) if name.strip_prefix("host_")
            .is_some_and(|id| matches!(id.len(), 32 | 64) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
                && !session.owns_opaque_identifier(CLASS_NAMESPACE, id)))
        })
    {
        return Err(HostContextError::Policy);
    }
    Ok(())
}

fn private_class(
    session: &gaze::Session,
    class: &gaze::PiiClass,
) -> Result<gaze::PiiClass, HostContextError> {
    match class {
        gaze::PiiClass::Custom(name)
            if !matches!(
                name.as_str(),
                "phone"
                    | "iban"
                    | "credit_card"
                    | "national_id"
                    | "steuer_id"
                    | "passport"
                    | "birth_date"
                    | "postal_code"
            ) =>
        {
            Ok(gaze::PiiClass::Custom(opaque_name(
                session,
                CLASS_NAMESPACE,
                name,
            )?))
        }
        _ => Ok(class.clone()),
    }
}

fn opaque_name(
    session: &gaze::Session,
    namespace: &str,
    name: &str,
) -> Result<String, HostContextError> {
    session
        .opaque_identifier(namespace, name)
        .map(|id| format!("host_{id}"))
        .map_err(|_| HostContextError::Policy)
}

pub(crate) fn policy_aliases(
    session: &gaze::Session,
    policy: &gaze::Policy,
) -> Result<std::collections::HashMap<gaze::PiiClass, gaze::PiiClass>, HostContextError> {
    let mut aliases = std::collections::HashMap::new();
    for rule in &policy.rules {
        if let gaze::RuleSpec::Class { class, .. } = rule {
            let hidden = private_class(session, class)?;
            if hidden != *class {
                aliases.insert(class.clone(), hidden);
            }
        }
    }
    Ok(aliases)
}

// Caller-defined identifiers are content too. Keep them out of source ids and tokens.
pub(crate) fn opaque_context(
    session: &gaze::Session,
    context: &mut gaze::Context,
    policy: &gaze::Policy,
) -> Result<gaze::Policy, HostContextError> {
    let mut policy = policy.clone();
    // Dictionary names without class_map also name their custom class.
    for name in context.dictionaries.keys() {
        if !context.class_map.contains_key(name) {
            context
                .class_map
                .insert(name.clone(), gaze::PiiClass::Custom(name.clone()));
        }
    }
    let mut classes = std::collections::HashMap::new();
    for class in context.class_map.values() {
        let hidden = private_class(session, class)?;
        if hidden != *class {
            classes.insert(class.clone(), hidden);
        }
    }
    let original_rules = policy.rules.clone();
    let mut ordered_classes = classes.iter().collect::<Vec<_>>();
    ordered_classes.sort_by_key(|(original, _)| original.to_canonical_str());
    for (original, hidden) in ordered_classes {
        if let Some(kinds) = context
            .record_match_kinds
            .remove(&original.to_canonical_str())
        {
            context
                .record_match_kinds
                .insert(hidden.to_canonical_str(), kinds);
        }
        if let Some(action) = original_rules.iter().find_map(|rule| match rule {
            gaze::RuleSpec::Class { class, action } if class == original => Some(*action),
            gaze::RuleSpec::Default { action } => Some(*action),
            _ => None,
        }) {
            policy.rules.insert(
                0,
                gaze::RuleSpec::Class {
                    class: hidden.clone(),
                    action,
                },
            );
        }
    }
    let dictionaries = std::mem::take(&mut context.dictionaries);
    let class_map = std::mem::take(&mut context.class_map);
    for (name, dictionary) in dictionaries {
        let hidden = if name.starts_with(gaze::RECORD_DICTIONARY_PREFIX) {
            format!(
                "{}{}",
                gaze::RECORD_DICTIONARY_PREFIX,
                opaque_name(session, DICTIONARY_NAMESPACE, &name)?
            )
        } else {
            opaque_name(session, DICTIONARY_NAMESPACE, &name)?
        };
        if let Some(class) = class_map.get(&name) {
            context
                .class_map
                .insert(hidden.clone(), classes.get(class).unwrap_or(class).clone());
        }
        context.dictionaries.insert(hidden, dictionary);
    }
    // Fields and rejection paths are not detection inputs at this boundary.
    context.fields.clear();
    context.record_value_rejections.clear();
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_context_profile_invalidates_on_session_registry_policy_rulepacks_and_locales() {
        let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
        let selection =
            HostContextSelection::present(r#"{"record":{"name":"Synthetic Qorvex"}}"#.into());
        let first = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let second = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let alternate = gaze_assembly::CorePipelineConfig::new().build().unwrap();
        let mut policy = core.policy().clone();
        policy.address_blocks = !policy.address_blocks;
        let mut rulepacks = core.rulepacks().to_vec();
        rulepacks[0].rulepack_id.push_str("-alternate-profile");
        let other_locales = [gaze::LocaleTag::parse("en").unwrap()];
        assert!(other_locales != core.locale_chain().as_slice());
        let cases = [
            (
                &first,
                core.pipeline(),
                core.policy(),
                core.rulepacks(),
                core.locale_chain().as_slice(),
            ),
            (
                &second,
                core.pipeline(),
                core.policy(),
                core.rulepacks(),
                core.locale_chain().as_slice(),
            ),
            (
                &second,
                alternate.pipeline(),
                core.policy(),
                core.rulepacks(),
                core.locale_chain().as_slice(),
            ),
            (
                &second,
                alternate.pipeline(),
                &policy,
                core.rulepacks(),
                core.locale_chain().as_slice(),
            ),
            (
                &second,
                alternate.pipeline(),
                &policy,
                rulepacks.as_slice(),
                core.locale_chain().as_slice(),
            ),
            (
                &second,
                alternate.pipeline(),
                &policy,
                rulepacks.as_slice(),
                other_locales.as_slice(),
            ),
        ];
        for (index, (session, resident, policy, rulepacks, locales)) in
            cases.into_iter().enumerate()
        {
            for _ in 0..3 {
                let mut context = parse(&selection).unwrap().unwrap();
                let policy = opaque_context(session, &mut context, policy).unwrap();
                assert!(response_pipeline(
                    &selection, session, resident, &policy, &context, rulepacks, locales
                )
                .is_ok());
            }
            assert_eq!(construction_count(&selection), index + 1);
        }
    }

    #[test]
    fn host_context_profile_constructs_once_for_concurrent_calls() {
        let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
        let selection =
            HostContextSelection::present(r#"{"record":{"name":"Synthetic Qorvex"}}"#.into());
        let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let barrier = std::sync::Barrier::new(6);
        std::thread::scope(|threads| {
            for _ in 0..6 {
                threads.spawn(|| {
                    let mut context = parse(&selection).unwrap().unwrap();
                    let policy = opaque_context(&session, &mut context, core.policy()).unwrap();
                    barrier.wait();
                    assert!(response_pipeline(
                        &selection,
                        &session,
                        core.pipeline(),
                        &policy,
                        &context,
                        core.rulepacks(),
                        core.locale_chain().as_slice(),
                    )
                    .is_ok());
                });
            }
        });
        assert_eq!(construction_count(&selection), 1);
    }

    #[test]
    fn host_context_profile_caches_failure_and_rejects_poisoned_slot() {
        let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
        let selection = HostContextSelection::present("{}".into());
        let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let mut policy = core.policy().clone();
        policy.detectors.push(gaze::DetectorSpec::default());
        let context = parse(&selection).unwrap().unwrap();
        for _ in 0..3 {
            assert!(matches!(
                response_pipeline(
                    &selection,
                    &session,
                    core.pipeline(),
                    &policy,
                    &context,
                    core.rulepacks(),
                    core.locale_chain().as_slice()
                ),
                Err(HostContextError::Policy)
            ));
        }
        assert_eq!(construction_count(&selection), 1);
        let HostContextSelection::Present(source) = &selection else {
            unreachable!()
        };
        let _panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = source.0.profile.lock().unwrap();
            panic!("poison profile lock");
        }));
        assert!(matches!(
            response_pipeline(
                &selection,
                &session,
                core.pipeline(),
                core.policy(),
                &context,
                core.rulepacks(),
                core.locale_chain().as_slice()
            ),
            Err(HostContextError::Policy)
        ));
    }

    #[test]
    fn host_context_parser_enforces_shared_record_depth_and_field_limits() {
        for (levels, accepted) in [(3, true), (4, false)] {
            let mut record = serde_json::json!({"name":"Synthetic Qorvex"});
            let mut path = "/name".to_owned();
            for level in 0..levels {
                let key = format!("level{level}");
                record = serde_json::json!({key.clone():record});
                path = format!("/{key}{path}");
            }
            let raw = serde_json::json!({"record":record,"field_map":{path:"Name"}}).to_string();
            assert_eq!(gaze::Context::from_json_str(&raw).is_ok(), accepted);
            assert_eq!(parse(&HostContextSelection::present(raw)).is_ok(), accepted);
        }
        for (fields, accepted) in [(32, true), (33, false)] {
            let mut record = serde_json::Map::new();
            let mut field_map = serde_json::Map::new();
            for field in 0..fields {
                let key = format!("field{field}");
                field_map.insert(format!("/{key}"), serde_json::json!("Name"));
                record.insert(key, serde_json::json!(format!("Synthetic Qorvex {field}")));
            }
            let raw = serde_json::json!({"record":record,"field_map":field_map}).to_string();
            assert_eq!(gaze::Context::from_json_str(&raw).is_ok(), accepted);
            assert_eq!(parse(&HostContextSelection::present(raw)).is_ok(), accepted);
        }
    }

    #[test]
    fn host_context_parser_retains_shared_boundary_and_duplicate_key_contracts() {
        let mut at_limit = "{}".to_owned();
        at_limit.push_str(&" ".repeat(4 * 1024 * 1024 - 2));
        let over_limit = format!("{at_limit} ");
        let cases = [
            "{}".to_owned(),
            r#"{"fields":{"private_canary_key":"private_canary_value"}}"#.to_owned(),
            r#"{"record":{"name":"Synthetic Qorvex"}}"#.to_owned(),
            r#"{"record":{"name":"Synthetic Qorvex","name":"Synthetic Zunvel"}}"#.to_owned(),
            r#"{"record":{"name":"Synthetic Qorvex"},"field_map":{"/private_canary_key":"Name"}}"#.to_owned(),
            r#"{"dictionaries":{"private_canary_key":{"terms":["private_canary_value"],"private_canary_unknown":true}}}"#.to_owned(),
            r#"{"record":{"name":["Synthetic Qorvex"]}}"#.to_owned(),
            format!(r#"{{"record":{{"name":"{}"}}}}"#, "q".repeat(256)),
            format!(r#"{{"record":{{"name":"{}"}}}}"#, "q".repeat(257)),
            at_limit,
            over_limit,
            "null".to_owned(),
            "[]".to_owned(),
        ];
        for raw in cases {
            let shared_accepts = gaze::Context::from_json_str(&raw).is_ok();
            let result = parse(&HostContextSelection::present(raw));
            assert_eq!(result.is_ok(), shared_accepts);
            if let Err(error) = result {
                assert_eq!(error, HostContextError::Invalid);
                assert!(!format!("{error:?} {error}").contains("private_canary"));
            }
        }
    }
}
