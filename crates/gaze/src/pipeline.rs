use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

use crate::detector::{Detection, Detector};
use crate::ner::{NerDetector, NerOptions};
use crate::normalize::normalize;
use crate::redaction_log::{DocumentKind, RedactionEntry, RedactionLogger};
use crate::rule::{Action, Context, Rule};
use crate::session::Session;
use crate::types::{CleanDocument, RawDocument, Value};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid regex: {0}")]
    InvalidRegex(#[source] regex::Error),
    #[error("unknown token: {0}")]
    UnknownToken(String),
    #[error("ephemeral sessions cannot be exported")]
    ExportForbidden,
    #[error("invalid snapshot version: {0}")]
    InvalidSnapshotVersion(u8),
    #[error("snapshot signature verification failed")]
    InvalidSnapshotSignature,
    #[error("snapshot decode failed: {0}")]
    SnapshotDecode(#[source] serde_json::Error),
    #[error("sqlite error: {0}")]
    Sqlite(String),
    #[error("ner load error: {0}")]
    NerLoad(#[source] crate::ner::NerLoadError),
}

#[derive(Clone)]
pub struct Pipeline {
    detectors: Vec<Arc<dyn Detector>>,
    redaction_loggers: Vec<Arc<dyn RedactionLogger>>,
    rules: Vec<Arc<dyn Rule>>,
}

impl Pipeline {
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::default()
    }

    pub fn redact(&self, session: &Session, raw: RawDocument) -> Result<CleanDocument> {
        match raw {
            RawDocument::Structured(fields) => {
                redact_structured(self, session, fields, DocumentKind::Structured)
            }
            RawDocument::Text(text) => Ok(CleanDocument::Text(
                self.redact_text(session, &text, None, DocumentKind::Text)?,
            )),
        }
    }

    fn redact_text(
        &self,
        session: &Session,
        text: &str,
        field_name: Option<&str>,
        document_kind: DocumentKind,
    ) -> Result<String> {
        let mut out = text.to_string();
        let normalized = normalize(text);
        let spans = &normalized.spans;
        let detections = self
            .detectors
            .iter()
            .enumerate()
            .flat_map(|(index, detector)| {
                detector
                    .detect(&normalized.text)
                    .into_iter()
                    .filter_map(move |detection| translate_detection(detection, spans, index))
            })
            .collect::<Vec<_>>();
        let (mut detections, losers) = select_winners(detections);
        for loser in &losers {
            self.log_entry(
                loser,
                field_name,
                document_kind.clone(),
                self.action_for(&loser.detection, &build_context(field_name)),
                true,
            )?;
        }

        detections.sort_by_key(|d| d.detection.span.start);
        let mut replacements = Vec::with_capacity(detections.len());

        for detection in detections {
            let raw = text[detection.detection.span.clone()].to_string();
            let context = build_context(field_name);
            let action = self.action_for(&detection.detection, &context);
            self.log_entry(
                &detection,
                field_name,
                document_kind.clone(),
                action,
                false,
            )?;

            let replacement = match action {
                Action::Tokenize => Some(session.tokenize(&detection.detection.class, &raw)?),
                Action::Redact => Some("[REDACTED]".to_string()),
                Action::FormatPreserve => Some(
                    session.format_preserving_fake(&detection.detection.class, &raw)?,
                ),
                Action::Generalize => Some(generalize_token(&detection.detection.class)),
                Action::Preserve => None,
            };
            replacements.push((detection.detection.span, replacement));
        }

        for (span, replacement) in replacements.into_iter().rev() {
            if let Some(replacement) = replacement {
                out.replace_range(span, &replacement);
            }
        }

        Ok(out)
    }

    fn action_for(&self, detection: &Detection, context: &Context) -> Action {
        self.rules
            .iter()
            .find_map(|rule| rule.action(&detection.class, context))
            .unwrap_or(Action::Preserve)
    }

    fn log_entry(
        &self,
        detection: &IndexedDetection,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        action: Action,
        conflict_loser: bool,
    ) -> Result<()> {
        let entry = RedactionEntry {
            source: detection.detection.source.clone(),
            class: detection.detection.class.clone(),
            action,
            field_name: field_name.map(str::to_string),
            document_kind,
            conflict_loser,
        };

        for logger in &self.redaction_loggers {
            logger.log(&entry)?;
        }

        Ok(())
    }
}

#[derive(Clone)]
struct IndexedDetection {
    detection: Detection,
    detector_index: usize,
}

#[derive(Default)]
pub struct PipelineBuilder {
    detectors: Vec<Arc<dyn Detector>>,
    redaction_loggers: Vec<Arc<dyn RedactionLogger>>,
    rules: Vec<Arc<dyn Rule>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NerConfig {
    pub model_dir: Option<PathBuf>,
    pub locale: Option<String>,
}

impl PipelineBuilder {
    pub fn detector<D>(mut self, detector: D) -> Self
    where
        D: Detector + 'static,
    {
        self.detectors.push(Arc::new(detector));
        self
    }

    pub fn rule<R>(mut self, rule: R) -> Self
    where
        R: Rule + 'static,
    {
        self.rules.push(Arc::new(rule));
        self
    }

    pub fn redaction_logger<L>(mut self, logger: L) -> Self
    where
        L: RedactionLogger + 'static,
    {
        self.redaction_loggers.push(Arc::new(logger));
        self
    }

    /// Wire up the NER detector from a resolved model directory.
    ///
    /// Fail-closed defaults:
    /// - `None` or empty path → NER is disabled, a `tracing::warn!` is emitted,
    ///   and the pipeline still builds with regex + index detectors intact.
    /// - `Some(path)` → `NerDetector::load` must succeed; any load error
    ///   propagates as `Error::NerLoad` and the pipeline does NOT build.
    ///
    /// No runtime downloads. Caller resolves the model directory from
    /// `[ner] model_dir` in `policy.toml` or the
    /// `${XDG_DATA_HOME:-~/.local/share}/gaze/models/davlan-mbert-ner-hrl/` default.
    pub fn with_ner_model_dir(self, model_dir: Option<&Path>) -> Result<Self> {
        self.with_ner_config(NerConfig {
            model_dir: model_dir.map(Path::to_path_buf),
            locale: None,
        })
    }

    pub fn with_ner_config(self, config: NerConfig) -> Result<Self> {
        let NerConfig { model_dir, locale } = config;
        match model_dir {
            None => {
                tracing::warn!(
                    "ner: no [ner] model_dir configured; transformer NER disabled, regex + index detectors still run"
                );
                Ok(self)
            }
            Some(path) if path.as_os_str().is_empty() => {
                tracing::warn!(
                    "ner: [ner] model_dir is empty; transformer NER disabled, regex + index detectors still run"
                );
                Ok(self)
            }
            Some(path) => {
                let detector = NerDetector::load_with_options(
                    &path,
                    NerOptions { locale },
                )
                .map_err(Error::NerLoad)?;
                Ok(self.detector(detector))
            }
        }
    }

    pub fn build(self) -> Result<Pipeline> {
        Ok(Pipeline {
            detectors: self.detectors,
            redaction_loggers: self.redaction_loggers,
            rules: self.rules,
        })
    }
}

fn redact_structured(
    pipeline: &Pipeline,
    session: &Session,
    fields: BTreeMap<String, Value>,
    document_kind: DocumentKind,
) -> Result<CleanDocument> {
    let mut clean = BTreeMap::new();
    for (key, value) in fields {
        let value = match value {
            Value::String(text) => serde_json::Value::String(
                pipeline.redact_text(session, &text, Some(&key), document_kind.clone())?,
            ),
            Value::I64(value) => serde_json::Value::Number(value.into()),
        };
        clean.insert(key, value);
    }
    Ok(CleanDocument::Structured(clean))
}

fn translate_detection(
    detection: Detection,
    spans: &[(usize, usize)],
    detector_index: usize,
) -> Option<IndexedDetection> {
    if detection.span.is_empty() || detection.span.end > spans.len() {
        return None;
    }

    let start = spans[detection.span.start].0;
    let end = spans[detection.span.end - 1].1;
    Some(IndexedDetection {
        detection: Detection {
            span: start..end,
            class: detection.class,
            source: detection.source,
        },
        detector_index,
    })
}

fn select_winners(mut detections: Vec<IndexedDetection>) -> (Vec<IndexedDetection>, Vec<IndexedDetection>) {
    detections.sort_by(|a, b| {
        let a_len = a.detection.span.end - a.detection.span.start;
        let b_len = b.detection.span.end - b.detection.span.start;
        b_len
            .cmp(&a_len)
            .then_with(|| a.detector_index.cmp(&b.detector_index))
            .then_with(|| a.detection.span.start.cmp(&b.detection.span.start))
    });

    let mut winners = Vec::new();
    let mut losers = Vec::new();
    for detection in detections {
        if winners
            .iter()
            .any(|winner: &IndexedDetection| overlaps(&winner.detection.span, &detection.detection.span))
        {
            losers.push(detection);
            continue;
        }
        winners.push(detection);
    }

    (winners, losers)
}

fn overlaps(left: &std::ops::Range<usize>, right: &std::ops::Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn generalize_token(class: &crate::detector::PiiClass) -> String {
    match class {
        crate::detector::PiiClass::Email => "[EMAIL]".to_string(),
        crate::detector::PiiClass::Name => "[NAME]".to_string(),
        crate::detector::PiiClass::Location => "[LOCATION]".to_string(),
        crate::detector::PiiClass::Organization => "[ORGANIZATION]".to_string(),
        crate::detector::PiiClass::Custom(name) => format!("[{}]", name.to_ascii_uppercase()),
    }
}

fn build_context(field_name: Option<&str>) -> Context {
    Context {
        field_name: field_name.map(str::to_string),
    }
}
