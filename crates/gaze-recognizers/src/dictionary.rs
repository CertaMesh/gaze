use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use gaze_types::{
    Candidate, ConflictTier, DetectContext, DictionaryEntry, LocaleBasis, LocaleTag, PiiClass,
    Recognizer,
};

/// Lookup-based [`Recognizer`] for tenant-specific PII.
///
/// Matches exact strings from a runtime-supplied dictionary: order IDs, song
/// titles, customer codes, or any domain-specific identifiers that regex cannot
/// reliably detect.
///
/// Supply dictionaries at runtime via [`DetectContext`] or the CLI's
/// `--context-json` wiring. Dictionary entries are session-scoped detection
/// inputs and are not stored in the audit log.
pub struct DictionaryRecognizer {
    id: String,
    class: PiiClass,
    dictionary_name: String,
    case_sensitive: bool,
    token_family: String,
    locales: Vec<LocaleTag>,
    locale_basis: LocaleBasis,
    score: f32,
    priority: i32,
    compiled_dictionaries: Mutex<HashMap<DictionaryCacheKey, Arc<AhoCorasick>>>,
}

impl DictionaryRecognizer {
    pub fn new(
        id: impl Into<String>,
        class: PiiClass,
        dictionary_name: impl Into<String>,
        case_sensitive: bool,
        token_family: impl Into<String>,
    ) -> Self {
        Self::with_rulepack_fields(
            id,
            class,
            dictionary_name,
            case_sensitive,
            token_family,
            vec![LocaleTag::Global],
            1.0,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_rulepack_fields(
        id: impl Into<String>,
        class: PiiClass,
        dictionary_name: impl Into<String>,
        case_sensitive: bool,
        token_family: impl Into<String>,
        locales: Vec<LocaleTag>,
        score: f32,
        priority: i32,
    ) -> Self {
        Self {
            id: id.into(),
            class,
            dictionary_name: dictionary_name.into(),
            case_sensitive,
            token_family: token_family.into(),
            locales,
            locale_basis: LocaleBasis::Document,
            score,
            priority,
            compiled_dictionaries: Mutex::new(HashMap::new()),
        }
    }

    pub fn dictionary_name(&self) -> &str {
        &self.dictionary_name
    }

    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Overrides how the recognizer's locale metadata affects eligibility.
    pub fn with_locale_basis(mut self, locale_basis: LocaleBasis) -> Self {
        self.locale_basis = locale_basis;
        self
    }

    fn automaton_for(&self, entry: &DictionaryEntry) -> Arc<AhoCorasick> {
        let key = DictionaryCacheKey::from_entry(entry);
        let mut compiled = self
            .compiled_dictionaries
            .lock()
            .expect("dictionary automaton cache poisoned");
        Arc::clone(compiled.entry(key).or_insert_with(|| {
            Arc::new(
                AhoCorasickBuilder::new()
                    .ascii_case_insensitive(!entry.case_sensitive())
                    .build(entry.terms())
                    .expect("DictionaryEntry validates terms before automaton construction"),
            )
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DictionaryCacheKey {
    terms_hash: u64,
    case_sensitive: bool,
}

impl DictionaryCacheKey {
    fn from_entry(entry: &DictionaryEntry) -> Self {
        let mut hasher = DefaultHasher::new();
        entry.terms().hash(&mut hasher);
        Self {
            terms_hash: hasher.finish(),
            case_sensitive: entry.case_sensitive(),
        }
    }
}

impl Recognizer for DictionaryRecognizer {
    fn id(&self) -> &str {
        &self.id
    }

    fn supported_class(&self) -> &PiiClass {
        &self.class
    }

    fn detect(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        let Some(entry) = ctx.dictionaries.get(&self.dictionary_name) else {
            return Ok(Vec::new());
        };
        let automaton = self.automaton_for(entry);

        Ok(automaton
            .find_iter(input)
            .filter(|m| is_token_boundary_match(input, m.start(), m.end()))
            .map(|m| {
                Candidate::new(
                    m.start()..m.end(),
                    self.class.clone(),
                    self.id.clone(),
                    self.score,
                    self.priority,
                    Some(input[m.start()..m.end()].to_string()),
                    self.token_family.clone(),
                    format!(
                        "dictionary:{}[#{}]",
                        self.dictionary_name,
                        m.pattern().as_usize()
                    ),
                    ConflictTier::None,
                    Vec::new(),
                )
            })
            .collect())
    }

    fn token_family(&self) -> &str {
        &self.token_family
    }

    fn locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn locale_basis(&self) -> LocaleBasis {
        self.locale_basis
    }
}

fn is_token_boundary_match(input: &str, start: usize, end: usize) -> bool {
    !has_identifier_char_before(input, start) && !has_identifier_char_after(input, end)
}

fn has_identifier_char_before(input: &str, start: usize) -> bool {
    input[..start]
        .chars()
        .next_back()
        .is_some_and(is_identifier_char)
}

fn has_identifier_char_after(input: &str, end: usize) -> bool {
    input[end..].chars().next().is_some_and(is_identifier_char)
}

fn is_identifier_char(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use gaze::{
        dictionary_bundle_from_context, ContextDictionary, RecognizerRegistry, TypedContext,
    };
    use serde_json::Map;

    use super::*;

    #[test]
    fn recognizer_detects_dictionary_hits_from_context_bundle() {
        let ctx = TypedContext {
            dictionaries: HashMap::from([(
                "dict_alpha".to_string(),
                ContextDictionary {
                    terms: vec!["AAA-12345".to_string()],
                    case_sensitive: true,
                },
            )]),
            class_map: HashMap::new(),
            fields: Map::new(),
        };
        let bundle = dictionary_bundle_from_context(&ctx);
        let detect_context = DetectContext::new(&[LocaleTag::Global], &bundle);
        let recognizer = DictionaryRecognizer::new(
            "dict/dict_alpha",
            PiiClass::Custom("class_alpha".to_string()),
            "dict_alpha",
            true,
            "counter",
        );

        let hits = recognizer
            .detect("Customer bought AAA-12345", &detect_context)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].span, 16..25);
        assert_eq!(hits[0].class, PiiClass::Custom("class_alpha".to_string()));
    }

    #[test]
    fn recognizer_locale_gates_dictionary_hits() {
        let ctx = TypedContext {
            dictionaries: HashMap::from([(
                "songs".to_string(),
                ContextDictionary {
                    terms: vec!["Bohemian Rhapsody".to_string()],
                    case_sensitive: false,
                },
            )]),
            class_map: HashMap::new(),
            fields: Map::new(),
        };
        let bundle = dictionary_bundle_from_context(&ctx);
        let detect_context = DetectContext::new(&[LocaleTag::EnUs], &bundle);
        let recognizer = DictionaryRecognizer::with_rulepack_fields(
            "dict/songs",
            PiiClass::Custom("song".to_string()),
            "songs",
            false,
            "counter",
            vec![LocaleTag::DeDe],
            1.0,
            0,
        );

        let registry = RecognizerRegistry::builder().register(recognizer).build();
        let hits = registry
            .detect_all("Listening to bohemian rhapsody", &detect_context)
            .expect("detect all");
        assert!(hits.is_empty());
    }

    #[test]
    fn dictionary_recognizer_emits_per_term_source() {
        let ctx = TypedContext {
            dictionaries: HashMap::from([(
                "songs".to_string(),
                ContextDictionary {
                    terms: vec![
                        "alpha-one".to_string(),
                        "bravo-two".to_string(),
                        "charlie-three".to_string(),
                    ],
                    case_sensitive: true,
                },
            )]),
            class_map: HashMap::new(),
            fields: Map::new(),
        };
        let bundle = dictionary_bundle_from_context(&ctx);
        let detect_context = DetectContext::new(&[LocaleTag::Global], &bundle);
        let recognizer = DictionaryRecognizer::new(
            "dict/songs",
            PiiClass::Custom("song".to_string()),
            "songs",
            true,
            "counter",
        );

        let hits = recognizer
            .detect(
                "first alpha-one, second bravo-two, third charlie-three",
                &detect_context,
            )
            .unwrap();

        assert_eq!(hits.len(), 3);
        let source_shape = regex::Regex::new(r"^dictionary:[a-z_]+\[#\d+\]$").unwrap();
        for hit in &hits {
            assert!(
                source_shape.is_match(&hit.source),
                "unexpected source shape: {}",
                hit.source
            );
        }
        assert_eq!(hits[0].source, "dictionary:songs[#0]");
        assert_eq!(hits[1].source, "dictionary:songs[#1]");
        assert_eq!(hits[2].source, "dictionary:songs[#2]");
    }

    #[test]
    fn dictionary_recognizer_source_index_matches_automaton_for_duplicate_terms() {
        let ctx = TypedContext {
            dictionaries: HashMap::from([(
                "songs".to_string(),
                ContextDictionary {
                    terms: vec![
                        "same-song".to_string(),
                        "same-song".to_string(),
                        "other-song".to_string(),
                    ],
                    case_sensitive: true,
                },
            )]),
            class_map: HashMap::new(),
            fields: Map::new(),
        };
        let bundle = dictionary_bundle_from_context(&ctx);
        let detect_context = DetectContext::new(&[LocaleTag::Global], &bundle);
        let recognizer = DictionaryRecognizer::new(
            "dict/songs",
            PiiClass::Custom("song".to_string()),
            "songs",
            true,
            "counter",
        );

        let hits = recognizer
            .detect("same-song then other-song", &detect_context)
            .unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].source, "dictionary:songs[#0]");
        assert_eq!(hits[1].source, "dictionary:songs[#2]");
    }

    #[test]
    fn dictionary_recognizer_does_not_match_inside_identifier() {
        let ctx = TypedContext {
            dictionaries: HashMap::from([(
                "artist_refs".to_string(),
                ContextDictionary {
                    terms: vec!["Artist".to_string()],
                    case_sensitive: true,
                },
            )]),
            class_map: HashMap::new(),
            fields: Map::new(),
        };
        let bundle = dictionary_bundle_from_context(&ctx);
        let detect_context = DetectContext::new(&[LocaleTag::Global], &bundle);
        let recognizer = DictionaryRecognizer::new(
            "dict/artist_refs",
            PiiClass::Custom("artist_ref".to_string()),
            "artist_refs",
            true,
            "counter",
        );

        let hits = recognizer
            .detect("Du antwortest als Artistfy-Support.", &detect_context)
            .unwrap();

        assert!(hits.is_empty(), "unexpected dictionary hits: {hits:?}");
    }
}
