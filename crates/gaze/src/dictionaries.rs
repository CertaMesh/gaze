use crate::context::Context;
pub use gaze_types::{
    DictionaryBundle, DictionaryEntry, DictionaryLoadError, DictionarySource, DictionaryStats,
    RulepackDict,
};

pub trait DictionaryBundleExt {
    fn from_context(ctx: &Context) -> Self;
}

impl DictionaryBundleExt for DictionaryBundle {
    fn from_context(ctx: &Context) -> Self {
        dictionary_bundle_from_context(ctx)
    }
}

pub fn dictionary_bundle_from_context(ctx: &Context) -> DictionaryBundle {
    let entries = ctx.dictionaries.iter().map(|(name, dictionary)| {
        (
            name.clone(),
            DictionaryEntry::new(
                name,
                dictionary.terms.clone(),
                dictionary.case_sensitive,
                DictionarySource::Cli,
            )
            .expect("Context validates dictionary terms before bundle construction"),
        )
    });
    DictionaryBundle::from_entries(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextDictionary, PiiClass};
    use serde_json::Map;
    use std::collections::HashMap;

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
        };

        let bundle = dictionary_bundle_from_context(&ctx);
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
        };

        let bundle = DictionaryBundle::from_context(&ctx);
        assert!(bundle.get("dict_alpha").is_some());
    }
}
