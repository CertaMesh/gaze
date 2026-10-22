use std::ops::Range;
use std::sync::Arc;

use super::error::{NerLoadError, NerRuntimeError};
use super::types::{NerBackendKind, NerSpanResult, VerifiedArtifacts};

mod ort;
mod preparation;
#[cfg(feature = "test-support")]
pub(crate) mod test_support;

#[cfg(test)]
pub(crate) use preparation::{NER_CHUNK_TOKEN_BUDGET, NER_CHUNK_TOKEN_OVERLAP};

/// Driver contract: produce byte-offset detections against a pre-normalized
/// input string. Each backend owns its own model-specific state (label map,
/// id2label for BERT, entity-type list for GLiNER, etc.) — the trait stays
/// shape-agnostic so new backends plug in without changing `NerDetector`.
pub(crate) trait NerBackend: Send + Sync {
    fn possible_classes(&self) -> Vec<gaze_types::PiiClass> {
        vec![gaze_types::PiiClass::Name]
    }

    fn chunk_ranges(&self, input: &str) -> Result<Vec<Range<usize>>, NerRuntimeError> {
        Ok(std::iter::once(0..input.len()).collect())
    }

    fn detect_document(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
        let mut spans = Vec::new();
        for chunk in self.chunk_ranges(input)? {
            spans.extend(
                self.detect(&input[chunk.clone()])?
                    .into_iter()
                    .map(|mut span| {
                        span.span = span.span.start + chunk.start..span.span.end + chunk.start;
                        span
                    }),
            );
        }
        Ok(spans)
    }

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
