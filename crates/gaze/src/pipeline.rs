use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use thiserror::Error;

use crate::detector::{Detection, Detector};
use crate::normalize::normalize;
use crate::policy::PolicyError;
use crate::redaction_log::{ConflictTier, DocumentKind, RedactionEntry, RedactionLogger};
use crate::registry::{Candidate, DetectContext, Recognizer, RecognizerRegistry};
use crate::rule::{Action, Rule, RuleContext};
use crate::rulepack::RulepackError;
use crate::session::Session;
use crate::types::{CleanDocument, RawDocument, Value};
use crate::DictionaryBundle;

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
    #[error("invalid snapshot payload")]
    InvalidSnapshotPayload,
    #[error("sqlite error: {0}")]
    Sqlite(String),
    #[error("policy error: {0}")]
    Policy(#[from] PolicyError),
    #[error("rulepack error: {0}")]
    Rulepack(#[from] RulepackError),
}

#[derive(Clone)]
pub struct Pipeline {
    registry: Arc<RecognizerRegistry>,
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

    /// Redacts using the default `[Global]` locale chain.
    ///
    /// Prefer `redact_with_context` when policy, CLI, or rulepack locale
    /// defaults should constrain which recognizers run.
    pub fn redact(&self, session: &Session, raw: RawDocument) -> Result<CleanDocument> {
        let locale_chain = [crate::LocaleTag::Global];
        self.redact_with_context(session, raw, &locale_chain)
    }

    pub fn redact_with_context(
        &self,
        session: &Session,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
    ) -> Result<CleanDocument> {
        let dictionaries = DictionaryBundle::default();
        self.redact_with_detect_context(
            session,
            raw,
            locale_chain,
            &dictionaries,
            empty_detect_fields(),
        )
    }

    pub fn redact_with_detect_context(
        &self,
        session: &Session,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
        detect_fields: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<CleanDocument> {
        match raw {
            RawDocument::Structured(structured_fields) => redact_structured(
                self,
                session,
                structured_fields,
                DocumentKind::Structured,
                locale_chain,
                dictionaries,
                detect_fields,
            ),
            RawDocument::Text(text) => Ok(CleanDocument::Text(self.redact_text(
                session,
                &text,
                None,
                DocumentKind::Text,
                locale_chain,
                dictionaries,
                detect_fields,
            )?)),
        }
    }

    fn redact_text(
        &self,
        session: &Session,
        text: &str,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
        fields: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<String> {
        let mut out = text.to_string();
        let normalized = normalize(text);
        let spans = &normalized.spans;
        let ctx = DetectContext {
            locale_chain,
            dictionaries,
            fields,
            degraded: std::cell::Cell::new(false),
        };
        let resolved = self
            .registry
            .detect_all_resolved(&normalized.text, &ctx)
            .into_iter()
            .filter_map(|candidate| translate_candidate(candidate, spans))
            .collect::<Vec<_>>();
        let losers = merged_losers(&resolved);
        let mut detections = resolved
            .into_iter()
            .map(IndexedDetection::from)
            .collect::<Vec<_>>();
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
                Action::Tokenize => Some(session.tokenize_with_family(
                    &detection.family,
                    &detection.detection.class,
                    &raw,
                )?),
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

    fn action_for(&self, detection: &Detection, context: &RuleContext) -> Action {
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
            decided_by: detection.decided_by,
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
    decided_by: ConflictTier,
    family: String,
}

#[derive(Default)]
pub struct PipelineBuilder {
    recognizers: Vec<Arc<dyn Recognizer>>,
    redaction_loggers: Vec<Arc<dyn RedactionLogger>>,
    rules: Vec<Arc<dyn Rule>>,
}

impl PipelineBuilder {
    pub fn detector<D>(mut self, detector: D) -> Self
    where
        D: Detector + 'static,
    {
        self.recognizers
            .push(Arc::new(DetectorRecognizer::new(detector)));
        self
    }

    pub fn recognizer<R>(mut self, recognizer: R) -> Self
    where
        R: Recognizer + 'static,
    {
        self.recognizers.push(Arc::new(recognizer));
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
        let mut registry = RecognizerRegistry::builder();
        for recognizer in self.recognizers {
            registry = registry.register_arc(recognizer);
        }
        Ok(Pipeline {
            registry: Arc::new(registry.build()),
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
    locale_chain: &[crate::LocaleTag],
    dictionaries: &DictionaryBundle,
    detect_fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<CleanDocument> {
    let mut clean = BTreeMap::new();
    for (key, value) in fields {
        let value = match value {
            Value::String(text) => serde_json::Value::String(pipeline.redact_text(
                session,
                &text,
                Some(&key),
                document_kind.clone(),
                locale_chain,
                dictionaries,
                detect_fields,
            )?),
            Value::I64(value) => serde_json::Value::Number(value.into()),
        };
        clean.insert(key, value);
    }
    Ok(CleanDocument::Structured(clean))
}

fn translate_candidate(candidate: Candidate, spans: &[(usize, usize)]) -> Option<Candidate> {
    translate_span(candidate.span, spans).map(|span| Candidate { span, ..candidate })
}

fn translate_span(
    span: std::ops::Range<usize>,
    spans: &[(usize, usize)],
) -> Option<std::ops::Range<usize>> {
    if span.is_empty() || span.end > spans.len() {
        return None;
    }

    let start = spans[span.start].0;
    let end = spans[span.end - 1].1;
    Some(start..end)
}

fn merged_losers(resolved: &[Candidate]) -> Vec<IndexedDetection> {
    resolved
        .iter()
        .flat_map(|winner| {
            winner.merged_sources.iter().map(|source| IndexedDetection {
                detection: Detection {
                    span: winner.span.clone(),
                    class: winner.class.clone(),
                    source: source.clone(),
                },
                decided_by: if winner.decided_by == ConflictTier::Merged {
                    ConflictTier::Merged
                } else {
                    winner.decided_by
                },
                family: winner.token_family.clone(),
            })
        })
        .collect()
}

impl From<Candidate> for IndexedDetection {
    fn from(candidate: Candidate) -> Self {
        Self {
            detection: Detection {
                span: candidate.span,
                class: candidate.class,
                source: candidate.source,
            },
            decided_by: candidate.decided_by,
            family: candidate.token_family,
        }
    }
}

struct DetectorRecognizer<D> {
    detector: D,
    class: crate::PiiClass,
}

impl<D> DetectorRecognizer<D> {
    fn new(detector: D) -> Self {
        Self {
            detector,
            class: crate::PiiClass::Custom("__legacy_detector__".to_string()),
        }
    }
}

impl<D> Recognizer for DetectorRecognizer<D>
where
    D: Detector + Send + Sync + 'static,
{
    fn id(&self) -> &str {
        "legacy-detector"
    }

    fn supported_class(&self) -> &crate::PiiClass {
        &self.class
    }

    fn detect(&self, input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
        self.detector
            .detect(input)
            .into_iter()
            .map(|detection| {
                let source = detection.source;
                Candidate {
                    span: detection.span,
                    class: detection.class,
                    recognizer_id: source.clone(),
                    score: 1.0,
                    priority: 0,
                    canonical_form: None,
                    token_family: "counter".to_string(),
                    source,
                    decided_by: ConflictTier::None,
                    merged_sources: Vec::new(),
                }
            })
            .collect()
    }

    fn token_family(&self) -> &str {
        "counter"
    }
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

fn build_context(field_name: Option<&str>) -> RuleContext {
    RuleContext {
        field_name: field_name.map(str::to_string),
    }
}

fn empty_detect_fields() -> &'static serde_json::Map<String, serde_json::Value> {
    static EMPTY_FIELDS: OnceLock<serde_json::Map<String, serde_json::Value>> = OnceLock::new();
    EMPTY_FIELDS.get_or_init(serde_json::Map::new)
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
        assert_eq!(loser.decided_by, ConflictTier::SpanLength);
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
                regex::Regex::new(r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b").unwrap(),
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
            CleanDocument::Text(text) => {
                assert!(text.starts_with("Reach <"));
                assert!(text.ends_with(":Email_1> today"));
            }
            other => panic!("expected text output, got {other:?}"),
        }
    }

    #[test]
    fn t21d_token_family_threads_from_recognizer_to_session() {
        struct FamilyRecognizer;

        impl Recognizer for FamilyRecognizer {
            fn id(&self) -> &str {
                "name.alpha"
            }

            fn supported_class(&self) -> &PiiClass {
                &PiiClass::Name
            }

            fn detect(&self, input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
                let Some(start) = input.find("Dr. Schmidt") else {
                    return Vec::new();
                };
                let end = start + "Dr. Schmidt".len();
                vec![Candidate {
                    span: start..end,
                    class: PiiClass::Name,
                    recognizer_id: self.id().to_string(),
                    score: 1.0,
                    priority: 0,
                    canonical_form: None,
                    token_family: self.token_family().to_string(),
                    source: self.id().to_string(),
                    decided_by: ConflictTier::None,
                    merged_sources: Vec::new(),
                }]
            }

            fn token_family(&self) -> &str {
                "alpha"
            }
        }

        let pipeline = Pipeline::builder()
            .recognizer(FamilyRecognizer)
            .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");

        let clean = pipeline
            .redact(
                &session,
                RawDocument::Text("Assigned to Dr. Schmidt".to_string()),
            )
            .expect("redact");
        let CleanDocument::Text(text) = clean else {
            panic!("expected text");
        };
        let token = text
            .strip_prefix("Assigned to ")
            .expect("token prefix")
            .to_string();
        assert!(regex::Regex::new(r"^<[0-9a-f]{8}:Name_\d+>$")
            .unwrap()
            .is_match(&token));

        let beta = session
            .tokenize_with_family("beta", &PiiClass::Name, "Dr. Schmidt")
            .expect("beta token");
        assert_ne!(token, beta);
        assert_eq!(
            session
                .tokenize_with_family("alpha", &PiiClass::Name, "Dr. Schmidt")
                .expect("alpha token"),
            token
        );
        assert_eq!(session.restore(&token).as_deref(), Some("Dr. Schmidt"));
        assert_eq!(session.restore(&beta).as_deref(), Some("Dr. Schmidt"));
    }
}
