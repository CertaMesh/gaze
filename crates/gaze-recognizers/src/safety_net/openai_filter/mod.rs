//! OpenAI Privacy Filter safety-net adapter.
//!
//! Phase 4.1 command choice: Gaze binds to the official `openai/privacy-filter`
//! CLI (`opf --format json --output-mode typed`) installed from a pinned Git
//! revision or an official release. The official CLI has the clearest provenance
//! and documents pipe input plus a stable JSON schema. We intentionally do not
//! use the `chiefautism/privacy-parser` fork by default; it should only become a
//! backend if a later review confirms native byte spans and no PII-bearing JSON
//! fields cross the adapter boundary.
//!
//! The official OPF JSON contains PII-bearing `text` and `placeholder` fields.
//! This module treats those fields as private deserialization details and
//! reduces output to byte offsets, labels, and scores before returning anything
//! to the rest of Gaze.

use std::sync::{Arc, OnceLock};

use gaze_types::{LeakSuspect, LocaleTag, SafetyNet, SafetyNetContext, SafetyNetError};

pub mod backend;
pub mod class_map;

pub use backend::subprocess::{SubprocessOpenAiFilterBackend, SubprocessOpenAiFilterConfig};
pub use backend::{OpenAiFilterBackend, RawSpan};
pub use class_map::{map_openai_label, openai_label_to_safety_net_class, validate_openai_label};

/// Safety-net implementation backed by a lazily initialized OPF subprocess.
///
/// Initialization is cached as `Result<Arc<_>, Arc<SafetyNetError>>`; this is
/// deliberate. Deterministic failures such as missing checkpoint paths are not
/// retried on every safety-net check, which prevents retry storms while still
/// preserving a closed, typed error.
#[derive(Debug)]
pub struct OpenAiFilterSafetyNet {
    locales: Vec<LocaleTag>,
    config: SubprocessOpenAiFilterConfig,
    backend: OnceLock<Result<Arc<SubprocessOpenAiFilterBackend>, Arc<SafetyNetError>>>,
}

impl OpenAiFilterSafetyNet {
    pub fn new(config: SubprocessOpenAiFilterConfig) -> Self {
        Self {
            locales: vec![LocaleTag::Global],
            config,
            backend: OnceLock::new(),
        }
    }

    pub fn from_env() -> Result<Self, SafetyNetError> {
        Ok(Self::new(SubprocessOpenAiFilterConfig::from_env()?))
    }

    pub fn with_locales(mut self, locales: Vec<LocaleTag>) -> Self {
        self.locales = locales;
        self
    }

    fn backend(&self) -> Result<Arc<SubprocessOpenAiFilterBackend>, SafetyNetError> {
        match self.backend.get_or_init(|| {
            SubprocessOpenAiFilterBackend::new(self.config.clone())
                .map(Arc::new)
                .map_err(Arc::new)
        }) {
            Ok(backend) => Ok(Arc::clone(backend)),
            Err(error) => Err((**error).clone()),
        }
    }
}

impl SafetyNet for OpenAiFilterSafetyNet {
    fn id(&self) -> &str {
        "openai-privacy-filter"
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
    backend: &dyn OpenAiFilterBackend,
    context: SafetyNetContext<'_>,
) -> Result<Option<LeakSuspect>, SafetyNetError> {
    let private_label = map_openai_label(&raw.label)?;
    let class = openai_label_to_safety_net_class(private_label).to_pii_class();
    let span = raw.start..raw.end;
    let Some(kind) = context.manifest.diff_against(&span, &class) else {
        return Ok(None);
    };

    Ok(Some(LeakSuspect {
        span,
        class,
        safety_net_id: backend.id().to_string(),
        score: raw.score,
        kind,
        raw_label: raw.label,
        field_path: context.field_path.map(str::to_string),
    }))
}

#[cfg(test)]
mod tests {
    use gaze_types::{DocumentKind, Manifest};

    use super::*;

    #[test]
    fn empty_command_failure_is_cached() {
        let net = OpenAiFilterSafetyNet::new(SubprocessOpenAiFilterConfig::new(""));
        let context = SafetyNetContext {
            manifest: &Manifest::default(),
            locale_chain: &[LocaleTag::Global],
            document_kind: DocumentKind::Text,
            session_id: None,
            field_path: None,
        };

        let first = net.check("clean", context).unwrap_err();
        let second = net.check("clean", context).unwrap_err();
        assert_eq!(first, second);
        assert!(net.backend.get().is_some());
    }
}
