use std::collections::BTreeMap;
use std::sync::Arc;

use thiserror::Error;

use crate::detector::{Detection, Detector};
use crate::normalize::normalize;
use crate::rule::{Action, Rule};
use crate::session::Session;
use crate::types::{CleanDocument, RawDocument, Value};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid regex: {0}")]
    InvalidRegex(#[source] regex::Error),
    #[error("session mutex poisoned")]
    SessionPoisoned,
    #[error("unknown token: {0}")]
    UnknownToken(String),
}

#[derive(Clone)]
pub struct Pipeline {
    detectors: Vec<Arc<dyn Detector>>,
    rules: Vec<Arc<dyn Rule>>,
}

impl Pipeline {
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::default()
    }

    pub fn redact(&self, session: &Session, raw: RawDocument) -> Result<CleanDocument> {
        match raw {
            RawDocument::Structured(fields) => redact_structured(self, session, fields),
            RawDocument::Text(text) => Ok(CleanDocument::Text(self.redact_text(session, &text)?)),
        }
    }

    fn redact_text(&self, session: &Session, text: &str) -> Result<String> {
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
        let mut detections = select_winners(detections);
        detections.sort_by_key(|d| std::cmp::Reverse(d.detection.span.start));

        for detection in detections {
            let raw = out[detection.detection.span.clone()].to_string();
            match self.action_for(&detection.detection) {
                Action::Tokenize => {
                    let token = session.tokenize(&detection.detection.class, &raw)?;
                    out.replace_range(detection.detection.span, &token);
                }
                Action::Redact => out.replace_range(detection.detection.span, "[REDACTED]"),
                Action::Preserve => {}
            }
        }

        Ok(out)
    }

    fn action_for(&self, detection: &Detection) -> Action {
        self.rules
            .iter()
            .find_map(|rule| rule.action(&detection.class))
            .unwrap_or(Action::Preserve)
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
    rules: Vec<Arc<dyn Rule>>,
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

    pub fn build(self) -> Result<Pipeline> {
        Ok(Pipeline {
            detectors: self.detectors,
            rules: self.rules,
        })
    }
}

fn redact_structured(
    pipeline: &Pipeline,
    session: &Session,
    fields: BTreeMap<String, Value>,
) -> Result<CleanDocument> {
    let mut clean = BTreeMap::new();
    for (key, value) in fields {
        let value = match value {
            Value::String(text) => serde_json::Value::String(pipeline.redact_text(session, &text)?),
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
        },
        detector_index,
    })
}

fn select_winners(mut detections: Vec<IndexedDetection>) -> Vec<IndexedDetection> {
    detections.sort_by(|a, b| {
        let a_len = a.detection.span.end - a.detection.span.start;
        let b_len = b.detection.span.end - b.detection.span.start;
        b_len
            .cmp(&a_len)
            .then_with(|| a.detector_index.cmp(&b.detector_index))
            .then_with(|| a.detection.span.start.cmp(&b.detection.span.start))
    });

    let mut winners = Vec::new();
    for detection in detections {
        if winners
            .iter()
            .any(|winner: &IndexedDetection| overlaps(&winner.detection.span, &detection.detection.span))
        {
            continue;
        }
        winners.push(detection);
    }

    winners
}

fn overlaps(left: &std::ops::Range<usize>, right: &std::ops::Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}
