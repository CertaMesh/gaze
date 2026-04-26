use thiserror::Error;

use crate::context::Context;
pub use gaze_types::{
    DictionaryBundle, DictionaryEntry, DictionarySource, DictionaryStats, RulepackDict,
};

#[derive(Debug, Error)]
pub enum DictionaryLoadError {
    #[error("dictionary '{name}' has no terms")]
    Empty { name: String },
    #[error(
        "unicode dictionary insensitive matching unsupported in v0.4.0, use case_sensitive = true"
    )]
    UnicodeInsensitiveUnsupported { name: String },
}

pub fn dictionary_bundle_from_context(ctx: &Context) -> DictionaryBundle {
    let entries = ctx.dictionaries.iter().map(|(name, dictionary)| {
        (
            name.clone(),
            DictionaryEntry::new(
                dictionary.terms.clone(),
                dictionary.case_sensitive,
                DictionarySource::Cli,
            ),
        )
    });
    DictionaryBundle::from_entries(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextDictionary, PiiClass};
    use std::collections::HashMap;
    use serde_json::Map;

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
        let a = DictionaryBundle::from_rulepack_terms(&[RulepackDict {
            name: "songs".to_string(),
            terms: vec!["Song A".to_string()],
            case_sensitive: true,
        }]);
        let b = DictionaryBundle::from_rulepack_terms(&[RulepackDict {
            name: "songs".to_string(),
            terms: vec!["Song B".to_string()],
            case_sensitive: true,
        }]);

        let merged = DictionaryBundle::merge(a, b);
        let entry = merged.get("songs").expect("entry");
        assert_eq!(entry.terms(), &["Song B".to_string()]);
    }
}
