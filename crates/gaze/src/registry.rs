use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::locale::{LocaleChain, LocaleTag};
use crate::redaction_log::ConflictTier;
use crate::resolver::resolve_candidates;
use crate::{DictionaryBundle, PiiClass};

static GLOBAL_LOCALE: [LocaleTag; 1] = [LocaleTag::Global];

pub trait Recognizer: Send + Sync {
    fn id(&self) -> &str;
    fn supported_class(&self) -> &PiiClass;
    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate>;
    fn token_family(&self) -> &str;
    fn locales(&self) -> &[LocaleTag] {
        &GLOBAL_LOCALE
    }
}

pub trait Validator: Send + Sync {
    fn id(&self) -> &str;
    fn validate(&self, raw: &str) -> ValidationResult;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationResult {
    Valid,
    Invalid,
    Indeterminate,
}

pub trait Canonicalizer: Send + Sync {
    fn canonicalize(&self, raw: &str) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub span: Range<usize>,
    pub class: PiiClass,
    pub recognizer_id: String,
    pub score: f32,
    pub priority: i32,
    pub canonical_form: Option<String>,
    pub token_family: String,
    pub source: String,
    pub decided_by: ConflictTier,
    pub merged_sources: Vec<String>,
}

pub struct DetectContext<'a> {
    pub locale_chain: &'a [LocaleTag],
    pub dictionaries: &'a DictionaryBundle,
    pub fields: &'a Map<String, Value>,
    pub degraded: Cell<bool>,
}

pub struct RecognizerRegistry {
    entries: Vec<Arc<dyn Recognizer>>,
    validators: HashMap<String, Arc<dyn Validator>>,
    canonicalizers: HashMap<String, Arc<dyn Canonicalizer>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PiiClass;

    struct StubRecognizer {
        class: PiiClass,
    }

    impl Recognizer for StubRecognizer {
        fn id(&self) -> &str {
            "stub"
        }

        fn supported_class(&self) -> &PiiClass {
            &self.class
        }

        fn detect(&self, _input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
            vec![Candidate {
                span: 0..5,
                class: self.class.clone(),
                recognizer_id: self.id().to_string(),
                score: 1.0,
                priority: 0,
                canonical_form: Some("canonical".to_string()),
                token_family: self.token_family().to_string(),
                source: "test".to_string(),
                decided_by: ConflictTier::None,
                merged_sources: Vec::new(),
            }]
        }

        fn token_family(&self) -> &str {
            "counter"
        }
    }

    #[test]
    fn registry_detect_all_uses_registered_recognizers() {
        let registry = RecognizerRegistry::builder()
            .register(StubRecognizer {
                class: PiiClass::Email,
            })
            .build();
        let dictionaries = DictionaryBundle::default();
        let fields = Map::new();
        let ctx = DetectContext {
            locale_chain: &[LocaleTag::Global],
            dictionaries: &dictionaries,
            fields: &fields,
            degraded: Cell::new(false),
        };

        let candidates = registry.detect_all("input", &ctx);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].class, PiiClass::Email);
        assert_eq!(candidates[0].token_family, "counter");

        let candidates = registry.detect_all_resolved("input", &ctx);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].class, PiiClass::Email);
    }

    #[test]
    fn default_locale_is_global() {
        let recognizer = StubRecognizer {
            class: PiiClass::Email,
        };

        assert_eq!(recognizer.locales(), &[LocaleTag::Global]);
    }

    #[test]
    fn registry_filters_recognizers_by_locale_before_detection() {
        struct LocaleRecognizer {
            locale: LocaleTag,
        }

        impl Recognizer for LocaleRecognizer {
            fn id(&self) -> &str {
                "locale"
            }

            fn supported_class(&self) -> &PiiClass {
                &PiiClass::Email
            }

            fn detect(&self, _input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
                vec![Candidate {
                    span: 0..5,
                    class: PiiClass::Email,
                    recognizer_id: self.id().to_string(),
                    score: 1.0,
                    priority: 0,
                    canonical_form: None,
                    token_family: "counter".to_string(),
                    source: self.id().to_string(),
                    decided_by: ConflictTier::None,
                    merged_sources: Vec::new(),
                }]
            }

            fn token_family(&self) -> &str {
                "counter"
            }

            fn locales(&self) -> &[LocaleTag] {
                std::slice::from_ref(&self.locale)
            }
        }

        let registry = RecognizerRegistry::builder()
            .register(LocaleRecognizer {
                locale: LocaleTag::DeDe,
            })
            .build();
        let dictionaries = DictionaryBundle::default();
        let fields = Map::new();
        let ctx = DetectContext {
            locale_chain: &[LocaleTag::EnUs, LocaleTag::Global],
            dictionaries: &dictionaries,
            fields: &fields,
            degraded: Cell::new(false),
        };

        assert!(registry.detect_all("input", &ctx).is_empty());
    }
}

impl RecognizerRegistry {
    pub fn builder() -> RecognizerRegistryBuilder {
        RecognizerRegistryBuilder::default()
    }

    pub fn detect_all(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        self.entries
            .iter()
            .filter(|recognizer| {
                LocaleChain::from(ctx.locale_chain).intersects(recognizer.locales())
            })
            .flat_map(|recognizer| recognizer.detect(input, ctx))
            .collect()
    }

    pub fn detect_all_resolved(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        let classes = self
            .entries
            .iter()
            .map(|recognizer| recognizer.supported_class().clone())
            .collect::<BTreeSet<_>>();
        let mut candidates = Vec::new();

        for class in classes {
            for locale in ctx.locale_chain {
                let locale_ctx = DetectContext {
                    locale_chain: std::slice::from_ref(locale),
                    dictionaries: ctx.dictionaries,
                    fields: ctx.fields,
                    degraded: Cell::new(ctx.degraded.get()),
                };
                let class_candidates = self
                    .entries
                    .iter()
                    .filter(|recognizer| recognizer.supported_class() == &class)
                    .filter(|recognizer| {
                        LocaleChain::from(locale_ctx.locale_chain).intersects(recognizer.locales())
                    })
                    .flat_map(|recognizer| recognizer.detect(input, &locale_ctx))
                    .filter(|candidate| candidate.score >= min_score(&class))
                    .collect::<Vec<_>>();
                if !class_candidates.is_empty() {
                    candidates.extend(class_candidates);
                    break;
                }
            }
        }

        resolve_candidates(candidates)
    }

    pub fn validators(&self) -> &HashMap<String, Arc<dyn Validator>> {
        &self.validators
    }

    pub fn canonicalizers(&self) -> &HashMap<String, Arc<dyn Canonicalizer>> {
        &self.canonicalizers
    }
}

impl From<&[LocaleTag]> for LocaleChain {
    fn from(tags: &[LocaleTag]) -> Self {
        let mut owned = tags.to_vec();
        if !owned.contains(&LocaleTag::Global) {
            owned.push(LocaleTag::Global);
        }
        LocaleChain::from_tags(owned)
    }
}

fn min_score(_class: &PiiClass) -> f32 {
    0.0
}

#[derive(Default)]
pub struct RecognizerRegistryBuilder {
    entries: Vec<Arc<dyn Recognizer>>,
    validators: HashMap<String, Arc<dyn Validator>>,
    canonicalizers: HashMap<String, Arc<dyn Canonicalizer>>,
}

impl RecognizerRegistryBuilder {
    pub fn register<R: Recognizer + 'static>(mut self, r: R) -> Self {
        self.entries.push(Arc::new(r));
        self
    }

    pub fn register_arc(mut self, r: Arc<dyn Recognizer>) -> Self {
        self.entries.push(r);
        self
    }

    pub fn build(self) -> RecognizerRegistry {
        RecognizerRegistry {
            entries: self.entries,
            validators: self.validators,
            canonicalizers: self.canonicalizers,
        }
    }
}
