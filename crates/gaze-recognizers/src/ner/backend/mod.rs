use std::sync::Arc;

use super::error::{NerLoadError, NerRuntimeError};
use super::types::{NerBackendKind, NerSpanResult, VerifiedArtifacts};

mod ort;
#[cfg(feature = "test-support")]
pub(crate) mod test_support;

/// Driver contract: produce byte-offset detections against a pre-normalized
/// input string. Each backend owns its own model-specific state (label map,
/// id2label for BERT, entity-type list for GLiNER, etc.) — the trait stays
/// shape-agnostic so new backends plug in without changing `NerDetector`.
pub(crate) trait NerBackend: Send + Sync {
    fn detect(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError>;
}

pub(crate) fn load_backend(
    verified: VerifiedArtifacts,
) -> Result<Arc<dyn NerBackend>, NerLoadError> {
    match verified.backend_kind {
        NerBackendKind::Ort => Ok(Arc::new(ort::OrtBackend::load(
            &verified.model_dir,
            verified.labels,
            verified.id2label,
        )?)),
        NerBackendKind::Gliner => Err(NerLoadError::UnsupportedBackend {
            backend: verified.backend_kind.as_str().to_string(),
        }),
    }
}
