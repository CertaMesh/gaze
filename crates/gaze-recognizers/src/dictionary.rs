use gaze_types::{Candidate, ConflictTier, DetectContext, LocaleTag, PiiClass, Recognizer};

pub struct DictionaryRecognizer {
    id: String,
    class: PiiClass,
    dictionary_name: String,
    case_sensitive: bool,
    token_family: String,
    locales: Vec<LocaleTag>,
    score: f32,
    priority: i32,
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
            score,
            priority,
        }
    }

    pub fn dictionary_name(&self) -> &str {
        &self.dictionary_name
    }

    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }
}

impl Recognizer for DictionaryRecognizer {
    fn id(&self) -> &str {
        &self.id
    }

    fn supported_class(&self) -> &PiiClass {
        &self.class
    }

    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        let Some(entry) = ctx.dictionaries.get(&self.dictionary_name) else {
            return Vec::new();
        };
        let haystack = if self.case_sensitive {
            None
        } else {
            Some(input.to_ascii_lowercase())
        };
        let search_input = haystack.as_deref().unwrap_or(input);

        entry
            .terms()
            .iter()
            .enumerate()
            .flat_map(|(pattern_index, term)| {
                let needle = if self.case_sensitive {
                    term.clone()
                } else {
                    term.to_ascii_lowercase()
                };
                search_input
                    .match_indices(&needle)
                    .map(move |(start, matched)| (pattern_index, start..start + matched.len()))
                    .collect::<Vec<_>>()
            })
            .map(|(pattern_index, span)| Candidate {
                canonical_form: Some(input[span.clone()].to_string()),
                span,
                class: self.class.clone(),
                recognizer_id: self.id.clone(),
                score: self.score,
                priority: self.priority,
                token_family: self.token_family.clone(),
                source: format!("dictionary:{}[#{}]", self.dictionary_name, pattern_index),
                decided_by: ConflictTier::None,
                merged_sources: Vec::new(),
            })
            .collect()
    }

    fn token_family(&self) -> &str {
        &self.token_family
    }

    fn locales(&self) -> &[LocaleTag] {
        &self.locales
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
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
        let fields = ();
        let detect_context = DetectContext {
            locale_chain: &[LocaleTag::Global],
            dictionaries: &bundle,
            fields: &fields,
            degraded: Cell::new(false),
        };
        let recognizer = DictionaryRecognizer::new(
            "dict/dict_alpha",
            PiiClass::Custom("class_alpha".to_string()),
            "dict_alpha",
            true,
            "counter",
        );

        let hits = recognizer.detect("Customer bought AAA-12345", &detect_context);
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
        let fields = ();
        let detect_context = DetectContext {
            locale_chain: &[LocaleTag::EnUs],
            dictionaries: &bundle,
            fields: &fields,
            degraded: Cell::new(false),
        };
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
        let hits = registry.detect_all("Listening to bohemian rhapsody", &detect_context);
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
        let fields = ();
        let detect_context = DetectContext {
            locale_chain: &[LocaleTag::Global],
            dictionaries: &bundle,
            fields: &fields,
            degraded: Cell::new(false),
        };
        let recognizer = DictionaryRecognizer::new(
            "dict/songs",
            PiiClass::Custom("song".to_string()),
            "songs",
            true,
            "counter",
        );

        let hits = recognizer.detect(
            "first alpha-one, second bravo-two, third charlie-three",
            &detect_context,
        );

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
}
