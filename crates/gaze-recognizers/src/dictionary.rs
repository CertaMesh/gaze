use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use gaze_types::{
    Candidate, ConflictTier, DetectContext, DictionaryEntry, LocaleBasis, LocaleTag, PiiClass,
    Recognizer,
};
use unicode_casefold::UnicodeCaseFold;

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
    compiled_unicode: Mutex<HashMap<DictionaryCacheKey, Arc<AhoCorasick>>>,
    unicode_case_insensitive: bool,
    cache_capacity: usize,
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
            compiled_unicode: Mutex::new(HashMap::new()),
            unicode_case_insensitive: false,
            cache_capacity: usize::MAX,
        }
    }

    pub fn dictionary_name(&self) -> &str {
        &self.dictionary_name
    }

    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Limit retained automata when one registered slot receives changing values.
    pub fn with_cache_capacity(mut self, capacity: usize) -> Self {
        self.cache_capacity = capacity.max(1);
        self
    }

    /// Record names need Unicode case matching while preserving source byte offsets.
    pub fn with_unicode_case_insensitive(mut self) -> Self {
        self.unicode_case_insensitive = true;
        self
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
        if let Some(existing) = compiled.get(&key) {
            return Arc::clone(existing);
        }
        if compiled.len() >= self.cache_capacity {
            compiled.clear();
        }
        let automaton = Arc::new(
            AhoCorasickBuilder::new()
                .ascii_case_insensitive(!entry.case_sensitive())
                .build(entry.terms())
                .expect("DictionaryEntry validates terms before automaton construction"),
        );
        compiled.insert(key, Arc::clone(&automaton));
        automaton
    }

    fn unicode_automaton_for(&self, entry: &DictionaryEntry) -> Arc<AhoCorasick> {
        let key = DictionaryCacheKey::from_entry(entry);
        let mut compiled = self
            .compiled_unicode
            .lock()
            .expect("dictionary Unicode cache poisoned");
        if let Some(existing) = compiled.get(&key) {
            return Arc::clone(existing);
        }
        if compiled.len() >= self.cache_capacity {
            compiled.clear();
        }
        let folded_terms = entry
            .terms()
            .iter()
            .map(|term| term.as_str().case_fold().collect::<String>())
            .collect::<Vec<_>>();
        let automaton = Arc::new(
            AhoCorasickBuilder::new()
                .build(folded_terms)
                .expect("DictionaryEntry validates terms before automaton construction"),
        );
        compiled.insert(key, Arc::clone(&automaton));
        automaton
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
    /// A dictionary term is rule evidence.
    fn evidence(&self) -> gaze_types::EvidenceKind {
        gaze_types::EvidenceKind::Rule
    }

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
        let matches = if self.unicode_case_insensitive {
            let (folded, starts, ends) = fold_with_original_offsets(input);
            self.unicode_automaton_for(entry)
                .find_iter(&folded)
                .filter_map(|hit| {
                    Some((
                        *starts.get(&hit.start())?,
                        *ends.get(&hit.end())?,
                        hit.pattern().as_usize(),
                    ))
                })
                .collect::<Vec<_>>()
        } else {
            self.automaton_for(entry)
                .find_iter(input)
                .map(|hit| (hit.start(), hit.end(), hit.pattern().as_usize()))
                .collect::<Vec<_>>()
        };

        Ok(matches
            .into_iter()
            .filter(|(start, end, _)| is_token_boundary_match(input, *start, *end))
            .map(|(start, end, index)| {
                Candidate::new(
                    start..end,
                    self.class.clone(),
                    self.id.clone(),
                    self.score,
                    self.priority,
                    Some(input[start..end].to_string()),
                    self.token_family.clone(),
                    format!("dictionary:{}[#{}]", self.dictionary_name, index),
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

    // `detect` reads only `ctx.dictionaries`, which is the same bundle at every chain step.
    fn detect_is_locale_invariant(&self) -> bool {
        true
    }
}

// Full Unicode folds can expand one character (for example, ß -> ss).
// Only complete original-character boundaries may become token spans.
fn fold_with_original_offsets(
    input: &str,
) -> (String, HashMap<usize, usize>, HashMap<usize, usize>) {
    let mut folded = String::with_capacity(input.len());
    let mut starts = HashMap::new();
    let mut ends = HashMap::new();
    for (start, ch) in input.char_indices() {
        starts.insert(folded.len(), start);
        for mapped in ch.case_fold() {
            folded.push(mapped);
        }
        ends.insert(folded.len(), start + ch.len_utf8());
    }
    (folded, starts, ends)
}

fn is_token_boundary_match(input: &str, start: usize, end: usize) -> bool {
    !has_identifier_char_before(input, start) && !has_identifier_char_after(input, end)
}

fn has_identifier_char_before(input: &str, start: usize) -> bool {
    has_identifier_component(input[..start].chars().rev())
}

fn has_identifier_char_after(input: &str, end: usize) -> bool {
    has_identifier_component(input[end..].chars())
}

fn has_identifier_component(mut chars: impl Iterator<Item = char>) -> bool {
    match chars.next() {
        // A hyphen connects components only when a base identifier character follows.
        Some('-') => chars.next().is_some_and(is_base_identifier_char),
        Some(ch) => is_base_identifier_char(ch),
        None => false,
    }
}

fn is_base_identifier_char(ch: char) -> bool {
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
    fn full_fold_does_not_create_partial_original_character_spans() {
        let (folded, starts, ends) = fold_with_original_offsets("AßB");
        assert_eq!(folded, "assb");
        assert_eq!(starts.get(&1), Some(&1));
        assert!(!starts.contains_key(&2));
        assert!(!ends.contains_key(&2));
        assert_eq!(ends.get(&3), Some(&3));
    }

    #[test]
    fn record_name_full_fold_detects_original_bytes() {
        let ctx = TypedContext {
            dictionaries: HashMap::from([(
                "record-name".into(),
                ContextDictionary {
                    terms: vec!["JÖRG STRASSE".into()],
                    case_sensitive: true,
                },
            )]),
            class_map: HashMap::new(),
            fields: Map::new(),
        };
        let bundle = dictionary_bundle_from_context(&ctx);
        let detect_context = DetectContext::new(&[LocaleTag::Global], &bundle);
        let recognizer = DictionaryRecognizer::new(
            "context/record-name",
            PiiClass::Name,
            "record-name",
            true,
            "counter",
        )
        .with_unicode_case_insensitive();
        let raw = "Jörg Straße";
        let hits = recognizer.detect(raw, &detect_context).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].span, 0..raw.len());
        assert_eq!(hits[0].canonical_form.as_deref(), Some(raw));
    }

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
    fn changing_record_slot_keeps_only_the_current_automaton() {
        let recognizer = DictionaryRecognizer::new(
            "context/record-slot",
            PiiClass::Name,
            "record-slot",
            true,
            "counter",
        )
        .with_cache_capacity(1);
        for value in ["Alice Smith", "Bob Schmidt"] {
            let context = TypedContext {
                dictionaries: HashMap::from([(
                    "record-slot".to_string(),
                    ContextDictionary {
                        terms: vec![value.to_string()],
                        case_sensitive: true,
                    },
                )]),
                class_map: HashMap::new(),
                fields: Map::new(),
            };
            let bundle = dictionary_bundle_from_context(&context);
            let detect_context = DetectContext::new(&[LocaleTag::Global], &bundle);
            assert_eq!(recognizer.detect(value, &detect_context).unwrap().len(), 1);
            assert_eq!(recognizer.compiled_dictionaries.lock().unwrap().len(), 1);
        }
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

    fn aaa_spans(input: &str) -> Vec<std::ops::Range<usize>> {
        let ctx = TypedContext {
            dictionaries: HashMap::from([(
                "dict_alpha".to_string(),
                ContextDictionary {
                    terms: vec!["AAA".to_string()],
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
        recognizer
            .detect(input, &detect_context)
            .unwrap()
            .into_iter()
            .map(|h| h.span)
            .collect()
    }

    #[test]
    fn dictionary_recognizer_does_not_match_prefix_inside_hyphenated_identifier() {
        let spans = aaa_spans("AAA then AAA-12345");
        assert_eq!(spans, vec![0..3], "expected only standalone AAA");
    }

    #[test]
    fn dictionary_recognizer_matches_term_with_leading_standalone_hyphen() {
        let spans = aaa_spans("-AAA AAA-");
        assert_eq!(
            spans,
            vec![1..4, 5..8],
            "expected both AAA spans in -AAA AAA-"
        );
    }

    #[test]
    fn dictionary_recognizer_does_not_match_suffix_inside_hyphenated_identifier() {
        let spans = aaa_spans("prefix-AAA-suffix");
        assert!(
            spans.is_empty(),
            "expected no match for AAA inside connected identifier, got {spans:?}"
        );
    }
}
