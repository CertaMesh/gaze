use crate::context::{Context, RECORD_DICTIONARY_PREFIX};
use crate::UntransformableReason;
pub use gaze_types::{
    DictionaryBundle, DictionaryEntry, DictionaryLoadError, DictionarySource, DictionaryStats,
    RulepackDict,
};

pub trait DictionaryBundleExt {
    /// Convert accepted caller context, refusing unsafe short record values.
    fn from_context(ctx: &Context) -> Result<Self, UntransformableReason>
    where
        Self: Sized;
}

impl DictionaryBundleExt for DictionaryBundle {
    fn from_context(ctx: &Context) -> Result<Self, UntransformableReason> {
        dictionary_bundle_from_context(ctx)
    }
}

/// Convert caller context into runtime dictionaries.
///
/// Refuses rejected short record values with a closed, value-free reason.
/// Observer-only tools must inspect [`Context::untransformable_reason`], warn
/// without values, and explicitly clear `record_value_rejections` first.
pub fn dictionary_bundle_from_context(
    ctx: &Context,
) -> Result<DictionaryBundle, UntransformableReason> {
    if let Some(reason) = ctx.untransformable_reason() {
        return Err(reason);
    }
    let entries = ctx.dictionaries.iter().map(|(name, dictionary)| {
        let mut entry = DictionaryEntry::new(
            name,
            dictionary.terms.clone(),
            dictionary.case_sensitive,
            DictionarySource::Cli,
        )
        .expect("Context validates dictionary terms before bundle construction");
        if name.starts_with(RECORD_DICTIONARY_PREFIX) {
            if let Some(class) = ctx.class_map.get(name) {
                entry = entry.with_record_allowed_kinds(
                    ctx.record_allowed_match_kinds(class, &dictionary.terms[0]),
                );
            }
        }
        (name.clone(), entry)
    });
    Ok(DictionaryBundle::from_entries(entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextDictionary, PiiClass};
    use serde_json::Map;
    use std::collections::HashMap;

    #[test]
    fn caller_value_too_short_refuses_public_dictionary_conversions_without_value() {
        let context = Context::from_json_str(
            r#"{"record":{"Qx":"Qx","email":"synthetic@example.invalid"},"field_map":{"/Qx":"Name"}}"#,
        )
        .unwrap();
        for result in [
            dictionary_bundle_from_context(&context),
            DictionaryBundle::from_context(&context),
        ] {
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("rejected caller context must refuse dictionary conversion"),
            };
            assert_eq!(error, UntransformableReason::CallerValueTooShort);
            assert_eq!(error.to_string(), "caller_value_too_short");
            for rendered in [error.to_string(), format!("{error:?}")] {
                assert!(!rendered.contains("Qx"));
                assert!(!rendered.contains("synthetic@example.invalid"));
            }
        }
    }

    #[test]
    fn context_bundle_builds_automata_per_request() {
        let ctx = Context {
            dictionaries: HashMap::from([(
                "dict_alpha".to_string(),
                ContextDictionary {
                    terms: vec!["AAA-12345".to_string()],
                    case_sensitive: true,
                },
            )]),
            class_map: HashMap::from([(
                "dict_alpha".to_string(),
                PiiClass::Custom("class_alpha".to_string()),
            )]),
            fields: Map::new(),
            record_match_kinds: Default::default(),
            record_value_rejections: Default::default(),
        };

        let bundle = dictionary_bundle_from_context(&ctx).unwrap();
        let entry = bundle.get("dict_alpha").expect("entry");
        assert_eq!(entry.terms(), &["AAA-12345".to_string()]);
        assert!(entry.case_sensitive());
    }

    #[test]
    fn merge_prefers_second_bundle_for_same_name() {
        let a = DictionaryBundle::from_rulepack_terms(&[RulepackDict::new(
            "songs",
            vec!["Song A".to_string()],
            true,
        )]);
        let b = DictionaryBundle::from_rulepack_terms(&[RulepackDict::new(
            "songs",
            vec!["Song B".to_string()],
            true,
        )]);

        let merged = DictionaryBundle::merge(a, b);
        let entry = merged.get("songs").expect("entry");
        assert_eq!(entry.terms(), &["Song B".to_string()]);
    }

    #[test]
    fn extension_trait_restores_from_context_constructor() {
        let ctx = Context {
            dictionaries: HashMap::from([(
                "dict_alpha".to_string(),
                ContextDictionary {
                    terms: vec!["AAA-12345".to_string()],
                    case_sensitive: true,
                },
            )]),
            class_map: HashMap::from([(
                "dict_alpha".to_string(),
                PiiClass::Custom("class_alpha".to_string()),
            )]),
            fields: Map::new(),
            record_match_kinds: Default::default(),
            record_value_rejections: Default::default(),
        };

        let bundle = DictionaryBundle::from_context(&ctx).unwrap();
        assert!(bundle.get("dict_alpha").is_some());
    }
}
