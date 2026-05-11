use std::collections::BTreeMap;
use std::sync::Arc;

use gaze_types::{
    EmittedTokenSpan, LeakReport, LeakReportTelemetry, LeakSuspect, Manifest, RedactionLogError,
    RedactionLogger, SafetyNet, SafetyNetContext, SafetyNetError,
};
use thiserror::Error;

use crate::detector::{Detection, Detector, PiiClass};
use crate::normalize::normalize;
use crate::policy::PolicyError;
use crate::redaction_log::{ConflictTier, DocumentKind, RedactionEntry};
use crate::registry::{Candidate, DetectContext, Recognizer, RecognizerRegistry};
use crate::rule::{Action, Rule, RuleContext};
use crate::rulepack::RulepackError;
use crate::session::Session;
use crate::types::{CleanDocument, RawDocument, Value};
use crate::DictionaryBundle;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid regex: {0}")]
    InvalidRegex(#[source] regex::Error),
    #[error("unknown token: {0}")]
    UnknownToken(String),
    #[error("ephemeral sessions cannot be exported")]
    ExportForbidden,
    #[error("document extension integrity fields cannot be empty")]
    EmptyDocumentIntegrity,
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
    #[error("safety net error: {0}")]
    SafetyNet(#[from] SafetyNetError),
    #[error("redaction log error: {0}")]
    RedactionLog(#[from] RedactionLogError),
    #[error("unsupported raw document variant")]
    UnsupportedRawDocumentVariant,
    #[error("unsupported structured value variant")]
    UnsupportedValueVariant,
    #[error("unsupported policy action variant")]
    UnsupportedActionVariant,
}

/// The stateless PII pseudonymization engine.
///
/// `Pipeline` owns the recognizer registry, rule set, locale resolver, and optional audit logger.
/// Construct once per process and share across requests; create one [`Session`] per conversation
/// or request.
///
/// # Fail-closed
///
/// Construction fails if a recognizer fails to initialize, a validator name is unknown, or a policy
/// cannot be parsed. There is no silent degradation to a weaker detection posture.
///
/// # Thread safety
///
/// `Pipeline` is `Send + Sync`. Share across threads; create one [`Session`] per request.
///
/// # Quick example
///
/// ```rust,no_run
/// use gaze::{CleanDocument, Pipeline, RawDocument, Scope, Session};
///
/// let pipeline = Pipeline::builder().build()?;
/// let session = Session::new(Scope::Ephemeral)?;
/// let CleanDocument::Text(clean) = pipeline.redact(
///     &session,
///     RawDocument::Text("test".into()),
/// )? else {
///     panic!("expected text variant");
/// };
/// # let _ = clean;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone)]
pub struct Pipeline {
    registry: Arc<RecognizerRegistry>,
    redaction_loggers: Vec<Arc<dyn RedactionLogger>>,
    safety_nets: Vec<Arc<dyn SafetyNet>>,
    rules: Vec<Arc<dyn Rule>>,
}

/// Observer-only safety-net scan result.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SafetyNetResult {
    /// Number of safety nets registered for this pipeline.
    pub nets_run: usize,
    /// Metadata-only leak report aggregated across scanned leaves.
    pub report: LeakReport,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline").finish_non_exhaustive()
    }
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

    pub fn with_safety_net<N>(mut self, safety_net: N) -> Pipeline
    where
        N: SafetyNet + 'static,
    {
        self.safety_nets.push(Arc::new(safety_net));
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
        self.redact_with_detect_context(session, raw, locale_chain, &dictionaries)
    }

    pub fn redact_with_detect_context(
        &self,
        session: &Session,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<CleanDocument> {
        match raw {
            RawDocument::Structured(structured_fields) => redact_structured(
                self,
                session,
                structured_fields,
                DocumentKind::Structured,
                locale_chain,
                dictionaries,
            ),
            RawDocument::Text(text) => Ok(CleanDocument::Text(self.redact_text(
                session,
                &text,
                None,
                DocumentKind::Text,
                locale_chain,
                dictionaries,
            )?)),
            _ => Err(Error::UnsupportedRawDocumentVariant),
        }
    }

    pub fn clean_with_safety_net(
        &self,
        session: &Session,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
    ) -> Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)> {
        let dictionaries = DictionaryBundle::default();
        self.clean_with_safety_net_detect_context(session, raw, locale_chain, &dictionaries)
    }

    pub fn clean_with_safety_net_detect_context(
        &self,
        session: &Session,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)> {
        match raw {
            RawDocument::Structured(structured_fields) => {
                let mut report = LeakReport::default();
                let clean = redact_structured_with_safety_net(
                    self,
                    session,
                    structured_fields,
                    locale_chain,
                    dictionaries,
                    &mut report,
                )?;
                Ok((CleanDocument::Structured(clean), Vec::new(), report))
            }
            RawDocument::Text(text) => {
                let clean = self.redact_text_with_manifest(
                    session,
                    &text,
                    None,
                    DocumentKind::Text,
                    locale_chain,
                    dictionaries,
                )?;
                let report = self.run_safety_nets(
                    session,
                    &clean.text,
                    &Manifest::from_spans(clean.manifest.clone()),
                    DocumentKind::Text,
                    locale_chain,
                    None,
                )?;
                Ok((CleanDocument::Text(clean.text), clean.manifest, report))
            }
            _ => Err(Error::UnsupportedRawDocumentVariant),
        }
    }

    pub fn scan_safety_nets(
        &self,
        session: &Session,
        clean_text: &str,
        locale_chain: &[crate::LocaleTag],
    ) -> Result<SafetyNetResult> {
        let nets_run = self.safety_nets.len();
        if nets_run == 0 {
            return Ok(SafetyNetResult {
                nets_run,
                report: LeakReport::default(),
            });
        }

        let report = self.run_safety_nets(
            session,
            clean_text,
            &Manifest::default(),
            DocumentKind::Text,
            locale_chain,
            None,
        )?;
        Ok(SafetyNetResult { nets_run, report })
    }

    pub fn scan_safety_nets_structured(
        &self,
        session: &Session,
        document: &BTreeMap<String, Value>,
        locale_chain: &[crate::LocaleTag],
    ) -> Result<SafetyNetResult> {
        let nets_run = self.safety_nets.len();
        if nets_run == 0 {
            return Ok(SafetyNetResult {
                nets_run,
                report: LeakReport::default(),
            });
        }

        let mut report = LeakReport::default();
        for (key, value) in document {
            walk_value_for_safety_net_scan(self, session, value, key, locale_chain, &mut report)?;
        }
        Ok(SafetyNetResult { nets_run, report })
    }

    #[allow(clippy::too_many_arguments)]
    fn redact_text(
        &self,
        session: &Session,
        text: &str,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<String> {
        Ok(self
            .redact_text_with_manifest(
                session,
                text,
                field_name,
                document_kind,
                locale_chain,
                dictionaries,
            )?
            .text)
    }

    #[allow(clippy::too_many_arguments)]
    fn redact_text_with_manifest(
        &self,
        session: &Session,
        text: &str,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<CleanText> {
        let normalized = normalize(text);
        let spans = &normalized.spans;
        let ctx = DetectContext::new(locale_chain, dictionaries);
        let (resolved, vetoed) = self
            .registry
            .detect_all_resolved(&normalized.text, &ctx);
        let vetoed = vetoed
            .into_iter()
            .filter_map(|vetoed| translate_vetoed_candidate(vetoed, spans))
            .collect::<Vec<_>>();
        let resolved = resolved
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
                session,
                loser,
                field_name,
                document_kind,
                self.action_for(&loser.detection, &build_context(field_name)),
                true,
            )?;
        }
        for vetoed in &vetoed {
            self.log_vetoed_entry(session, vetoed, field_name, document_kind)?;
        }

        detections.sort_by_key(|d| d.detection.span.start);
        let mut out = String::with_capacity(text.len());
        let mut emitted = Vec::with_capacity(detections.len());
        let mut cursor = 0usize;

        for detection in detections {
            let raw = text[detection.detection.span.clone()].to_string();
            let context = build_context(field_name);
            let action = self.action_for(&detection.detection, &context);
            self.log_entry(
                session,
                &detection,
                field_name,
                document_kind,
                action,
                false,
            )?;

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
                _ => return Err(Error::UnsupportedActionVariant),
            };

            let span = detection.detection.span;
            if span.start > cursor {
                out.push_str(&text[cursor..span.start]);
            }
            match replacement {
                Some(replacement) => {
                    let clean_start = out.len();
                    out.push_str(&replacement);
                    emitted.push(EmittedTokenSpan::new(
                        clean_start..out.len(),
                        span.clone(),
                        detection.detection.class,
                    ));
                }
                None => out.push_str(&text[span.clone()]),
            }
            cursor = span.end;
        }

        if cursor < text.len() {
            out.push_str(&text[cursor..]);
        }

        Ok(CleanText {
            text: out,
            manifest: emitted,
        })
    }

    fn run_safety_nets(
        &self,
        session: &Session,
        clean_text: &str,
        manifest: &Manifest,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        field_path: Option<&str>,
    ) -> Result<LeakReport> {
        if self.safety_nets.is_empty() {
            return Ok(LeakReport::default());
        }

        let mut suspects = Vec::<LeakSuspect>::new();
        let mut telemetry = Vec::new();
        let active = gaze_types::LocaleChain::from(locale_chain);
        for net in &self.safety_nets {
            if !active.intersects(net.supported_locales()) {
                telemetry.push(LeakReportTelemetry::LocaleSkipped {
                    safety_net_id: net.id().to_string(),
                    document_kind,
                    field_path: field_path.map(str::to_string),
                });
                continue;
            }

            let context = SafetyNetContext::new(
                manifest,
                locale_chain,
                document_kind,
                Some(session.audit_session_id()),
                field_path,
            );
            let mut reported = net.check(clean_text, context)?;
            if let Some(path) = field_path {
                for suspect in &mut reported {
                    if suspect.field_path.is_none() {
                        suspect.field_path = Some(path.to_string());
                    }
                }
            }
            suspects.extend(reported);
        }

        Ok(LeakReport::from_parts(suspects, telemetry))
    }

    fn action_for(&self, detection: &Detection, context: &RuleContext) -> Action {
        self.rules
            .iter()
            .find_map(|rule| rule.action(&detection.class, context))
            .unwrap_or(Action::Preserve)
    }

    fn log_entry(
        &self,
        session: &Session,
        detection: &IndexedDetection,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        action: Action,
        conflict_loser: bool,
    ) -> Result<()> {
        let entry = RedactionEntry::new(
            detection.detection.source.clone(),
            detection.detection.class.clone(),
            action,
            field_name.map(str::to_string),
            document_kind,
            conflict_loser,
            detection.decided_by,
            crate::redaction_log::current_epoch_ms(),
            Some(session.audit_session_id().to_string()),
        );

        for logger in &self.redaction_loggers {
            logger.log(&entry)?;
        }

        Ok(())
    }

    fn log_vetoed_entry(
        &self,
        session: &Session,
        vetoed: &crate::validator_veto::VetoedCandidate,
        field_name: Option<&str>,
        document_kind: DocumentKind,
    ) -> Result<()> {
        let entry = RedactionEntry::new(
            vetoed.candidate.source.clone(),
            vetoed.candidate.class.clone(),
            self.action_for(
                &Detection::new(
                    vetoed.candidate.span.clone(),
                    vetoed.candidate.class.clone(),
                    vetoed.candidate.source.clone(),
                ),
                &build_context(field_name),
            ),
            field_name.map(str::to_string),
            document_kind,
            true,
            ConflictTier::ValidatorVeto,
            crate::redaction_log::current_epoch_ms(),
            Some(session.audit_session_id().to_string()),
        )
        .with_validator_fail_reason(vetoed.reason);

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

struct CleanText {
    text: String,
    manifest: Vec<EmittedTokenSpan>,
}

/// Builder for [`Pipeline`].
///
/// Obtain via [`Pipeline::builder()`]. Chain `.recognizer()`, `.rule()`, optionally
/// `.redaction_logger()` / `.register_safety_net()`, then call `.build()`.
///
/// For bundled defaults (core rulepack + locale-aware recognizers without manual wiring), use
/// `gaze_assembly::CorePipelineConfig` instead.
#[derive(Default)]
pub struct PipelineBuilder {
    recognizers: Vec<Arc<dyn Recognizer>>,
    redaction_loggers: Vec<Arc<dyn RedactionLogger>>,
    safety_nets: Vec<Arc<dyn SafetyNet>>,
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

    pub fn register_safety_net<N>(mut self, safety_net: N) -> Self
    where
        N: SafetyNet + 'static,
    {
        self.safety_nets.push(Arc::new(safety_net));
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
            safety_nets: self.safety_nets,
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
) -> Result<CleanDocument> {
    let mut clean = BTreeMap::new();
    for (key, value) in fields {
        let path = format!("$.{key}");
        clean.insert(
            key.clone(),
            redact_structured_value(
                pipeline,
                session,
                value,
                &key,
                &path,
                document_kind,
                locale_chain,
                dictionaries,
            )?,
        );
    }
    Ok(CleanDocument::Structured(clean))
}

#[allow(clippy::too_many_arguments)]
fn redact_structured_value(
    pipeline: &Pipeline,
    session: &Session,
    value: Value,
    field_name: &str,
    field_path: &str,
    document_kind: DocumentKind,
    locale_chain: &[crate::LocaleTag],
    dictionaries: &DictionaryBundle,
) -> Result<Value> {
    match value {
        Value::String(text) => Ok(Value::String(pipeline.redact_text(
            session,
            &text,
            Some(field_name),
            document_kind,
            locale_chain,
            dictionaries,
        )?)),
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(idx, value)| {
                redact_structured_value(
                    pipeline,
                    session,
                    value,
                    field_name,
                    &format!("{field_path}[{idx}]"),
                    document_kind,
                    locale_chain,
                    dictionaries,
                )
            })
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(fields) => {
            let mut clean = BTreeMap::new();
            for (key, value) in fields {
                let child_path = format!("{field_path}.{key}");
                clean.insert(
                    key.clone(),
                    redact_structured_value(
                        pipeline,
                        session,
                        value,
                        &key,
                        &child_path,
                        document_kind,
                        locale_chain,
                        dictionaries,
                    )?,
                );
            }
            Ok(Value::Object(clean))
        }
        Value::Null | Value::Bool(_) | Value::I64(_) => Ok(value),
        _ => Err(Error::UnsupportedValueVariant),
    }
}

#[allow(clippy::too_many_arguments)]
fn redact_structured_with_safety_net(
    pipeline: &Pipeline,
    session: &Session,
    fields: BTreeMap<String, Value>,
    locale_chain: &[crate::LocaleTag],
    dictionaries: &DictionaryBundle,
    report: &mut LeakReport,
) -> Result<BTreeMap<String, Value>> {
    let mut clean = BTreeMap::new();
    for (key, value) in fields {
        let path = format!("$.{key}");
        clean.insert(
            key.clone(),
            redact_structured_value_with_safety_net(
                pipeline,
                session,
                value,
                &key,
                &path,
                locale_chain,
                dictionaries,
                report,
            )?,
        );
    }
    Ok(clean)
}

#[allow(clippy::too_many_arguments)]
fn redact_structured_value_with_safety_net(
    pipeline: &Pipeline,
    session: &Session,
    value: Value,
    field_name: &str,
    field_path: &str,
    locale_chain: &[crate::LocaleTag],
    dictionaries: &DictionaryBundle,
    report: &mut LeakReport,
) -> Result<Value> {
    match value {
        Value::String(text) => {
            if text.is_empty() {
                return Ok(Value::String(text));
            }
            let clean = pipeline.redact_text_with_manifest(
                session,
                &text,
                Some(field_name),
                DocumentKind::Structured,
                locale_chain,
                dictionaries,
            )?;
            // For RawDocument::Structured, locale gating uses the session-level
            // locale chain across all fields; fields have no locale annotations.
            let field_report = pipeline.run_safety_nets(
                session,
                &clean.text,
                &Manifest::from_spans(clean.manifest),
                DocumentKind::Structured,
                locale_chain,
                Some(field_path),
            )?;
            report.extend(field_report);
            Ok(Value::String(clean.text))
        }
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(idx, value)| {
                redact_structured_value_with_safety_net(
                    pipeline,
                    session,
                    value,
                    field_name,
                    &format!("{field_path}[{idx}]"),
                    locale_chain,
                    dictionaries,
                    report,
                )
            })
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(fields) => {
            let mut clean = BTreeMap::new();
            for (key, value) in fields {
                let child_path = format!("{field_path}.{key}");
                clean.insert(
                    key.clone(),
                    redact_structured_value_with_safety_net(
                        pipeline,
                        session,
                        value,
                        &key,
                        &child_path,
                        locale_chain,
                        dictionaries,
                        report,
                    )?,
                );
            }
            Ok(Value::Object(clean))
        }
        Value::Null | Value::Bool(_) | Value::I64(_) => {
            if let Some(scalar) = value.scalar_to_safety_net_string() {
                let field_report = pipeline.run_safety_nets(
                    session,
                    &scalar,
                    &Manifest::default(),
                    DocumentKind::Structured,
                    locale_chain,
                    Some(field_path),
                )?;
                report.extend(field_report);
            }
            Ok(value)
        }
        _ => Err(Error::UnsupportedValueVariant),
    }
}

fn walk_value_for_safety_net_scan(
    pipeline: &Pipeline,
    session: &Session,
    value: &Value,
    field_path: &str,
    locale_chain: &[crate::LocaleTag],
    report: &mut LeakReport,
) -> Result<()> {
    match value {
        Value::String(text) => {
            if !text.is_empty() {
                let field_report = pipeline.run_safety_nets(
                    session,
                    text,
                    &Manifest::default(),
                    DocumentKind::Structured,
                    locale_chain,
                    Some(field_path),
                )?;
                report.extend(field_report);
            }
        }
        Value::Null => {}
        Value::Bool(_) | Value::I64(_) => {
            if let Some(scalar) = value.scalar_to_safety_net_string() {
                let field_report = pipeline.run_safety_nets(
                    session,
                    &scalar,
                    &Manifest::default(),
                    DocumentKind::Structured,
                    locale_chain,
                    Some(field_path),
                )?;
                report.extend(field_report);
            }
        }
        Value::Array(values) => {
            for (idx, value) in values.iter().enumerate() {
                walk_value_for_safety_net_scan(
                    pipeline,
                    session,
                    value,
                    &format!("{field_path}[{idx}]"),
                    locale_chain,
                    report,
                )?;
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                walk_value_for_safety_net_scan(
                    pipeline,
                    session,
                    value,
                    &format!("{field_path}.{key}"),
                    locale_chain,
                    report,
                )?;
            }
        }
        _ => return Err(Error::UnsupportedValueVariant),
    }
    Ok(())
}

fn translate_candidate(candidate: Candidate, spans: &[(usize, usize)]) -> Option<Candidate> {
    translate_span(candidate.span.clone(), spans).map(|span| candidate.with_span(span))
}

fn translate_vetoed_candidate(
    vetoed: crate::validator_veto::VetoedCandidate,
    spans: &[(usize, usize)],
) -> Option<crate::validator_veto::VetoedCandidate> {
    translate_candidate(vetoed.candidate, spans).map(|candidate| {
        crate::validator_veto::VetoedCandidate {
            candidate,
            reason: vetoed.reason,
        }
    })
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
                detection: Detection::new(
                    winner.span.clone(),
                    winner.class.clone(),
                    source.clone(),
                ),
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
            detection: Detection::new(candidate.span, candidate.class, candidate.source),
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
                Candidate::new(
                    detection.span,
                    detection.class,
                    source.clone(),
                    1.0,
                    0,
                    None,
                    "counter",
                    source,
                    ConflictTier::None,
                    Vec::new(),
                )
            })
            .collect()
    }

    fn token_family(&self) -> &str {
        "counter"
    }
}

fn generalize_token(class: &PiiClass) -> String {
    match class {
        PiiClass::Email => "[EMAIL]".to_string(),
        PiiClass::Name => "[NAME]".to_string(),
        PiiClass::Location => "[LOCATION]".to_string(),
        PiiClass::Organization => "[ORGANIZATION]".to_string(),
        PiiClass::Custom(name) => format!("[{}]", name.to_ascii_uppercase()),
    }
}

fn build_context(field_name: Option<&str>) -> RuleContext {
    RuleContext {
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
        fn log(&self, entry: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
            self.entries.lock().unwrap().push(entry.clone());
            Ok(())
        }
    }

    #[test]
    fn generalize_token_custom_class_preserves_identity() {
        // Regression guard: custom classes must not collapse to an indistinct [PII].
        assert_eq!(generalize_token(&PiiClass::Custom("foo".into())), "[FOO]");
    }

    #[test]
    fn stacked_ner_detectors_resolve_via_span_conflict() {
        // Input: "Alice Smith works here" — byte spans: Alice=0..5, full name=0..11.
        let text = "Alice Smith works here";
        let short_detection = Detection::new(0..5, PiiClass::Name, "ner/bert");
        let long_detection = Detection::new(0..11, PiiClass::Name, "ner/gliner");

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
        let alice = Detection::new(0..5, PiiClass::Name, "ner/bert");
        let berlin = Detection::new(14..20, PiiClass::Location, "ner/gliner");

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
                    .map(|m| Detection::new(m.range(), PiiClass::Email, "regex"))
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
                vec![Candidate::new(
                    start..end,
                    PiiClass::Name,
                    self.id(),
                    1.0,
                    0,
                    None,
                    self.token_family(),
                    self.id(),
                    ConflictTier::None,
                    Vec::new(),
                )]
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
