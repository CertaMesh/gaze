use std::collections::HashMap;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use thiserror::Error;

use crate::context::Context;

#[derive(Debug, Default)]
pub struct DictionaryBundle {
    entries: HashMap<String, DictionaryEntry>,
}

#[derive(Debug)]
pub struct DictionaryEntry {
    terms: Vec<String>,
    case_sensitive: bool,
    automaton: AhoCorasick,
    source: DictionarySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictionarySource {
    Cli,
    Rulepack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryStats {
    pub name: String,
    pub term_count: usize,
    pub source: DictionarySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulepackDict {
    pub name: String,
    pub terms: Vec<String>,
    pub case_sensitive: bool,
}

#[derive(Debug, Error)]
pub enum DictionaryLoadError {
    #[error("dictionary '{name}' has no terms")]
    Empty { name: String },
    #[error(
        "unicode dictionary insensitive matching unsupported in v0.4.0, use case_sensitive = true"
    )]
    UnicodeInsensitiveUnsupported { name: String },
    #[error("failed to build dictionary '{name}' automaton: {source}")]
    Automaton {
        name: String,
        #[source]
        source: aho_corasick::BuildError,
    },
}

impl DictionaryBundle {
    pub fn from_context(ctx: &Context) -> Self {
        let mut entries = HashMap::with_capacity(ctx.dictionaries.len());
        for (name, dictionary) in &ctx.dictionaries {
            let entry = DictionaryEntry::new(
                name,
                dictionary.terms.clone(),
                dictionary.case_sensitive,
                DictionarySource::Cli,
            )
            .expect("Context validates dictionary terms before bundle construction");
            entries.insert(name.clone(), entry);
        }
        Self { entries }
    }

    pub fn from_rulepack_terms(terms: &[RulepackDict]) -> Self {
        let mut entries = HashMap::with_capacity(terms.len());
        for dictionary in terms {
            let entry = DictionaryEntry::new(
                &dictionary.name,
                dictionary.terms.clone(),
                dictionary.case_sensitive,
                DictionarySource::Rulepack,
            )
            .expect("Policy validates dictionary terms before bundle construction");
            entries.insert(dictionary.name.clone(), entry);
        }
        Self { entries }
    }

    pub fn merge(a: Self, b: Self) -> Self {
        let mut entries = a.entries;
        entries.extend(b.entries);
        Self { entries }
    }

    pub fn get(&self, name: &str) -> Option<&DictionaryEntry> {
        self.entries.get(name)
    }

    pub fn stats(&self) -> Vec<DictionaryStats> {
        let mut stats = self
            .entries
            .iter()
            .map(|(name, entry)| DictionaryStats {
                name: name.clone(),
                term_count: entry.terms.len(),
                source: entry.source,
            })
            .collect::<Vec<_>>();
        stats.sort_by(|a, b| a.name.cmp(&b.name));
        stats
    }
}

impl DictionaryEntry {
    pub fn new(
        name: &str,
        terms: Vec<String>,
        case_sensitive: bool,
        source: DictionarySource,
    ) -> Result<Self, DictionaryLoadError> {
        if terms.is_empty() {
            return Err(DictionaryLoadError::Empty {
                name: name.to_string(),
            });
        }
        if !case_sensitive && terms.iter().any(|term| !term.is_ascii()) {
            return Err(DictionaryLoadError::UnicodeInsensitiveUnsupported {
                name: name.to_string(),
            });
        }
        let automaton = AhoCorasickBuilder::new()
            .ascii_case_insensitive(!case_sensitive)
            .build(&terms)
            .map_err(|source| DictionaryLoadError::Automaton {
                name: name.to_string(),
                source,
            })?;
        Ok(Self {
            terms,
            case_sensitive,
            automaton,
            source,
        })
    }

    pub fn automaton(&self) -> &AhoCorasick {
        &self.automaton
    }

    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    pub fn terms(&self) -> &[String] {
        &self.terms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextDictionary, PiiClass};
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

        let bundle = DictionaryBundle::from_context(&ctx);
        let entry = bundle.get("dict_alpha").expect("entry");
        assert!(entry.automaton().find("AAA-12345").is_some());
        assert!(entry.automaton().find("aaa-12345").is_none());
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
        assert!(entry.automaton().find("Song B").is_some());
        assert!(entry.automaton().find("Song A").is_none());
    }
}
