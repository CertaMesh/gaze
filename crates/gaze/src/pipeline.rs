use std::collections::BTreeMap;
use std::sync::Arc;

use thiserror::Error;

use crate::detector::{Detection, Detector};
use crate::normalize::normalize;
use crate::policy::PolicyError;
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
    #[error("snapshot expired: issued_at={issued_at}, ttl_secs={ttl_secs}")]
    BlobExpired { issued_at: u64, ttl_secs: u64 },
    #[error("snapshot decode failed: {0}")]
    SnapshotDecode(#[source] serde_json::Error),
    #[error("sqlite error: {0}")]
    Sqlite(String),
    #[error("policy error: {0}")]
    Policy(#[from] PolicyError),
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

    pub fn with_redaction_logger<L>(mut self, logger: L) -> Pipeline
    where
        L: RedactionLogger + 'static,
    {
        self.redaction_loggers.push(Arc::new(logger));
        self
    }

    pub fn redact(&self, session: &Session, raw: RawDocument) -> Result<CleanDocument> {
        match raw {
            RawDocument::Structured(fields) => {
                redact_structured(self, session, fields, DocumentKind::Structured)
            }
            RawDocument::Text(text) => Ok(CleanDocument::Text(self.redact_text(
                session,
                &text,
                None,
                DocumentKind::Text,
            )?)),
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
            self.log_entry(&detection, field_name, document_kind.clone(), action, false)?;

            let replacement = match action {
                Action::Tokenize => Some(session.tokenize(&detection.detection.class, &raw)?),
                Action::Redact => Some("[REDACTED]".to_string()),
                Action::FormatPreserve => {
                    Some(session.format_preserving_fake(&detection.detection.class, &raw)?)
                }
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
            Value::String(text) => serde_json::Value::String(pipeline.redact_text(
                session,
                &text,
                Some(&key),
                document_kind.clone(),
            )?),
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

fn select_winners(
    mut detections: Vec<IndexedDetection>,
) -> (Vec<IndexedDetection>, Vec<IndexedDetection>) {
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
        if winners.iter().any(|winner: &IndexedDetection| {
            overlaps(&winner.detection.span, &detection.detection.span)
        }) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::{Detection, PiiClass};
    use crate::rule::{ClassRule, DefaultRule};
    use crate::session::{Scope, Session};
    use std::sync::Mutex;

    /// Shared-handle test double: callers keep an `Arc<Mutex<Vec<_>>>` and
    /// clone it into the logger, letting the builder take ownership while
    /// the test retains read access.
    struct CapturingLogger {
        entries: Arc<Mutex<Vec<RedactionEntry>>>,
    }

    struct FixedDetector {
        detections: Vec<Detection>,
    }

    impl Detector for FixedDetector {
        fn detect(&self, _input: &str) -> Vec<Detection> {
            self.detections.clone()
        }
    }

    fn detector_with_detections(source: &str, detections: Vec<Detection>) -> FixedDetector {
        FixedDetector {
            detections: detections
                .into_iter()
                .map(|mut detection| {
                    detection.source = source.to_string();
                    detection
                })
                .collect(),
        }
    }

    impl RedactionLogger for CapturingLogger {
        fn log(&self, entry: &RedactionEntry) -> Result<()> {
            self.entries.lock().unwrap().push(entry.clone());
            Ok(())
        }
    }

    #[test]
    fn stacked_ner_detectors_resolve_via_span_conflict() {
        // Input: "Alice Smith works here" — byte spans: Alice=0..5, full name=0..11.
        let text = "Alice Smith works here";
        let short_detection = Detection {
            span: 0..5,
            class: PiiClass::Name,
            source: "ner/bert".to_string(),
        };
        let long_detection = Detection {
            span: 0..11,
            class: PiiClass::Name,
            source: "ner/gliner".to_string(),
        };

        let bert = detector_with_detections("ner/bert", vec![short_detection]);
        let gliner = detector_with_detections("ner/gliner", vec![long_detection]);

        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));

        let pipeline = Pipeline::builder()
            .detector(bert)
            .detector(gliner)
            .rule(ClassRule::new(PiiClass::Name, Action::Redact))
            .rule(DefaultRule::new(Action::Preserve))
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .build()
            .expect("pipeline");

        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = pipeline
            .redact(&session, RawDocument::Text(text.to_string()))
            .expect("redact");

        let out = match clean {
            CleanDocument::Text(t) => t,
            _ => panic!("expected text"),
        };

        // Longer span wins: full name replaced, trailing " works here" preserved.
        assert_eq!(out, "[REDACTED] works here");

        let entries = entries.lock().unwrap();
        assert_eq!(
            entries.len(),
            2,
            "expected one winner + one loser: {entries:?}"
        );
        let winner = entries.iter().find(|e| !e.conflict_loser).expect("winner");
        let loser = entries.iter().find(|e| e.conflict_loser).expect("loser");
        assert_eq!(winner.source, "ner/gliner", "longer span should win");
        assert_eq!(loser.source, "ner/bert", "shorter span should lose");
    }

    #[test]
    fn stacked_detectors_both_win_when_spans_disjoint() {
        let text = "Alice visited Berlin";
        let alice = Detection {
            span: 0..5,
            class: PiiClass::Name,
            source: "ner/bert".to_string(),
        };
        let berlin = Detection {
            span: 14..20,
            class: PiiClass::Location,
            source: "ner/gliner".to_string(),
        };

        let bert = detector_with_detections("ner/bert", vec![alice]);
        let gliner = detector_with_detections("ner/gliner", vec![berlin]);

        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));

        let pipeline = Pipeline::builder()
            .detector(bert)
            .detector(gliner)
            .rule(ClassRule::new(PiiClass::Name, Action::Redact))
            .rule(ClassRule::new(PiiClass::Location, Action::Redact))
            .rule(DefaultRule::new(Action::Preserve))
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .build()
            .expect("pipeline");

        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = pipeline
            .redact(&session, RawDocument::Text(text.to_string()))
            .expect("redact");

        let out = match clean {
            CleanDocument::Text(t) => t,
            _ => panic!("expected text"),
        };

        assert_eq!(out, "[REDACTED] visited [REDACTED]");
        let entries = entries.lock().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| !e.conflict_loser));
    }

    #[test]
    fn pipeline_builder_detects_email() {
        struct EmailDetector(regex::Regex);

        impl Detector for EmailDetector {
            fn detect(&self, input: &str) -> Vec<Detection> {
                self.0
                    .find_iter(input)
                    .map(|m| Detection {
                        span: m.range(),
                        class: PiiClass::Email,
                        source: "regex".to_string(),
                    })
                    .collect()
            }
        }

        let pipeline = Pipeline::builder()
            .detector(EmailDetector(
                regex::Regex::new(r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b")
                    .unwrap(),
            ))
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();

        let clean = pipeline
            .redact(
                &session,
                RawDocument::Text("Reach alice@example.com today".to_string()),
            )
            .unwrap();

        match clean {
            CleanDocument::Text(text) => assert_eq!(text, "Reach <Email_1> today"),
            other => panic!("expected text output, got {other:?}"),
        }
    }
}
