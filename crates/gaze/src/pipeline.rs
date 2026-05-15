use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

#[cfg(feature = "bundled-recognizers")]
use gaze_recognizers::{
    LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan, ModelStage,
};
use gaze_types::{
    AmbiguityReason, AmbiguityRecord, CollisionMembership, EmittedTokenSpan, FallbackReason,
    LeakKind, LeakReport, LeakReportTelemetry, LeakSuspect, Manifest, RedactionLogError,
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
    #[error("safety net fallback failed closed: {0:?}")]
    SafetyNetFallback(FallbackReason),
    #[error("capitals heuristic gate is unsupported for locale {locale}")]
    UnsupportedCapitalHeuristicLocale { locale: String },
    #[error("unsupported raw document variant")]
    UnsupportedRawDocumentVariant,
    #[error("unsupported structured value variant")]
    UnsupportedValueVariant,
    #[error("unsupported policy action variant")]
    UnsupportedActionVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SafetyNetMode {
    Strict,
    Tolerant,
    Redact,
    Resolve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SafetyNetFallback {
    Strict,
    Tolerant,
    Redact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SafetyNetPolicy {
    pub mode: SafetyNetMode,
    pub fallback: SafetyNetFallback,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PipelineOptimizationConfig {
    pub skip_class_gating: bool,
    pub capitals_heuristic_gate: bool,
    pub prefix_cache: bool,
    pub length_bucketing: bool,
}

impl PipelineOptimizationConfig {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_skip_class_gating(mut self, enabled: bool) -> Self {
        self.skip_class_gating = enabled;
        self
    }
    pub fn with_capitals_heuristic_gate(mut self, enabled: bool) -> Self {
        self.capitals_heuristic_gate = enabled;
        self
    }
    pub fn with_prefix_cache(mut self, enabled: bool) -> Self {
        self.prefix_cache = enabled;
        self
    }
    pub fn with_length_bucketing(mut self, enabled: bool) -> Self {
        self.length_bucketing = enabled;
        self
    }
}

impl Default for SafetyNetPolicy {
    fn default() -> Self {
        Self {
            mode: SafetyNetMode::Resolve,
            fallback: SafetyNetFallback::Redact,
        }
    }
}

impl SafetyNetPolicy {
    pub fn new(mode: SafetyNetMode, fallback: SafetyNetFallback) -> Self {
        Self { mode, fallback }
    }
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
    #[cfg(feature = "bundled-recognizers")]
    safety_net_registry: Option<Arc<LocaleAwareModelRegistry>>,
    optimization_config: PipelineOptimizationConfig,
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

    pub fn with_pipeline_optimizations(mut self, config: PipelineOptimizationConfig) -> Pipeline {
        self.optimization_config = config;
        self
    }

    #[cfg(feature = "bundled-recognizers")]
    pub fn with_safety_net_registry(mut self, registry: LocaleAwareModelRegistry) -> Pipeline {
        self.safety_net_registry = Some(Arc::new(registry));
        self
    }

    /// Pseudonymizes using the default `[Global]` locale chain.
    ///
    /// Prefer `pseudonymize_with_context` when policy, CLI, or rulepack locale
    /// defaults should constrain which recognizers run.
    pub fn redact(&self, session: &Session, raw: RawDocument) -> Result<CleanDocument> {
        let locale_chain = [crate::LocaleTag::Global];
        self.pseudonymize_with_context(session, raw, &locale_chain)
    }

    pub fn pseudonymize_with_context(
        &self,
        session: &Session,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
    ) -> Result<CleanDocument> {
        let dictionaries = DictionaryBundle::default();
        self.pseudonymize_with_detect_context(session, raw, locale_chain, &dictionaries)
    }

    pub fn pseudonymize_with_detect_context(
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
            RawDocument::Text(text) => Ok(CleanDocument::Text(self.pseudonymize_text(
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
        self.clean_with_safety_net_policy_detect_context(
            session,
            raw,
            locale_chain,
            dictionaries,
            SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
        )
    }

    pub fn clean_with_safety_net_policy_detect_context(
        &self,
        session: &Session,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
        policy: SafetyNetPolicy,
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
                    policy,
                )?;
                Ok((CleanDocument::Structured(clean), Vec::new(), report))
            }
            RawDocument::Text(text) => {
                let mut clean = self.redact_text_with_manifest(
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
                    policy.mode,
                )?;
                self.apply_safety_net_policy(
                    session,
                    &mut clean,
                    &report,
                    DocumentKind::Text,
                    locale_chain,
                    None,
                    policy,
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
        let nets_run = self.safety_nets_len();
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
            SafetyNetMode::Strict,
        )?;
        Ok(SafetyNetResult { nets_run, report })
    }

    pub fn scan_safety_nets_structured(
        &self,
        session: &Session,
        document: &BTreeMap<String, Value>,
        locale_chain: &[crate::LocaleTag],
    ) -> Result<SafetyNetResult> {
        let nets_run = self.safety_nets_len();
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
    fn pseudonymize_text(
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
        if self.optimization_config.prefix_cache {
            if let Some(hit) = session
                .lookup_prefix_cache(text)
                .filter(|hit| hit.raw_len < text.len())
            {
                let suffix = &text[hit.raw_len..];
                let suffix_clean = self.redact_text_with_manifest_uncached(
                    session,
                    suffix,
                    field_name,
                    document_kind,
                    locale_chain,
                    dictionaries,
                )?;
                let clean_offset = hit.clean_text.len();
                let raw_offset = hit.raw_len;
                let mut manifest = hit.manifest.clone();
                manifest.extend(suffix_clean.manifest.into_iter().map(|mut span| {
                    span.clean_span.start += clean_offset;
                    span.clean_span.end += clean_offset;
                    span.raw_span.start += raw_offset;
                    span.raw_span.end += raw_offset;
                    span
                }));
                let mut clean_text = hit.clean_text;
                clean_text.push_str(&suffix_clean.text);
                self.log_prefix_cache_entries(
                    session,
                    &hit.manifest,
                    field_name,
                    document_kind,
                    locale_chain,
                )?;
                session.store_prefix_cache(text, &clean_text, &manifest);
                return Ok(CleanText {
                    text: clean_text,
                    manifest,
                });
            }
        }

        let clean = self.redact_text_with_manifest_uncached(
            session,
            text,
            field_name,
            document_kind,
            locale_chain,
            dictionaries,
        )?;
        if self.optimization_config.prefix_cache {
            session.store_prefix_cache(text, &clean.text, &clean.manifest);
        }
        Ok(clean)
    }

    #[allow(clippy::too_many_arguments)]
    fn redact_text_with_manifest_uncached(
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
        let (resolved, vetoed) = self.registry.detect_all_resolved(&normalized.text, &ctx);
        let vetoed = vetoed
            .into_iter()
            .filter_map(|vetoed| translate_vetoed_candidate(vetoed, spans))
            .collect::<Vec<_>>();
        let resolved = resolved
            .into_iter()
            .filter_map(|candidate| translate_candidate(candidate, spans))
            .collect::<Vec<_>>();
        let losers = merged_losers(&resolved, &self.registry);
        let mut detections = resolved
            .into_iter()
            .map(|candidate| indexed_detection_from_candidate(candidate, &self.registry))
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

    #[allow(clippy::too_many_arguments)]
    fn run_safety_nets(
        &self,
        session: &Session,
        clean_text: &str,
        manifest: &Manifest,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        field_path: Option<&str>,
        safety_net_mode: SafetyNetMode,
    ) -> Result<LeakReport> {
        if self.safety_nets_len() == 0 {
            return Ok(LeakReport::default());
        }
        if self.should_skip_safety_nets(clean_text, manifest, locale_chain, safety_net_mode)? {
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
        #[cfg(feature = "bundled-recognizers")]
        if let Some(registry) = &self.safety_net_registry {
            let locale = locale_chain
                .first()
                .cloned()
                .unwrap_or(crate::LocaleTag::Global);
            let selected = registry
                .resolve(&locale, ModelStage::Pass3SafetyNet)
                .map_err(model_error_to_safety_net_error)?;
            if let Some(model) = selected.first() {
                let spans = model
                    .infer(
                        ModelInput {
                            text: clean_text.to_string(),
                            locale,
                        },
                        ModelHints {
                            stage: ModelStage::Pass3SafetyNet,
                            max_spans: None,
                        },
                    )
                    .map_err(model_error_to_safety_net_error)?;
                for span in spans {
                    if let Some(suspect) =
                        model_span_to_suspect(span, model.name(), manifest, field_path)
                    {
                        suspects.push(suspect);
                    }
                }
            }
        }

        Ok(LeakReport::from_parts(suspects, telemetry))
    }

    fn should_skip_safety_nets(
        &self,
        clean_text: &str,
        manifest: &Manifest,
        locale_chain: &[crate::LocaleTag],
        safety_net_mode: SafetyNetMode,
    ) -> Result<bool> {
        if !matches!(
            safety_net_mode,
            SafetyNetMode::Strict | SafetyNetMode::Tolerant
        ) {
            return Ok(false);
        }
        if self.optimization_config.capitals_heuristic_gate {
            validate_capitals_gate_locales(locale_chain)?;
            if is_numeric_heavy(clean_text) || !has_non_sentence_start_capital(clean_text) {
                return Ok(true);
            }
        }
        if self.optimization_config.skip_class_gating
            && !manifest.spans.is_empty()
            && !has_residual_gold_shape(clean_text)
        {
            return Ok(true);
        }
        Ok(false)
    }

    fn safety_nets_len(&self) -> usize {
        let single = self.safety_nets.len();
        #[cfg(feature = "bundled-recognizers")]
        {
            single
                + self
                    .safety_net_registry
                    .as_ref()
                    .map_or(0, |registry| usize::from(!registry.is_empty()))
        }
        #[cfg(not(feature = "bundled-recognizers"))]
        {
            single
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_safety_net_policy(
        &self,
        session: &Session,
        clean: &mut CleanText,
        report: &LeakReport,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        field_path: Option<&str>,
        policy: SafetyNetPolicy,
    ) -> Result<()> {
        match policy.mode {
            SafetyNetMode::Strict | SafetyNetMode::Tolerant => Ok(()),
            SafetyNetMode::Redact => self.redact_safety_net_suspects(
                session,
                clean,
                report,
                document_kind,
                field_path,
                None,
                true,
            ),
            SafetyNetMode::Resolve => {
                let reason = match self.resolve_safety_net_suspects(
                    session,
                    clean,
                    report,
                    document_kind,
                    field_path,
                )? {
                    Some(reason) => Some(reason),
                    None => {
                        let follow_up = self.run_safety_nets(
                            session,
                            &clean.text,
                            &Manifest::from_spans(clean.manifest.clone()),
                            document_kind,
                            locale_chain,
                            field_path,
                            SafetyNetMode::Resolve,
                        )?;
                        (follow_up.stats.uncovered_count + follow_up.stats.partial_bleed_count > 0)
                            .then_some(FallbackReason::ResidualSuspect)
                    }
                };
                if let Some(reason) = reason {
                    self.apply_safety_net_fallback(
                        session,
                        clean,
                        report,
                        document_kind,
                        field_path,
                        policy.fallback,
                        reason,
                    )?;
                }
                Ok(())
            }
        }
    }

    fn resolve_safety_net_suspects(
        &self,
        session: &Session,
        clean: &mut CleanText,
        report: &LeakReport,
        document_kind: DocumentKind,
        field_path: Option<&str>,
    ) -> Result<Option<FallbackReason>> {
        if report
            .suspects
            .iter()
            .any(|suspect| matches!(suspect.kind, LeakKind::ClassMismatch { .. }))
        {
            return Ok(Some(FallbackReason::OverlapConflict));
        }
        for suspect in actionable_suspects(report).into_iter().rev() {
            let span = suspect_action_span(suspect);
            if !is_char_boundary_range(&clean.text, &span) {
                return Ok(Some(FallbackReason::ResidualSuspect));
            }
            let raw = clean.text[span.clone()].to_string();
            let replacement = session.tokenize_with_family("safety_net", &suspect.class, &raw)?;
            self.log_safety_net_entry(
                session,
                suspect,
                document_kind,
                field_path,
                Action::Tokenize,
                false,
                ConflictTier::Resolve,
                None,
            )?;
            replace_clean_span(
                clean,
                span.clone(),
                &replacement,
                Some(EmittedTokenSpan::new(
                    span.start..span.start + replacement.len(),
                    span,
                    suspect.class.clone(),
                )),
            );
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_safety_net_fallback(
        &self,
        session: &Session,
        clean: &mut CleanText,
        report: &LeakReport,
        document_kind: DocumentKind,
        field_path: Option<&str>,
        fallback: SafetyNetFallback,
        reason: FallbackReason,
    ) -> Result<()> {
        for suspect in redaction_suspects(report) {
            self.log_safety_net_entry(
                session,
                suspect,
                document_kind,
                field_path,
                fallback_action(fallback),
                true,
                ConflictTier::Fallback,
                Some(reason),
            )?;
        }
        match fallback {
            SafetyNetFallback::Strict => Err(Error::SafetyNetFallback(reason)),
            SafetyNetFallback::Tolerant => Ok(()),
            SafetyNetFallback::Redact => self.redact_safety_net_suspects(
                session,
                clean,
                report,
                document_kind,
                field_path,
                Some(reason),
                false,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn redact_safety_net_suspects(
        &self,
        session: &Session,
        clean: &mut CleanText,
        report: &LeakReport,
        document_kind: DocumentKind,
        field_path: Option<&str>,
        fallback_reason: Option<FallbackReason>,
        log_entries: bool,
    ) -> Result<()> {
        for suspect in redaction_suspects(report).into_iter().rev() {
            let span = suspect_action_span(suspect);
            if !is_char_boundary_range(&clean.text, &span) {
                continue;
            }
            if log_entries {
                self.log_safety_net_entry(
                    session,
                    suspect,
                    document_kind,
                    field_path,
                    Action::Redact,
                    fallback_reason.is_some(),
                    if fallback_reason.is_some() {
                        ConflictTier::Fallback
                    } else {
                        ConflictTier::Redact
                    },
                    fallback_reason,
                )?;
            }
            replace_clean_span(clean, span, "", None);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn log_safety_net_entry(
        &self,
        session: &Session,
        suspect: &LeakSuspect,
        document_kind: DocumentKind,
        field_path: Option<&str>,
        action: Action,
        conflict_loser: bool,
        decided_by: ConflictTier,
        fallback_reason: Option<FallbackReason>,
    ) -> Result<()> {
        let source = format!("safety_net.{}", suspect.safety_net_id);
        let mut entry = RedactionEntry::new(
            source.clone(),
            suspect.class.clone(),
            action,
            field_path
                .map(str::to_string)
                .or_else(|| suspect.field_path.clone()),
            document_kind,
            conflict_loser,
            decided_by,
            crate::redaction_log::current_epoch_ms(),
            Some(session.audit_session_id().to_string()),
        )
        .with_recognizer_metadata(Some(source), None);
        if let Some(reason) = fallback_reason {
            entry = entry.with_fallback_triggered(reason);
        }
        for logger in &self.redaction_loggers {
            logger.log(&entry)?;
        }
        Ok(())
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
        let mut entry = RedactionEntry::new(
            detection.detection.source.clone(),
            detection.detection.class.clone(),
            action,
            field_name.map(str::to_string),
            document_kind,
            conflict_loser,
            detection.decided_by,
            crate::redaction_log::current_epoch_ms(),
            Some(session.audit_session_id().to_string()),
        )
        .with_recognizer_metadata(
            detection.recognizer_id.clone(),
            detection.recognizer_version_id.clone(),
        );
        if let Some(record) = detection.ambiguity_record.clone() {
            entry = entry.with_ambiguity_record(record);
        }
        if detection.collision_family.is_some() || detection.collision_variant.is_some() {
            entry = entry.with_collision_metadata(
                detection.collision_family.clone(),
                detection.collision_variant.clone(),
            );
        }

        for logger in &self.redaction_loggers {
            logger.log(&entry)?;
        }

        Ok(())
    }

    fn log_prefix_cache_entries(
        &self,
        session: &Session,
        manifest: &[EmittedTokenSpan],
        field_name: Option<&str>,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
    ) -> Result<()> {
        let locale = locale_chain
            .first()
            .map(crate::LocaleTag::as_str)
            .unwrap_or("global")
            .to_string();
        for span in manifest {
            let entry = RedactionEntry::new(
                "prefix_cache",
                span.class.clone(),
                Action::Tokenize,
                field_name.map(str::to_string),
                document_kind,
                false,
                ConflictTier::None,
                crate::redaction_log::current_epoch_ms(),
                Some(session.audit_session_id().to_string()),
            )
            .with_recognizer_metadata(Some("prefix_cache".to_string()), None)
            .with_provenance_metadata(
                Some("prefix_cache".to_string()),
                None,
                None,
                None,
                None,
                Some(locale.clone()),
                Some("session_prefix".to_string()),
                Some(span.class.to_canonical_str()),
                Some(span.class.to_canonical_str()),
                None,
                None,
            );
            for logger in &self.redaction_loggers {
                logger.log(&entry)?;
            }
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
        .with_recognizer_metadata(
            Some(vetoed.candidate.recognizer_id.clone()),
            vetoed.candidate.recognizer_version_id.clone(),
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
    recognizer_id: Option<String>,
    recognizer_version_id: Option<String>,
    decided_by: ConflictTier,
    family: String,
    ambiguity_record: Option<AmbiguityRecord>,
    collision_family: Option<String>,
    collision_variant: Option<String>,
}

struct CleanText {
    text: String,
    manifest: Vec<EmittedTokenSpan>,
}

fn actionable_suspects(report: &LeakReport) -> Vec<&LeakSuspect> {
    let mut suspects = report
        .suspects
        .iter()
        .filter(|suspect| {
            matches!(
                suspect.kind,
                LeakKind::Uncovered | LeakKind::PartialBleed { .. }
            )
        })
        .collect::<Vec<_>>();
    suspects.sort_by_key(|suspect| suspect_action_span(suspect).start);
    suspects
}

fn redaction_suspects(report: &LeakReport) -> Vec<&LeakSuspect> {
    let mut suspects = report.suspects.iter().collect::<Vec<_>>();
    suspects.sort_by_key(|suspect| suspect_action_span(suspect).start);
    suspects
}

fn suspect_action_span(suspect: &LeakSuspect) -> Range<usize> {
    match &suspect.kind {
        LeakKind::PartialBleed { uncovered } => uncovered.clone(),
        _ => suspect.span.clone(),
    }
}

fn fallback_action(fallback: SafetyNetFallback) -> Action {
    match fallback {
        SafetyNetFallback::Strict | SafetyNetFallback::Tolerant => Action::Preserve,
        SafetyNetFallback::Redact => Action::Redact,
    }
}

fn is_char_boundary_range(text: &str, span: &Range<usize>) -> bool {
    span.start <= span.end
        && span.end <= text.len()
        && text.is_char_boundary(span.start)
        && text.is_char_boundary(span.end)
}

fn validate_capitals_gate_locales(locale_chain: &[crate::LocaleTag]) -> Result<()> {
    for locale in locale_chain {
        if !capital_case_locale_supported(locale) {
            return Err(Error::UnsupportedCapitalHeuristicLocale {
                locale: locale.as_str().to_string(),
            });
        }
    }
    Ok(())
}

fn capital_case_locale_supported(locale: &crate::LocaleTag) -> bool {
    matches!(
        locale,
        crate::LocaleTag::Global
            | crate::LocaleTag::DeDe
            | crate::LocaleTag::DeAt
            | crate::LocaleTag::DeCh
            | crate::LocaleTag::EnUs
            | crate::LocaleTag::EnGb
            | crate::LocaleTag::EnIe
            | crate::LocaleTag::EnAu
            | crate::LocaleTag::EnCa
    )
}

fn is_numeric_heavy(text: &str) -> bool {
    let digits = text.chars().filter(|ch| ch.is_ascii_digit()).count();
    let letters = text.chars().filter(|ch| ch.is_alphabetic()).count();
    digits >= 6 && digits > letters.saturating_mul(2)
}

// This heuristic is only recall-preserving for locales with capital-case name
// conventions (currently English and German). Arabic, CJK, and other scripts
// must fail closed through `validate_capitals_gate_locales`.
fn has_non_sentence_start_capital(text: &str) -> bool {
    let mut sentence_start = true;
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_uppercase() && !sentence_start {
            return true;
        }
        sentence_start = matches!(ch, '.' | '!' | '?');
    }
    false
}

fn has_residual_gold_shape(text: &str) -> bool {
    text.contains('@') || is_numeric_heavy(text)
}

fn replace_clean_span(
    clean: &mut CleanText,
    span: Range<usize>,
    replacement: &str,
    emitted: Option<EmittedTokenSpan>,
) {
    let removed_len = span.end - span.start;
    let replacement_len = replacement.len();
    clean.text.replace_range(span.clone(), replacement);
    clean.manifest = clean
        .manifest
        .iter()
        .filter_map(|existing| adjust_emitted_span(existing, &span, replacement_len, removed_len))
        .chain(emitted)
        .collect();
    clean.manifest.sort_by_key(|span| span.clean_span.start);
}

fn adjust_emitted_span(
    existing: &EmittedTokenSpan,
    edited: &Range<usize>,
    replacement_len: usize,
    removed_len: usize,
) -> Option<EmittedTokenSpan> {
    if existing.clean_span.start < edited.end && edited.start < existing.clean_span.end {
        return None;
    }
    let mut span = existing.clone();
    if span.clean_span.start >= edited.end {
        if replacement_len >= removed_len {
            let delta = replacement_len - removed_len;
            span.clean_span.start += delta;
            span.clean_span.end += delta;
        } else {
            let delta = removed_len - replacement_len;
            span.clean_span.start -= delta;
            span.clean_span.end -= delta;
        }
    }
    Some(span)
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
    collision_memberships: Vec<(String, CollisionMembership)>,
    anchor_cue_bundles: Vec<(crate::LocaleTag, String, Vec<String>, Option<u16>)>,
    redaction_loggers: Vec<Arc<dyn RedactionLogger>>,
    safety_nets: Vec<Arc<dyn SafetyNet>>,
    #[cfg(feature = "bundled-recognizers")]
    safety_net_registry: Option<Arc<LocaleAwareModelRegistry>>,
    optimization_config: PipelineOptimizationConfig,
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

    pub fn register_collision(
        mut self,
        recognizer_id: impl Into<String>,
        membership: CollisionMembership,
    ) -> Self {
        self.collision_memberships
            .push((recognizer_id.into(), membership));
        self
    }

    pub fn register_anchor_cue_bundle(
        mut self,
        locale: crate::LocaleTag,
        anchor_key: impl Into<String>,
        names: Vec<String>,
        window_chars: Option<u16>,
    ) -> Self {
        self.anchor_cue_bundles
            .push((locale, anchor_key.into(), names, window_chars));
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

    pub fn pipeline_optimizations(mut self, config: PipelineOptimizationConfig) -> Self {
        self.optimization_config = config;
        self
    }

    pub fn enable_skip_class_gating(mut self) -> Self {
        self.optimization_config.skip_class_gating = true;
        self
    }

    pub fn enable_capitals_heuristic_gate(mut self) -> Self {
        self.optimization_config.capitals_heuristic_gate = true;
        self
    }

    pub fn enable_prefix_cache(mut self) -> Self {
        self.optimization_config.prefix_cache = true;
        self
    }

    pub fn enable_length_bucketing(mut self) -> Self {
        self.optimization_config.length_bucketing = true;
        self
    }

    #[cfg(feature = "bundled-recognizers")]
    pub fn register_safety_net_registry(mut self, registry: LocaleAwareModelRegistry) -> Self {
        self.safety_net_registry = Some(Arc::new(registry));
        self
    }

    pub fn build(self) -> Result<Pipeline> {
        let mut registry = RecognizerRegistry::builder();
        for recognizer in self.recognizers {
            registry = registry.register_arc(recognizer);
        }
        for (recognizer_id, membership) in self.collision_memberships {
            registry = registry.register_collision(recognizer_id, membership);
        }
        for (locale, anchor_key, names, window_chars) in self.anchor_cue_bundles {
            registry = registry.register_anchor_cue_bundle(locale, anchor_key, names, window_chars);
        }
        Ok(Pipeline {
            registry: Arc::new(registry.build()),
            redaction_loggers: self.redaction_loggers,
            safety_nets: self.safety_nets,
            #[cfg(feature = "bundled-recognizers")]
            safety_net_registry: self.safety_net_registry,
            optimization_config: self.optimization_config,
            rules: self.rules,
        })
    }
}

#[cfg(feature = "bundled-recognizers")]
fn model_span_to_suspect(
    span: ModelSpan,
    backend_name: &str,
    manifest: &Manifest,
    field_path: Option<&str>,
) -> Option<LeakSuspect> {
    let kind = manifest.diff_against(&span.byte_range, &span.class)?;
    Some(LeakSuspect::new(
        span.byte_range,
        span.class.clone(),
        backend_name,
        span.confidence,
        kind,
        format!("{:?}", span.class),
        field_path.map(str::to_string),
    ))
}

#[cfg(feature = "bundled-recognizers")]
fn model_error_to_safety_net_error(error: ModelError) -> SafetyNetError {
    match error {
        ModelError::NoLocaleModelCoverage { .. } | ModelError::LocaleNotSupported(_) => {
            SafetyNetError::Unavailable {
                reason: error.to_string(),
            }
        }
        ModelError::IntegrityMismatch => SafetyNetError::ModelIntegrityMismatch {
            expected: "locale-aware registry".to_string(),
            actual: "backend integrity mismatch".to_string(),
        },
        ModelError::InitFailed(reason) => SafetyNetError::ModelUnavailable { reason },
        ModelError::InferenceFailed(message) | ModelError::Internal(message) => {
            SafetyNetError::Runtime { message }
        }
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
        Value::String(text) => Ok(Value::String(pipeline.pseudonymize_text(
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
    policy: SafetyNetPolicy,
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
                policy,
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
    policy: SafetyNetPolicy,
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
                policy.mode,
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
                    policy,
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
                        policy,
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
                    policy.mode,
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
                    SafetyNetMode::Strict,
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
                    SafetyNetMode::Strict,
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

fn merged_losers(resolved: &[Candidate], registry: &RecognizerRegistry) -> Vec<IndexedDetection> {
    resolved
        .iter()
        .flat_map(|winner| {
            winner.merged_sources.iter().map(|source| {
                let class = registry
                    .recognizer(source)
                    .map(|recognizer| recognizer.supported_class().clone())
                    .unwrap_or_else(|| winner.class.clone());
                let membership = registry.family_policy().membership(source);
                IndexedDetection {
                    detection: Detection::new(winner.span.clone(), class, source.clone()),
                    recognizer_id: Some(source.clone()),
                    recognizer_version_id: None,
                    decided_by: if winner.decided_by == ConflictTier::Merged {
                        ConflictTier::Merged
                    } else {
                        winner.decided_by
                    },
                    family: winner.token_family.clone(),
                    ambiguity_record: None,
                    collision_family: membership.map(|membership| membership.family.clone()),
                    collision_variant: membership.map(|membership| membership.variant.clone()),
                }
            })
        })
        .collect()
}

fn indexed_detection_from_candidate(
    candidate: Candidate,
    registry: &RecognizerRegistry,
) -> IndexedDetection {
    let membership = registry
        .family_policy()
        .membership(&candidate.recognizer_id);
    let mut collision_family = membership.map(|membership| membership.family.clone());
    let collision_variant = membership.map(|membership| membership.variant.clone());
    let mut ambiguity_record = None;

    let hybrid_reason = match candidate.decided_by {
        ConflictTier::CollisionPolicy => Some(AmbiguityReason::PrecedenceTie),
        ConflictTier::AnchoredContext => Some(AmbiguityReason::NoAnchor),
        _ => None,
    };
    if let Some(reason) = hybrid_reason {
        if let Some(hybrid) = crate::conflict::hybrid_fallback::emit(&candidate, registry, reason) {
            ambiguity_record = Some(hybrid.ambiguity_record);
            collision_family = Some(hybrid.collision_family);
        }
    }

    IndexedDetection {
        detection: Detection::new(candidate.span, candidate.class, candidate.source),
        recognizer_id: Some(candidate.recognizer_id),
        recognizer_version_id: candidate.recognizer_version_id,
        decided_by: candidate.decided_by,
        family: candidate.token_family,
        ambiguity_record,
        collision_family,
        collision_variant,
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
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    struct CountingSafetyNet {
        calls: Arc<AtomicUsize>,
    }

    impl SafetyNet for CountingSafetyNet {
        fn id(&self) -> &str {
            "counting"
        }
        fn supported_locales(&self) -> &[crate::LocaleTag] {
            &[crate::LocaleTag::Global]
        }
        fn check(
            &self,
            _clean_text: &str,
            _context: SafetyNetContext<'_>,
        ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    #[test]
    fn generalize_token_custom_class_preserves_identity() {
        // Regression guard: custom classes must not collapse to an indistinct [PII].
        assert_eq!(generalize_token(&PiiClass::Custom("foo".into())), "[FOO]");
    }

    #[test]
    fn skip_class_gating_skips_only_observer_mode() {
        let calls = Arc::new(AtomicUsize::new(0));
        let pipeline = Pipeline::builder()
            .detector(detector_with_detections(
                "email.global",
                vec![Detection::new(6..27, PiiClass::Email, "email.global")],
            ))
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(CountingSafetyNet {
                calls: Arc::clone(&calls),
            })
            .enable_skip_class_gating()
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let dictionaries = DictionaryBundle::default();
        pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text("Reach alice@example.invalid".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
                SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
            )
            .expect("observer safety net");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text("Reach alice@example.invalid".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
                SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Redact),
            )
            .expect("resolve safety net");
        assert!(calls.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn capitals_gate_fails_closed_for_unsupported_locale() {
        let calls = Arc::new(AtomicUsize::new(0));
        let pipeline = Pipeline::builder()
            .register_safety_net(CountingSafetyNet {
                calls: Arc::clone(&calls),
            })
            .enable_capitals_heuristic_gate()
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let dictionaries = DictionaryBundle::default();
        let err = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text("مرحبا".to_string()),
                &[crate::LocaleTag::Other("ar-SA".to_string())],
                &dictionaries,
                SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
            )
            .expect_err("unsupported capitals locale must fail closed");
        assert!(matches!(
            err,
            Error::UnsupportedCapitalHeuristicLocale { locale } if locale == "ar-SA"
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn prefix_cache_hits_emit_audit_provenance() {
        struct NameRecognizer;
        impl Recognizer for NameRecognizer {
            fn id(&self) -> &str {
                "name.fixture"
            }
            fn supported_class(&self) -> &PiiClass {
                &PiiClass::Name
            }
            fn detect(&self, input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
                input
                    .find("Dr. Schmidt")
                    .map(|start| {
                        Candidate::new(
                            start..start + "Dr. Schmidt".len(),
                            PiiClass::Name,
                            self.id(),
                            1.0,
                            0,
                            None,
                            self.token_family(),
                            self.id(),
                            ConflictTier::None,
                            Vec::new(),
                        )
                    })
                    .into_iter()
                    .collect()
            }
            fn token_family(&self) -> &str {
                "counter"
            }
        }
        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let pipeline = Pipeline::builder()
            .recognizer(NameRecognizer)
            .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .enable_prefix_cache()
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        pipeline
            .redact(&session, RawDocument::Text("Dr. Schmidt".to_string()))
            .expect("prime cache");
        pipeline
            .redact(
                &session,
                RawDocument::Text("Dr. Schmidt reports".to_string()),
            )
            .expect("cache hit");
        let entries = entries.lock().unwrap();
        assert!(entries.iter().any(|entry| {
            entry.source == "prefix_cache"
                && entry.provenance_stage.as_deref() == Some("prefix_cache")
        }));
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
    fn audit_entry_threads_recognizer_lineage_from_candidate() {
        struct VersionedRecognizer;

        impl Recognizer for VersionedRecognizer {
            fn id(&self) -> &str {
                "name.semantic"
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
                    "ner/ort",
                    ConflictTier::None,
                    Vec::new(),
                )
                .with_recognizer_version_id("name.semantic.v2")]
            }

            fn token_family(&self) -> &str {
                "counter"
            }
        }

        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let pipeline = Pipeline::builder()
            .recognizer(VersionedRecognizer)
            .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");

        pipeline
            .redact(
                &session,
                RawDocument::Text("Contact Dr. Schmidt today.".to_string()),
            )
            .expect("redact");

        let entries = entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, "ner/ort");
        assert_eq!(entries[0].recognizer_id.as_deref(), Some("name.semantic"));
        assert_eq!(
            entries[0].recognizer_version_id.as_deref(),
            Some("name.semantic.v2")
        );
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
                RawDocument::Text("Reach alice@example.invalid today".to_string()),
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
