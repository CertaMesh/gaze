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

pub mod backend;
pub mod class_map;

pub use backend::subprocess::{
    SubprocessKijiBackend, SubprocessKijiConfig, REQUIRED_KIJI_ARTIFACTS,
};
pub use backend::{KijiDistilbertBackend, RawSpan};
pub use class_map::{
    kiji_label_to_pii_class, kiji_label_to_safety_net_class, map_kiji_label, validate_kiji_label,
    KijiLabel,
};

/// Safety-net implementation backed by a lazily initialized Kiji subprocess.
///
/// Same caching strategy as `OpenAiFilterSafetyNet`: deterministic init
/// failures (missing SHA256SUMS, missing artifacts, bad perms) cache in a
/// `OnceLock` so we never retry-storm on every check.
#[derive(Debug)]
pub struct KijiDistilbertSafetyNet {
    locales: Vec<LocaleTag>,
    config: SubprocessKijiConfig,
    backend: OnceLock<Result<Arc<SubprocessKijiBackend>, Arc<SafetyNetError>>>,
}

impl KijiDistilbertSafetyNet {
    pub fn new(config: SubprocessKijiConfig) -> Self {
        Self {
            locales: vec![LocaleTag::Global],
            config,
            backend: OnceLock::new(),
        }
    }

    pub fn from_env() -> Result<Self, SafetyNetError> {
        Ok(Self::new(SubprocessKijiConfig::from_env()?))
    }

    pub fn with_locales(mut self, locales: Vec<LocaleTag>) -> Self {
        self.locales = locales;
        self
    }

    fn backend(&self) -> Result<Arc<SubprocessKijiBackend>, SafetyNetError> {
        match self.backend.get_or_init(|| {
            SubprocessKijiBackend::new(self.config.clone())
                .map(Arc::new)
                .map_err(Arc::new)
        }) {
            Ok(backend) => Ok(Arc::clone(backend)),
            Err(error) => Err((**error).clone()),
        }
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
