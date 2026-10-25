//! Host-only, request-local detection inputs. These types have no wire representation.
use std::fmt;

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
#[derive(Clone)]
pub struct HostRecordContext(std::sync::Arc<SecretJson>);

struct SecretJson(zeroize::Zeroizing<String>);

impl HostContextSelection {
    /// Preserve original bytes, including duplicate keys, for validation at dispatch.
    pub fn present(raw: String) -> Self {
        Self::Present(HostRecordContext(std::sync::Arc::new(SecretJson(
            zeroize::Zeroizing::new(raw),
        ))))
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

/// Value-free context failures; no underlying diagnostic or source chain escapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HostContextError {
    /// Shared context parser rejected the original JSON.
    #[error("invalid host context")]
    Invalid,
    /// The host has not supplied matching resident policy inputs.
    #[error("host context unsupported")]
    Unsupported,
    /// Context cannot be safely assembled against the resident policy.
    #[error("host context policy rejected")]
    Policy,
}

pub(crate) fn parse(
    selection: HostContextSelection,
) -> Result<Option<gaze::Context>, HostContextError> {
    match selection {
        HostContextSelection::Absent => Ok(None),
        HostContextSelection::Present(raw) => gaze::Context::from_json_str(&raw.0 .0)
            .map(Some)
            .map_err(|_| HostContextError::Invalid),
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
    for (original, hidden) in &classes {
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

/// Conservative guess exclusion uses the shared record normalization without cues or
/// token boundaries: tool-added punctuation/cues must not turn a guess into an oracle.
pub(crate) fn contains_guess(text: &str, terms: &[String]) -> bool {
    terms
        .iter()
        .any(|term| gaze_recognizers::contains_record_guess(text, term))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(parse(HostContextSelection::present(raw)).is_ok(), accepted);
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
            assert_eq!(parse(HostContextSelection::present(raw)).is_ok(), accepted);
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
            let result = parse(HostContextSelection::present(raw));
            assert_eq!(result.is_ok(), shared_accepts);
            if let Err(error) = result {
                assert_eq!(error, HostContextError::Invalid);
                assert!(!format!("{error:?} {error}").contains("private_canary"));
            }
        }
    }
}
