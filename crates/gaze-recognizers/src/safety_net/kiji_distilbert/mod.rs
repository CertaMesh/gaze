//! Kiji DistilBERT safety-net adapter (v0.8 Tier 2.5).
//!
//! Pass-3 SafetyNet backend that runs a pinned DistilBERT NER model as a
//! subprocess. The contract is observer-only: every label arrives as a
//! validated `KijiLabel`, every span passes through `normalize_raw_spans`,
//! and no PII-bearing upstream field crosses the adapter boundary. This is
//! the second NER opinion at the chokepoint (Axis 1 — defense in depth).
//!
//! The subprocess reads tokenized clean text on stdin and emits a JSON span
//! array on stdout (`[{label, start, end, score?}, ...]`). The `_text` and
//! `_placeholder` fields, if present, are dropped at deserialization with a
//! buffer-scrub on `Drop`.

use std::sync::{Arc, OnceLock};

use gaze_types::{LeakSuspect, LocaleTag, SafetyNet, SafetyNetContext, SafetyNetError};

use crate::{LocaleAwareModel, ModelError, ModelHints, ModelInput, ModelSpan};

pub mod backend;
pub mod class_map;

pub use backend::artifacts::{
    KIJI_DISTILBERT_BUNDLE_SHA256, KIJI_DISTILBERT_HF_COMMIT, KIJI_DISTILBERT_HF_REPO,
    KIJI_DISTILBERT_SHA256SUMS, REQUIRED_KIJI_ARTIFACTS,
};
pub use backend::ort::{OrtKijiBackend, OrtKijiConfig};
pub use backend::subprocess::{SubprocessKijiBackend, SubprocessKijiConfig};
pub use backend::{KijiBackendKind, KijiDistilbertBackend, RawSpan};
pub use class_map::{
    kiji_label_to_pii_class, kiji_label_to_safety_net_class, map_kiji_label, validate_kiji_label,
    KijiLabel,
};

pub type KijiOrtBackend = OrtKijiBackend;
pub type KijiOrtConfig = OrtKijiConfig;

/// Safety-net implementation backed by a lazily initialized Kiji subprocess.
///
/// Same caching strategy as `OpenAiFilterSafetyNet`: deterministic init
/// failures (missing SHA256SUMS, missing artifacts, bad perms) cache in a
/// `OnceLock` so we never retry-storm on every check.
pub struct KijiDistilbertSafetyNet {
    locales: Vec<LocaleTag>,
    config: KijiDistilbertConfig,
    backend: OnceLock<Result<Arc<dyn KijiDistilbertBackend>, Arc<SafetyNetError>>>,
}

impl std::fmt::Debug for KijiDistilbertSafetyNet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KijiDistilbertSafetyNet")
            .field("locales", &self.locales)
            .field("backend_kind", &self.backend_kind())
            .finish_non_exhaustive()
    }
}

impl KijiDistilbertSafetyNet {
    pub fn new(config: impl Into<KijiDistilbertConfig>) -> Self {
        Self {
            locales: vec![LocaleTag::Global],
            config: config.into(),
            backend: OnceLock::new(),
        }
    }

    pub fn from_env() -> Result<Self, SafetyNetError> {
        Ok(Self::new(SubprocessKijiConfig::from_env()?))
    }

    pub fn from_env_ort() -> Result<Self, SafetyNetError> {
        Ok(Self::new(OrtKijiConfig::from_env()?))
    }

    pub fn backend_kind(&self) -> KijiBackendKind {
        self.config.kind()
    }

    pub fn with_locales(mut self, locales: Vec<LocaleTag>) -> Self {
        self.locales = locales;
        self
    }

    fn backend(&self) -> Result<Arc<dyn KijiDistilbertBackend>, SafetyNetError> {
        match self.backend.get_or_init(|| self.config.load_backend()) {
            Ok(backend) => Ok(Arc::clone(backend)),
            Err(error) => Err((**error).clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum KijiDistilbertConfig {
    Subprocess(SubprocessKijiConfig),
    Ort(OrtKijiConfig),
}

impl KijiDistilbertConfig {
    pub fn kind(&self) -> KijiBackendKind {
        match self {
            Self::Subprocess(_) => KijiBackendKind::Subprocess,
            Self::Ort(_) => KijiBackendKind::Ort,
        }
    }

    fn load_backend(&self) -> Result<Arc<dyn KijiDistilbertBackend>, Arc<SafetyNetError>> {
        match self {
            Self::Subprocess(config) => SubprocessKijiBackend::new(config.clone())
                .map(|backend| Arc::new(backend) as Arc<dyn KijiDistilbertBackend>)
                .map_err(Arc::new),
            Self::Ort(config) => OrtKijiBackend::new(config.clone())
                .map(|backend| Arc::new(backend) as Arc<dyn KijiDistilbertBackend>)
                .map_err(Arc::new),
        }
    }
}

impl From<SubprocessKijiConfig> for KijiDistilbertConfig {
    fn from(config: SubprocessKijiConfig) -> Self {
        Self::Subprocess(config)
    }
}

impl From<OrtKijiConfig> for KijiDistilbertConfig {
    fn from(config: OrtKijiConfig) -> Self {
        Self::Ort(config)
    }
}

impl SafetyNet for KijiDistilbertSafetyNet {
    fn id(&self) -> &str {
        "kiji-distilbert"
    }

    fn supported_locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let backend = self.backend()?;
        let raw_spans = backend.infer(clean_text)?;
        raw_spans
            .into_iter()
            .filter_map(|raw| match raw_span_to_suspect(raw, &*backend, context) {
                Ok(Some(suspect)) => Some(Ok(suspect)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }
}

impl LocaleAwareModel for KijiDistilbertSafetyNet {
    fn name(&self) -> &str {
        "kiji-distilbert"
    }

    fn native_locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn infer(&self, input: ModelInput, hints: ModelHints) -> Result<Vec<ModelSpan>, ModelError> {
        let backend = self
            .backend()
            .map_err(|error| ModelError::InitFailed(error.to_string()))?;
        let mut spans = backend
            .infer(&input.text)
            .map_err(|error| ModelError::InferenceFailed(error.to_string()))?
            .into_iter()
            .map(|raw| raw_span_to_model_span(raw, self.name(), &input.text))
            .collect::<Result<Vec<_>, _>>()?;

        if let Some(max_spans) = hints.max_spans {
            spans.truncate(max_spans as usize);
        }

        Ok(spans)
    }
}

fn raw_span_to_model_span(
    raw: RawSpan,
    model_name: &str,
    input: &str,
) -> Result<ModelSpan, ModelError> {
    if raw.start >= raw.end
        || raw.end > input.len()
        || !input.is_char_boundary(raw.start)
        || !input.is_char_boundary(raw.end)
    {
        return Err(ModelError::InferenceFailed(
            "kiji returned out-of-bounds span".to_string(),
        ));
    }

    let label = map_kiji_label(&raw.label)
        .map_err(|error| ModelError::InferenceFailed(error.to_string()))?;
    let class = kiji_label_to_pii_class(label);
    let byte_range = raw.start..raw.end;
    let text = input[byte_range.clone()].to_string();

    Ok(ModelSpan {
        text,
        byte_range,
        class,
        confidence: raw.score,
        model_name: model_name.to_string(),
    })
}

fn raw_span_to_suspect(
    raw: RawSpan,
    backend: &dyn KijiDistilbertBackend,
    context: SafetyNetContext<'_>,
) -> Result<Option<LeakSuspect>, SafetyNetError> {
    let label = map_kiji_label(&raw.label)?;
    let class = kiji_label_to_pii_class(label);
    let span = raw.start..raw.end;
    let Some(kind) = context.manifest.diff_against(&span, &class) else {
        return Ok(None);
    };

    Ok(Some(LeakSuspect::new(
        span,
        class,
        backend.id(),
        raw.score,
        kind,
        raw.label,
        context.field_path.map(str::to_string),
    )))
}

#[cfg(test)]
mod tests {
    use gaze_types::{DocumentKind, Manifest};

    use super::*;

    #[test]
    fn empty_command_failure_is_cached() {
        let net = KijiDistilbertSafetyNet::new(SubprocessKijiConfig::new(""));
        let manifest = Manifest::default();
        let context = SafetyNetContext::new(
            &manifest,
            &[LocaleTag::Global],
            DocumentKind::Text,
            None,
            None,
        );

        let first = net.check("clean", context).unwrap_err();
        let second = net.check("clean", context).unwrap_err();
        assert_eq!(first, second);
        assert!(net.backend.get().is_some());
    }
}
