use std::ops::Range;

use gaze_types::{LocaleTag, PiiClass};
use thiserror::Error;

/// Locale-aware model backend for Pass-2 NER and Pass-3 safety-net inference.
pub trait LocaleAwareModel: Send + Sync {
    /// Stable backend name used in diagnostics and audit trails.
    fn name(&self) -> &str;

    /// Locales this backend natively supports.
    fn native_locales(&self) -> &[LocaleTag];

    /// Run inference for the supplied text and locale hints.
    fn infer(&self, input: ModelInput, hints: ModelHints) -> Result<Vec<ModelSpan>, ModelError>;
}

/// Input passed to a locale-aware model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInput {
    pub text: String,
    pub locale: LocaleTag,
}

/// Execution hints supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelHints {
    pub stage: ModelStage,
    pub max_spans: Option<u32>,
}

/// Pipeline stage requesting model inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStage {
    Pass2Ner,
    Pass3SafetyNet,
}

/// Span emitted by a locale-aware model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSpan {
    pub text: String,
    pub byte_range: Range<usize>,
    pub class: PiiClass,
    pub confidence: Option<f32>,
    pub model_name: String,
}

/// Closed error set for locale-aware model routing and inference.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelError {
    #[error("backend init failed: {0}")]
    InitFailed(String),
    #[error("inference failed: {0}")]
    InferenceFailed(String),
    #[error("locale not supported: {0:?}")]
    LocaleNotSupported(LocaleTag),
    #[error("model integrity mismatch")]
    IntegrityMismatch,
    #[error("no locale model coverage for {locale:?} at {stage:?}")]
    NoLocaleModelCoverage {
        locale: LocaleTag,
        stage: ModelStage,
    },
    #[error("backend internal error: {0}")]
    Internal(String),
}

/// Registry that resolves locale-aware model backends by locale fallback tier.
#[derive(Default)]
pub struct LocaleAwareModelRegistry {
    backends: Vec<Box<dyn LocaleAwareModel>>,
}

impl LocaleAwareModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_backends(backends: Vec<Box<dyn LocaleAwareModel>>) -> Self {
        Self { backends }
    }

    pub fn register(&mut self, backend: impl LocaleAwareModel + 'static) {
        self.backends.push(Box::new(backend));
    }

    /// Resolve models by four tiers:
    /// exact locale, parent language, multilingual `Global`, then fail-closed.
    pub fn resolve(
        &self,
        locale: &LocaleTag,
        stage: ModelStage,
    ) -> Result<Vec<&dyn LocaleAwareModel>, ModelError> {
        if let Some(matches) = self.matches_for(locale) {
            return Ok(matches);
        }

        if let Some(parent) = parent_language(locale) {
            if let Some(matches) = self.matches_for(&parent) {
                return Ok(matches);
            }
        }

        if let Some(matches) = self.matches_for(&LocaleTag::Global) {
            return Ok(matches);
        }

        Err(ModelError::NoLocaleModelCoverage {
            locale: locale.clone(),
            stage,
        })
    }

    fn matches_for(&self, locale: &LocaleTag) -> Option<Vec<&dyn LocaleAwareModel>> {
        let matches: Vec<&dyn LocaleAwareModel> = self
            .backends
            .iter()
            .filter(|backend| backend.native_locales().contains(locale))
            .map(|backend| backend.as_ref())
            .collect();

        (!matches.is_empty()).then_some(matches)
    }
}

fn parent_language(locale: &LocaleTag) -> Option<LocaleTag> {
    match locale {
        LocaleTag::DeDe | LocaleTag::DeAt | LocaleTag::DeCh => {
            Some(LocaleTag::Other("de".to_string()))
        }
        LocaleTag::EnUs | LocaleTag::EnGb | LocaleTag::EnIe | LocaleTag::EnAu | LocaleTag::EnCa => {
            Some(LocaleTag::Other("en".to_string()))
        }
        LocaleTag::Other(raw) => raw
            .split_once('-')
            .map(|(language, _)| LocaleTag::Other(language.to_ascii_lowercase())),
        LocaleTag::Global => None,
        _ => None,
    }
}
