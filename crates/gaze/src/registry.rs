use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::locale::LocaleTag;
use crate::resolver::resolve_candidates;
use crate::PiiClass;

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
    pub canonical_form: Option<String>,
    pub token_family: String,
    pub source: String,
}

pub struct DetectContext<'a> {
    pub locale_chain: &'a [LocaleTag],
    pub dictionaries: &'a DictionaryBundle,
    pub fields: &'a Map<String, Value>,
    pub degraded: Cell<bool>,
}

#[derive(Debug, Default)]
pub struct DictionaryBundle;

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
                canonical_form: Some("canonical".to_string()),
                token_family: self.token_family().to_string(),
                source: "test".to_string(),
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
        let dictionaries = DictionaryBundle;
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
}

impl RecognizerRegistry {
    pub fn builder() -> RecognizerRegistryBuilder {
        RecognizerRegistryBuilder::default()
    }

    pub fn detect_all(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        self.entries
            .iter()
            .flat_map(|recognizer| recognizer.detect(input, ctx))
            .collect()
    }

    pub fn detect_all_resolved(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        resolve_candidates(self.detect_all(input, ctx))
    }

    pub fn validators(&self) -> &HashMap<String, Arc<dyn Validator>> {
        &self.validators
    }

    pub fn canonicalizers(&self) -> &HashMap<String, Arc<dyn Canonicalizer>> {
        &self.canonicalizers
    }
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
