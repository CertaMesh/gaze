use std::collections::BTreeMap;
use std::sync::Arc;

use thiserror::Error;

use crate::detector::{Detection, Detector};
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
        let mut detections = self
            .detectors
            .iter()
            .flat_map(|detector| detector.detect(text))
            .collect::<Vec<_>>();
        detections.sort_by_key(|d| std::cmp::Reverse(d.span.start));

        for detection in detections {
            let raw = out[detection.span.clone()].to_string();
            match self.action_for(&detection) {
                Action::Tokenize => {
                    let token = session.tokenize(&detection.class, &raw)?;
                    out.replace_range(detection.span, &token);
                }
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
