use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gaze_types::{Detection, Detector, RecognizerRuntimeError};

use super::backend::{load_backend, NerBackend};
use super::decode;
use super::error::NerLoadError;
use super::loader::warn_on_label_vocab_mismatch;
use super::types::{LabelMap, NerBackendKind, NerOptions, NerSpanResult};

/// NER detector backed by a pinned local model artifact set. Multiple
/// `NerDetector` instances with different backends may be stacked in the
/// same `Pipeline`; span-conflict resolution picks winners across detectors.
pub struct NerDetector {
    #[allow(dead_code)]
    pub(crate) model_dir: PathBuf,
    pub(crate) backend_kind: NerBackendKind,
    pub(crate) recognizer_version_id: String,
    pub(crate) locale: Option<String>,
    pub(crate) threshold: f32,
    pub(crate) backend: Arc<dyn NerBackend>,
}

impl fmt::Debug for NerDetector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NerDetector")
            .field("model_dir", &self.model_dir)
            .field("backend_kind", &self.backend_kind)
            .field("recognizer_version_id", &self.recognizer_version_id)
            .field("locale", &self.locale)
            .field("threshold", &self.threshold)
            .finish_non_exhaustive()
    }
}

impl NerDetector {
    /// Full load: verify artifacts, initialize the configured backend.
    /// Fails closed on any load error.
    pub fn load(model_dir: &Path) -> Result<Self, NerLoadError> {
        Self::load_with_options(model_dir, NerOptions::default())
    }

    pub fn load_with_options(model_dir: &Path, options: NerOptions) -> Result<Self, NerLoadError> {
        let verified = Self::verify_artifacts(model_dir)?;
        let backend_kind = verified.backend_kind;
        let recognizer_version_id = format!(
            "ner.{}.{}",
            verified.recognizer_model_id, verified.recognizer_model_version
        );
        let model_dir_path = verified.model_dir.clone();
        let label_count = verified.labels.len();
        let id2label_len = verified.id2label.len();
        warn_on_label_vocab_mismatch(&verified.labels, &verified.id2label, model_dir);
        let backend = load_backend(verified)?;

        tracing::info!(
            backend = backend_kind.as_str(),
            recognizer_version_id = %recognizer_version_id,
            labels = label_count,
            id2label_size = id2label_len,
            locale = options.locale.as_deref().unwrap_or(""),
            threshold = options.threshold,
            model_dir = %model_dir_path.display(),
            "ner: detector registered"
        );

        Ok(Self {
            model_dir: model_dir_path,
            backend_kind,
            recognizer_version_id,
            locale: options.locale,
            threshold: options.threshold,
            backend,
        })
    }

    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    pub fn backend_kind(&self) -> NerBackendKind {
        self.backend_kind
    }

    pub fn recognizer_version_id(&self) -> &str {
        &self.recognizer_version_id
    }

    pub(crate) fn detect_span_results(
        &self,
        input: &str,
    ) -> Result<Vec<NerSpanResult>, super::error::NerRuntimeError> {
        let mut spans = Vec::new();
        for chunk in self.backend.chunk_ranges(input)? {
            spans.extend(self.backend.detect(&input[chunk.clone()])?.into_iter().map(
                |mut span| {
                    span.span = span.span.start + chunk.start..span.span.end + chunk.start;
                    span
                },
            ));
        }
        Ok(merge_overlapping_spans(spans)
            .into_iter()
            .filter(|span| decode::is_valid_entity_span(input, &span.span, &span.class))
            .collect())
    }

    /// Label/offset reconstruction helper. Public for testing the BIO merge.
    /// `subword_spans` are byte ranges against `document_text` (the tokenizer
    /// input string), `subword_labels` are CoNLL-style labels per subword
    /// (e.g. `O`, `B-PER`, `I-PER`), and `provenance` is the detector source
    /// id recorded on each `Detection` (e.g. `"ner/ort"`). Returns merged
    /// detections, dropping labels absent from the label map and subword spans
    /// overlapping special tokens (empty ranges). Joiner bridging (`Anne-Marie`,
    /// `john.doe`) reads the bytes between tokens from `document_text`, so it
    /// must be the real text the offsets index into.
    pub fn merge_bio_spans(
        labels: &LabelMap,
        subword_spans: &[(usize, usize)],
        subword_labels: &[&str],
        document_text: &str,
        provenance: &str,
    ) -> Vec<Detection> {
        decode::merge_bio_spans(
            labels,
            subword_spans,
            subword_labels,
            document_text,
            provenance,
        )
    }

    /// Scored variant of [`Self::merge_bio_spans`]; `document_text` has the
    /// same contract (the string the offsets index into).
    pub fn merge_bio_span_results(
        labels: &LabelMap,
        subword_spans: &[(usize, usize)],
        subword_labels: &[&str],
        subword_scores: &[f32],
        document_text: &str,
    ) -> Vec<NerSpanResult> {
        decode::merge_bio_span_results(
            labels,
            subword_spans,
            subword_labels,
            subword_scores,
            document_text,
        )
    }
}

impl Detector for NerDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        self.try_detect(input)
            .expect("ner detector backend failure is fail-closed")
    }

    fn try_detect(&self, input: &str) -> Result<Vec<Detection>, RecognizerRuntimeError> {
        self.detect_span_results(input)
            .map(|detections| {
                detections
                    .into_iter()
                    .map(|span| {
                        Detection::new(
                            span.span,
                            span.class,
                            format!("ner/{}", self.backend_kind.as_str()),
                        )
                    })
                    .collect()
            })
            .map_err(|err| {
                tracing::warn!(backend = self.backend_kind.as_str(), error = %err, "ner: backend detect failed");
                RecognizerRuntimeError::new("ner", err.to_string())
            })
    }
}

fn merge_overlapping_spans(mut spans: Vec<NerSpanResult>) -> Vec<NerSpanResult> {
    spans.sort_by(|left, right| {
        left.span
            .start
            .cmp(&right.span.start)
            .then(left.span.end.cmp(&right.span.end))
            .then(left.class.cmp(&right.class))
    });

    let mut merged: Vec<NerSpanResult> = Vec::new();
    for span in spans {
        if let Some(last) = merged.last_mut() {
            if last.class == span.class && last.span.end >= span.span.start {
                last.span.end = last.span.end.max(span.span.end);
                last.score = last.score.max(span.score);
                continue;
            }
        }
        merged.push(span);
    }
    merged
}
