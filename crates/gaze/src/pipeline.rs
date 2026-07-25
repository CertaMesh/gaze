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
    RedactionLogger, RestoreDecision, RestorePolicy, RestoreTelemetry, RestoredText, SafetyNet,
    SafetyNetContext, SafetyNetError, RESTORE_PHASE_MANIFEST_BYPASS_SCAN,
    RESTORE_PHASE_MANIFEST_LOOKUP, RESTORE_PHASE_UNKNOWN_TOKEN_SCAN,
};
use thiserror::Error;

use crate::detector::{Detection, Detector, PiiClass};
use crate::normalize::normalize;
use crate::policy::PolicyError;
use crate::redaction_log::{ConflictTier, DocumentKind, RedactionEntry};
use crate::registry::{Candidate, DetectContext, Recognizer, RecognizerRegistry};
use crate::rule::{Action, Rule, RuleContext};
use crate::rulepack::RulepackError;
use crate::session::{PrefixCacheHit, RestoreEvent, Session, SessionTransaction};
use crate::types::{CleanDocument, RawDocument, Value};
use crate::DictionaryBundle;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid regex: {0}")]
    InvalidRegex(#[source] regex::Error),
    #[error("unknown token: [REDACTED]")]
    UnknownToken {
        class: PiiClass,
        ordinal: u32,
        raw: String,
    },
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
    #[error("recognizer detection failed: {0}")]
    RecognizerDetect(#[from] gaze_types::DetectError),
    #[error("redaction log error: {0}")]
    RedactionLog(#[from] RedactionLogError),
    #[error("safety net fallback failed closed: {0:?}")]
    SafetyNetFallback(FallbackReason),
    #[error("safety net span invalid: start={start}, end={end}, text_len={text_len}")]
    SafetyNetSpanInvalid {
        start: usize,
        end: usize,
        text_len: usize,
    },
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

/// Controls whether one transactional protection call may reuse or populate the prefix cache.
///
/// [`Self::Suppress`] bypasses both prefix-cache lookup and publication while retaining all
/// detector, mapping, and manifest behavior. It is intended for mutation probes whose output is
/// inspected but never emitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrefixCacheWriteMode {
    Allow,
    Suppress,
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
    restore_boundary_dlp_audit: bool,
    rules: Vec<Arc<dyn Rule>>,
}

enum ProtectionTarget<'target, 'session> {
    Live(&'target Session),
    Staged(
        &'target mut SessionTransaction<'session>,
        PrefixCacheWriteMode,
    ),
}

impl ProtectionTarget<'_, '_> {
    fn tokenize_with_family(
        &mut self,
        family: &str,
        class: &PiiClass,
        raw: &str,
    ) -> Result<String> {
        match self {
            Self::Live(session) => session.tokenize_with_family(family, class, raw),
            Self::Staged(transaction, _) => transaction.tokenize_with_family(family, class, raw),
        }
    }

    fn format_preserving_fake(&mut self, class: &PiiClass, raw: &str) -> Result<String> {
        match self {
            Self::Live(session) => session.format_preserving_fake(class, raw),
            Self::Staged(transaction, _) => transaction.format_preserving_fake(class, raw),
        }
    }

    fn audit_session_id(&self) -> &str {
        match self {
            Self::Live(session) => session.audit_session_id(),
            Self::Staged(transaction, _) => transaction.audit_session_id(),
        }
    }

    /// Restores a single token against the state this run is writing into.
    ///
    /// Integrity checks must ask the target, not the session: in `Staged` mode the tokens this
    /// run just emitted live in an uncommitted transaction and are invisible to the session.
    fn restore(&self, token: &str) -> Option<String> {
        match self {
            Self::Live(session) => session.restore(token),
            Self::Staged(transaction, _) => transaction.restore(token),
        }
    }

    fn contains_token(&self, token: &str) -> bool {
        match self {
            Self::Live(session) => session.contains_token(token),
            Self::Staged(transaction, _) => transaction.contains_token(token),
        }
    }

    fn restore_strict_text(&self, text: &str) -> std::result::Result<String, crate::RestoreError> {
        match self {
            Self::Live(session) => session.restore_strict_text(text),
            Self::Staged(transaction, _) => transaction.restore_strict_text(text),
        }
    }

    fn lookup_prefix_cache(&self, text: &str) -> Option<PrefixCacheHit> {
        match self {
            Self::Live(session) => session.lookup_prefix_cache(text),
            Self::Staged(transaction, PrefixCacheWriteMode::Allow) => {
                transaction.lookup_prefix_cache(text)
            }
            Self::Staged(_, PrefixCacheWriteMode::Suppress) => None,
        }
    }

    fn store_prefix_cache(&mut self, raw: &str, clean_text: &str, manifest: &[EmittedTokenSpan]) {
        match self {
            Self::Live(session) => session.store_prefix_cache(raw, clean_text, manifest),
            Self::Staged(transaction, PrefixCacheWriteMode::Allow) => {
                transaction.store_prefix_cache(raw, clean_text, manifest);
            }
            Self::Staged(_, PrefixCacheWriteMode::Suppress) => {}
        }
    }
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

    pub fn restore_with_telemetry(
        &self,
        session: &Session,
        text: &str,
    ) -> Result<(RestoredText, RestoreTelemetry)> {
        self.restore_with_policy_telemetry(session, text, RestorePolicy::Strict)
    }

    pub fn restore_with_policy_telemetry(
        &self,
        session: &Session,
        text: &str,
        policy: RestorePolicy,
    ) -> Result<(RestoredText, RestoreTelemetry)> {
        let mut telemetry = RestoreTelemetry::new(policy);
        telemetry.phase_execution_mask |= RESTORE_PHASE_MANIFEST_LOOKUP;
        let restored = restore_known_tokens(session, text)?;
        telemetry.phase_execution_mask |=
            RESTORE_PHASE_UNKNOWN_TOKEN_SCAN | RESTORE_PHASE_MANIFEST_BYPASS_SCAN;
        let unknown_token_count = count_unknown_restore_tokens(session, &restored);
        telemetry.unknown_token_count = unknown_token_count;
        telemetry.manifest_bypass_count = unknown_token_count;
        telemetry.restore_decision = match (policy, unknown_token_count) {
            (_, 0) => RestoreDecision::Success,
            (RestorePolicy::Strict, _) => RestoreDecision::Failed,
            (RestorePolicy::Lenient, _) => RestoreDecision::Partial,
            (_, _) => RestoreDecision::Failed,
        };
        Ok((RestoredText::new(restored), telemetry))
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
        let mut target = ProtectionTarget::Live(session);
        self.pseudonymize_target_with_detect_context(&mut target, raw, locale_chain, dictionaries)
    }

    /// Runs the detector/rule pipeline against isolated transaction state.
    ///
    /// Mapping and prefix-cache publication occurs only when the caller commits
    /// the transaction. Configured [`RedactionLogger`] sinks still receive
    /// trusted-side attempt rows synchronously; discarding the transaction does
    /// not retract those metadata-only audit attempts.
    pub fn pseudonymize_transaction_with_detect_context(
        &self,
        transaction: &mut SessionTransaction<'_>,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<CleanDocument> {
        self.pseudonymize_transaction_with_detect_context_and_prefix_cache_write_mode(
            transaction,
            raw,
            locale_chain,
            dictionaries,
            PrefixCacheWriteMode::Allow,
        )
    }

    /// Runs transactional protection with an explicit prefix-cache mode.
    ///
    /// Suppression bypasses cache lookup and publication without disabling detection,
    /// tokenization, or manifest staging. The caller still owns the transaction and decides
    /// whether any staged mapping is committed.
    pub fn pseudonymize_transaction_with_detect_context_and_prefix_cache_write_mode(
        &self,
        transaction: &mut SessionTransaction<'_>,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
        prefix_cache_write_mode: PrefixCacheWriteMode,
    ) -> Result<CleanDocument> {
        let mut target = ProtectionTarget::Staged(transaction, prefix_cache_write_mode);
        self.pseudonymize_target_with_detect_context(&mut target, raw, locale_chain, dictionaries)
    }

    fn pseudonymize_target_with_detect_context(
        &self,
        target: &mut ProtectionTarget<'_, '_>,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<CleanDocument> {
        match raw {
            RawDocument::Structured(structured_fields) => redact_structured(
                self,
                target,
                structured_fields,
                DocumentKind::Structured,
                locale_chain,
                dictionaries,
            ),
            RawDocument::Text(text) => Ok(CleanDocument::Text(self.pseudonymize_text(
                target,
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
        let mut target = ProtectionTarget::Live(session);
        self.clean_target_with_safety_net_policy_detect_context(
            &mut target,
            raw,
            locale_chain,
            dictionaries,
            policy,
        )
    }

    /// Runs detector and safety-net protection against isolated transaction state.
    ///
    /// Mapping/cache publication and trusted-side audit-attempt behavior match
    /// [`Self::pseudonymize_transaction_with_detect_context`].
    pub fn clean_transaction_with_safety_net_policy_detect_context(
        &self,
        transaction: &mut SessionTransaction<'_>,
        raw: RawDocument,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
        policy: SafetyNetPolicy,
    ) -> Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)> {
        let mut target = ProtectionTarget::Staged(transaction, PrefixCacheWriteMode::Allow);
        self.clean_target_with_safety_net_policy_detect_context(
            &mut target,
            raw,
            locale_chain,
            dictionaries,
            policy,
        )
    }

    fn clean_target_with_safety_net_policy_detect_context(
        &self,
        target: &mut ProtectionTarget<'_, '_>,
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
                    target,
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
                    target,
                    &text,
                    None,
                    DocumentKind::Text,
                    locale_chain,
                    dictionaries,
                )?;
                let report = self.run_safety_nets(
                    target,
                    &clean.text,
                    &Manifest::from_spans(clean.manifest.clone()),
                    DocumentKind::Text,
                    locale_chain,
                    None,
                    policy.mode,
                )?;
                self.apply_safety_net_policy(
                    target,
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

        let mut target = ProtectionTarget::Live(session);
        let report = self.run_safety_nets(
            &mut target,
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
        let mut target = ProtectionTarget::Live(session);
        for (key, value) in document {
            walk_value_for_safety_net_scan(
                self,
                &mut target,
                value,
                key,
                locale_chain,
                &mut report,
            )?;
        }
        Ok(SafetyNetResult { nets_run, report })
    }

    pub fn restore_strict_text(&self, session: &Session, text: &str) -> Result<String> {
        match session.restore_strict_text_with_events(text) {
            Ok((restored, events)) => {
                if self.restore_boundary_dlp_audit {
                    self.log_restore_events(session, &events)?;
                }
                Ok(restored)
            }
            Err(Error::UnknownToken {
                class,
                ordinal,
                raw,
            }) => {
                self.log_restore_strict_rejection(session, class.clone(), ordinal)?;
                Err(Error::UnknownToken {
                    class,
                    ordinal,
                    raw,
                })
            }
            Err(err) => Err(err),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pseudonymize_text(
        &self,
        target: &mut ProtectionTarget<'_, '_>,
        text: &str,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<String> {
        Ok(self
            .redact_text_with_manifest(
                target,
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
        target: &mut ProtectionTarget<'_, '_>,
        text: &str,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<CleanText> {
        if self.optimization_config.prefix_cache {
            if let Some(hit) = target
                .lookup_prefix_cache(text)
                .filter(|hit| hit.raw_len < text.len())
            {
                let suffix = &text[hit.raw_len..];
                let suffix_clean = self.redact_text_with_manifest_uncached(
                    target,
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
                    target,
                    &hit.manifest,
                    field_name,
                    document_kind,
                    locale_chain,
                )?;
                target.store_prefix_cache(text, &clean_text, &manifest);
                return Ok(CleanText {
                    text: clean_text,
                    manifest,
                });
            }
        }

        let clean = self.redact_text_with_manifest_uncached(
            target,
            text,
            field_name,
            document_kind,
            locale_chain,
            dictionaries,
        )?;
        if self.optimization_config.prefix_cache {
            target.store_prefix_cache(text, &clean.text, &clean.manifest);
        }
        Ok(clean)
    }

    #[allow(clippy::too_many_arguments)]
    fn redact_text_with_manifest_uncached(
        &self,
        target: &mut ProtectionTarget<'_, '_>,
        text: &str,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> Result<CleanText> {
        let normalized = normalize(text);
        let spans = &normalized.spans;
        let ctx = DetectContext::new(locale_chain, dictionaries);
        let (resolved, vetoed) = self.registry.detect_all_resolved(&normalized.text, &ctx)?;
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
                target,
                loser,
                field_name,
                document_kind,
                self.action_for(&loser.detection, &build_context(field_name)),
                true,
            )?;
        }
        for vetoed in &vetoed {
            self.log_vetoed_entry(target, vetoed, field_name, document_kind)?;
        }

        detections.sort_by_key(|d| d.detection.span.start);
        let mut out = String::with_capacity(text.len());
        let mut emitted = Vec::with_capacity(detections.len());
        let mut cursor = 0usize;

        for detection in detections {
            let raw = text[detection.detection.span.clone()].to_string();
            let context = build_context(field_name);
            let action = self.action_for(&detection.detection, &context);
            self.log_entry(target, &detection, field_name, document_kind, action, false)?;

            let replacement = match action {
                Action::Tokenize => Some(target.tokenize_with_family(
                    &detection.family,
                    &detection.detection.class,
                    &raw,
                )?),
                Action::Redact => Some("[REDACTED]".to_string()),
                Action::FormatPreserve => {
                    Some(target.format_preserving_fake(&detection.detection.class, &raw)?)
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
        target: &mut ProtectionTarget<'_, '_>,
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
                Some(target.audit_session_id()),
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
            if selected.len() > 1 {
                let selected_backend = selected[0].name();
                let dropped = selected
                    .iter()
                    .skip(1)
                    .map(|backend| backend.name().to_string())
                    .collect::<Vec<_>>();
                tracing::debug!(
                    selected_backend,
                    backend_silently_dropped = ?dropped,
                    "locale-aware safety-net registry resolved multiple backends; using first"
                );
                self.log_backend_silently_dropped(
                    target,
                    document_kind,
                    field_path,
                    selected_backend,
                    dropped,
                )?;
            }
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
        target: &mut ProtectionTarget<'_, '_>,
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
                target,
                clean,
                report,
                document_kind,
                field_path,
                None,
                true,
            ),
            SafetyNetMode::Resolve => {
                let reason = match self.resolve_safety_net_suspects(
                    target,
                    clean,
                    report,
                    document_kind,
                    field_path,
                )? {
                    Some(reason) => Some(reason),
                    None => {
                        let follow_up = self.run_safety_nets(
                            target,
                            &clean.text,
                            &Manifest::from_spans(clean.manifest.clone()),
                            document_kind,
                            locale_chain,
                            field_path,
                            SafetyNetMode::Resolve,
                        )?;
                        self.post_resolution_fallback_reason(
                            target,
                            clean,
                            &follow_up,
                            document_kind,
                            field_path,
                        )?
                    }
                };
                if let Some(reason) = reason {
                    self.apply_safety_net_fallback(
                        target,
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
        target: &mut ProtectionTarget<'_, '_>,
        clean: &mut CleanText,
        report: &LeakReport,
        document_kind: DocumentKind,
        field_path: Option<&str>,
    ) -> Result<Option<FallbackReason>> {
        if report.suspects.is_empty() {
            return Ok(None);
        }
        validate_clean_manifest(clean)?;

        // Phase 1: classify. A suspect that lies wholly inside a live token is already
        // protected — acting on it would re-tokenize a token and destroy restore.
        let mut protected = Vec::new();
        let mut actionable = Vec::new();
        for suspect in &report.suspects {
            if suspect_is_inside_live_token(target, clean, suspect) {
                protected.push(suspect);
            } else if matches!(suspect.kind, LeakKind::ClassMismatch { .. }) {
                return Ok(Some(FallbackReason::OverlapConflict));
            } else {
                actionable.push(suspect);
            }
        }
        sort_safety_net_suspects(&mut protected);
        sort_safety_net_suspects(&mut actionable);

        // Phase 2: plan every resolution against the UNMUTATED clean text, so each plan's
        // coordinates are established before any of them is applied.
        let mut plans = Vec::with_capacity(actionable.len());
        for suspect in actionable {
            let span = suspect_action_span(suspect);
            if !is_char_boundary_range(&clean.text, &span) {
                return Ok(Some(FallbackReason::ResidualSuspect));
            }
            if !suspect_action_span_matches_manifest(clean, suspect) {
                return Ok(Some(FallbackReason::OverlapConflict));
            }
            // Establish the ORIGINAL-request interval before tokenization mutates the
            // session or any clean-text / manifest state.
            let raw_span = map_clean_span_to_raw(clean, &span)?;
            let raw = clean.text[span.clone()].to_string();
            plans.push(PlannedSafetyNetResolution {
                suspect,
                clean_span: span,
                raw_span,
                raw,
            });
        }
        // `plans` is sorted by clean-span start, so pairwise adjacency is sufficient.
        if plans
            .windows(2)
            .any(|pair| ranges_overlap(&pair[0].clean_span, &pair[1].clean_span))
        {
            return Ok(Some(FallbackReason::OverlapConflict));
        }

        for suspect in protected {
            self.log_safety_net_entry(
                target,
                suspect,
                document_kind,
                field_path,
                Action::Preserve,
                true,
                ConflictTier::Resolve,
                None,
            )?;
        }
        if plans.is_empty() {
            return Ok(None);
        }

        // Baseline for the restore-integrity check. `None` means the clean text was already
        // un-restorable before this pass (e.g. the source document quotes a foreign token);
        // exact-restore preservation is undefined there, so it is not this pass's invariant.
        let restore_baseline = target.restore_strict_text(&clean.text).ok();

        // Phase 3: apply right-to-left, so an applied plan never shifts an unapplied one.
        for plan in plans.into_iter().rev() {
            let suspect = plan.suspect;
            let replacement =
                target.tokenize_with_family("safety_net", &suspect.class, &plan.raw)?;
            self.log_safety_net_entry(
                target,
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
                plan.clean_span.clone(),
                &replacement,
                Some(EmittedTokenSpan::new(
                    plan.clean_span.start..plan.clean_span.start + replacement.len(),
                    plan.raw_span,
                    suspect.class.clone(),
                )),
            );
        }

        validate_clean_manifest(clean)?;
        if let Some(baseline) = restore_baseline {
            let restored = target
                .restore_strict_text(&clean.text)
                .map_err(|_| manifest_integrity_error("resolution left unrestorable clean text"))?;
            assert_resolution_preserved_restore(&baseline, &restored)?;
        }
        Ok(None)
    }

    /// Post-resolution verdict: replaces the bare `uncovered + partial_bleed > 0` count.
    ///
    /// Any residual suspect that is not already protected by a live token means the resolve pass
    /// did not converge, so the document takes the configured fallback. Residual suspects that
    /// *are* inside a live token are audited as no-ops rather than counted as residue — the net
    /// is re-flagging text the pipeline already tokenized.
    fn post_resolution_fallback_reason(
        &self,
        target: &mut ProtectionTarget<'_, '_>,
        clean: &CleanText,
        report: &LeakReport,
        document_kind: DocumentKind,
        field_path: Option<&str>,
    ) -> Result<Option<FallbackReason>> {
        if report.suspects.is_empty() {
            return Ok(None);
        }
        validate_clean_manifest(clean)?;
        let mut protected = Vec::new();
        for suspect in &report.suspects {
            if suspect_is_inside_live_token(target, clean, suspect) {
                protected.push(suspect);
                continue;
            }
            let reason = if matches!(suspect.kind, LeakKind::ClassMismatch { .. }) {
                FallbackReason::OverlapConflict
            } else {
                FallbackReason::ResidualSuspect
            };
            return Ok(Some(reason));
        }
        sort_safety_net_suspects(&mut protected);
        for suspect in protected {
            self.log_safety_net_entry(
                target,
                suspect,
                document_kind,
                field_path,
                Action::Preserve,
                true,
                ConflictTier::Resolve,
                None,
            )?;
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_safety_net_fallback(
        &self,
        target: &mut ProtectionTarget<'_, '_>,
        clean: &mut CleanText,
        report: &LeakReport,
        document_kind: DocumentKind,
        field_path: Option<&str>,
        fallback: SafetyNetFallback,
        reason: FallbackReason,
    ) -> Result<()> {
        for suspect in redaction_suspects(report) {
            self.log_safety_net_entry(
                target,
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
                target,
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
        target: &mut ProtectionTarget<'_, '_>,
        clean: &mut CleanText,
        report: &LeakReport,
        document_kind: DocumentKind,
        field_path: Option<&str>,
        fallback_reason: Option<FallbackReason>,
        log_entries: bool,
    ) -> Result<()> {
        // Redaction spans are derived from manifest coordinates via
        // `expand_span_to_overlapping_manifest_entries`. A manifest that misdescribes the
        // document would make the redactor delete the wrong bytes and can leave the flagged
        // PII in place, so a proven-inconsistent manifest must fail closed here rather than
        // produce a "degraded but safe" document.
        validate_clean_manifest(clean)?;
        for suspect in redaction_suspects(report).into_iter().rev() {
            let span =
                round_span_outward_to_char_boundaries(&clean.text, suspect_action_span(suspect))?;
            let span = expand_span_to_overlapping_manifest_entries(clean, span);
            for existing in clean
                .manifest
                .iter()
                .filter(|existing| ranges_overlap(&existing.clean_span, &span))
            {
                tracing::warn!(
                    class = ?existing.class,
                    clean_span_start = existing.clean_span.start,
                    clean_span_end = existing.clean_span.end,
                    "safety net redaction dropping overlapping manifest entry"
                );
            }
            if log_entries {
                self.log_safety_net_entry(
                    target,
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
        target: &ProtectionTarget<'_, '_>,
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
            Some(target.audit_session_id().to_string()),
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

    fn log_backend_silently_dropped(
        &self,
        target: &ProtectionTarget<'_, '_>,
        document_kind: DocumentKind,
        field_path: Option<&str>,
        selected_backend: &str,
        dropped: Vec<String>,
    ) -> Result<()> {
        let entry = RedactionEntry::new(
            format!("safety_net.{selected_backend}"),
            PiiClass::Custom("safety_net.backend".to_string()),
            Action::Preserve,
            field_path.map(str::to_string),
            document_kind,
            true,
            ConflictTier::None,
            crate::redaction_log::current_epoch_ms(),
            Some(target.audit_session_id().to_string()),
        )
        .with_backend_silently_dropped(dropped);
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
        target: &ProtectionTarget<'_, '_>,
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
            Some(target.audit_session_id().to_string()),
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
        target: &ProtectionTarget<'_, '_>,
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
                Some(target.audit_session_id().to_string()),
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

    fn log_restore_strict_rejection(
        &self,
        session: &Session,
        class: PiiClass,
        ordinal: u32,
    ) -> Result<()> {
        let entry = RedactionEntry::new(
            "restore_strict",
            class.clone(),
            Action::Preserve,
            None,
            DocumentKind::Text,
            true,
            ConflictTier::None,
            crate::redaction_log::current_epoch_ms(),
            Some(session.audit_session_id().to_string()),
        )
        .with_recognizer_metadata(Some("restore_strict".to_string()), None)
        .with_provenance_metadata(
            Some("restore_strict".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(class.to_canonical_str()),
            Some(class.to_canonical_str()),
            None,
            Some(format!("ordinal:{ordinal}")),
        );
        for logger in &self.redaction_loggers {
            logger.log(&entry)?;
        }
        Ok(())
    }

    fn log_restore_events(&self, session: &Session, events: &[RestoreEvent]) -> Result<()> {
        for event in events {
            let entry = RedactionEntry::new(
                event.kind.as_str(),
                event.class.clone(),
                Action::Preserve,
                None,
                DocumentKind::Text,
                false,
                ConflictTier::None,
                crate::redaction_log::current_epoch_ms(),
                Some(session.audit_session_id().to_string()),
            )
            .with_recognizer_metadata(Some("restore_boundary_dlp".to_string()), None)
            .with_provenance_metadata(
                Some("restore_boundary_dlp".to_string()),
                None,
                None,
                None,
                None,
                None,
                Some(event.kind.as_str().to_string()),
                Some(event.class.to_canonical_str()),
                Some(event.class.to_canonical_str()),
                None,
                Some(format!(
                    "sha256:{};span:{}..{}",
                    event.raw_sha256, event.location.start, event.location.end
                )),
            );
            for logger in &self.redaction_loggers {
                logger.log(&entry)?;
            }
        }
        Ok(())
    }

    fn log_vetoed_entry(
        &self,
        target: &ProtectionTarget<'_, '_>,
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
            Some(target.audit_session_id().to_string()),
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

struct PlannedSafetyNetResolution<'a> {
    suspect: &'a LeakSuspect,
    clean_span: Range<usize>,
    raw_span: Range<usize>,
    raw: String,
}

/// Provable manifest corruption. Reached only when the manifest contradicts the document it
/// describes, which no pipeline-produced manifest can do; see `validate_clean_manifest`.
///
/// This is deliberately the same closed error variant the rest of the safety-net surface uses:
/// the `manifest-integrity:` message prefix names the class without widening `Error`.
fn manifest_integrity_error(message: &'static str) -> Error {
    Error::SafetyNet(SafetyNetError::InvalidOutput {
        message: format!("manifest-integrity: {message}"),
    })
}

/// Every manifest entry must sit in a monotonic, gap-preserving clean/raw alignment: walking the
/// manifest from the document start must reach each entry's own clean span unambiguously.
///
/// Defense in depth. `redact_text_with_manifest` emits an entry for every replacing action, so a
/// pipeline-produced manifest satisfies this by construction and the check cannot fire from the
/// public surface. It guards the manifest-derived decisions that follow — span/manifest agreement,
/// live-token classification, redaction-span expansion — against a path that breaks the invariant,
/// instead of letting those decisions silently read a manifest that lies. The corruption shape it
/// rejects is an entry whose clean span was rewritten while its `raw_span` kept its old length.
fn validate_clean_manifest(clean: &CleanText) -> Result<()> {
    for emitted in &clean.manifest {
        map_clean_span_to_raw(clean, &emitted.clean_span).map_err(|_| {
            manifest_integrity_error("manifest entry has no unambiguous original span")
        })?;
    }
    Ok(())
}

/// A safety-net resolution replaces raw bytes with a token that restores to exactly those bytes,
/// so the restored document must be byte-identical before and after.
fn assert_resolution_preserved_restore(baseline: &str, restored: &str) -> Result<()> {
    if restored != baseline {
        return Err(manifest_integrity_error(
            "safety-net resolution broke exact restore",
        ));
    }
    Ok(())
}

/// True when the suspect span lies wholly inside exactly one live token.
///
/// Such a suspect is the net re-flagging text the pipeline already protected. Acting on it would
/// tokenize a token and permanently break restore, so it is audited as a no-op instead.
fn suspect_is_inside_live_token(
    target: &ProtectionTarget<'_, '_>,
    clean: &CleanText,
    suspect: &LeakSuspect,
) -> bool {
    let span = &suspect.span;
    if span.start >= span.end || !is_char_boundary_range(&clean.text, span) {
        return false;
    }
    let mut containing = clean.manifest.iter().filter(|emitted| {
        emitted.clean_span.start <= span.start && span.end <= emitted.clean_span.end
    });
    let Some(emitted) = containing.next() else {
        return false;
    };
    if containing.next().is_some() {
        return false;
    }
    let Some(token) = clean.text.get(emitted.clean_span.clone()) else {
        return false;
    };
    let Some(restored) = target.restore(token) else {
        return false;
    };
    target.contains_token(token)
        && emitted.raw_span.start < emitted.raw_span.end
        && restored.len() == emitted.raw_span.end - emitted.raw_span.start
}

/// True when the suspect's own coverage claim matches what the manifest actually covers.
///
/// `Uncovered` must touch no emitted token, and `PartialBleed { uncovered }` must name the single
/// gap the manifest leaves inside the suspect span. A suspect that disagrees with the manifest is
/// stale or wrong; resolving it would tokenize across live tokens.
fn suspect_action_span_matches_manifest(clean: &CleanText, suspect: &LeakSuspect) -> bool {
    if suspect.span.start >= suspect.span.end || !is_char_boundary_range(&clean.text, &suspect.span)
    {
        return false;
    }
    match &suspect.kind {
        LeakKind::Uncovered => !clean
            .manifest
            .iter()
            .any(|emitted| ranges_overlap(&emitted.clean_span, &suspect.span)),
        LeakKind::PartialBleed { uncovered } => {
            if uncovered.start >= uncovered.end
                || !is_char_boundary_range(&clean.text, uncovered)
                || uncovered.start < suspect.span.start
                || uncovered.end > suspect.span.end
            {
                return false;
            }
            let mut cursor = suspect.span.start;
            let mut gaps = Vec::new();
            for emitted in clean
                .manifest
                .iter()
                .filter(|emitted| ranges_overlap(&emitted.clean_span, &suspect.span))
            {
                let covered_start = emitted.clean_span.start.max(suspect.span.start);
                let covered_end = emitted.clean_span.end.min(suspect.span.end);
                if cursor < covered_start {
                    gaps.push(cursor..covered_start);
                }
                cursor = cursor.max(covered_end);
            }
            if cursor < suspect.span.end {
                gaps.push(cursor..suspect.span.end);
            }
            gaps.as_slice() == [uncovered.clone()]
        }
        LeakKind::ClassMismatch { .. } => false,
        _ => false,
    }
}

/// Total order over suspects. The pairwise overlap check below is only correct on a sorted list,
/// and the tie-breakers keep audit-row order independent of safety-net iteration order.
fn sort_safety_net_suspects(suspects: &mut [&LeakSuspect]) {
    suspects.sort_by(|left, right| {
        (
            left.span.start,
            left.span.end,
            left.class.to_canonical_str(),
            left.safety_net_id.as_str(),
        )
            .cmp(&(
                right.span.start,
                right.span.end,
                right.class.to_canonical_str(),
                right.safety_net_id.as_str(),
            ))
    });
}

fn clean_to_raw_mapping_error(message: &'static str) -> Error {
    Error::SafetyNet(SafetyNetError::InvalidOutput {
        message: message.to_string(),
    })
}

/// Translate a CLEAN-text span into the ORIGINAL-request byte interval it covers.
///
/// `EmittedTokenSpan::raw_span` is an original-document coordinate by contract: downstream
/// consumers slice the original request bytes with it (`gaze-token-bridge` ingest) and compare
/// it against authorized original-coordinate ranges (`gaze-proxy` residual validation). The
/// safety-net RESOLVE path only ever sees clean coordinates, so it must map them back through
/// the manifest emitted by the primary pass before recording a manifest entry.
///
/// Fails closed when the mapping is ambiguous — a clean boundary that lands strictly inside an
/// already-emitted token has no single original counterpart, and a manifest that is not a
/// monotonic, gap-preserving clean/raw alignment cannot be walked at all.
fn map_clean_span_to_raw(clean: &CleanText, span: &Range<usize>) -> Result<Range<usize>> {
    if span.start >= span.end || !is_char_boundary_range(&clean.text, span) {
        return Err(Error::SafetyNetSpanInvalid {
            start: span.start,
            end: span.end,
            text_len: clean.text.len(),
        });
    }
    let start = map_clean_boundary_to_raw(&clean.manifest, span.start)
        .ok_or_else(|| clean_to_raw_mapping_error("clean-to-raw start mapping failed"))?;
    let end = map_clean_boundary_to_raw(&clean.manifest, span.end)
        .ok_or_else(|| clean_to_raw_mapping_error("clean-to-raw end mapping failed"))?;
    if start >= end {
        return Err(clean_to_raw_mapping_error("empty clean-to-raw mapping"));
    }
    Ok(start..end)
}

/// Map a single clean-text byte boundary to its original-request byte boundary.
///
/// Walks the manifest in clean order, carrying a clean cursor and a raw cursor: untokenized
/// runs advance both by the same amount, and each emitted token jumps them to its own
/// clean/raw ends. Returns `None` (caller fails closed) when the offset falls strictly inside
/// an emitted token, or when the manifest is not monotonic / not gap-preserving.
fn map_clean_boundary_to_raw(manifest: &[EmittedTokenSpan], offset: usize) -> Option<usize> {
    let mut clean_cursor = 0usize;
    let mut raw_cursor = 0usize;
    for emitted in manifest {
        if emitted.clean_span.start < clean_cursor
            || emitted.raw_span.start < raw_cursor
            || emitted.clean_span.start - clean_cursor != emitted.raw_span.start - raw_cursor
        {
            return None;
        }
        if offset < emitted.clean_span.start {
            return raw_cursor.checked_add(offset.checked_sub(clean_cursor)?);
        }
        if offset == emitted.clean_span.start {
            return Some(emitted.raw_span.start);
        }
        if offset < emitted.clean_span.end {
            return None;
        }
        if offset == emitted.clean_span.end {
            return Some(emitted.raw_span.end);
        }
        clean_cursor = emitted.clean_span.end;
        raw_cursor = emitted.raw_span.end;
    }
    raw_cursor.checked_add(offset.checked_sub(clean_cursor)?)
}

fn is_char_boundary_range(text: &str, span: &Range<usize>) -> bool {
    span.start <= span.end
        && span.end <= text.len()
        && text.is_char_boundary(span.start)
        && text.is_char_boundary(span.end)
}

fn round_span_outward_to_char_boundaries(text: &str, span: Range<usize>) -> Result<Range<usize>> {
    if span.start > span.end || span.end > text.len() {
        return Err(Error::SafetyNetSpanInvalid {
            start: span.start,
            end: span.end,
            text_len: text.len(),
        });
    }

    let mut start = span.start;
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }

    let mut end = span.end;
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }

    if !is_char_boundary_range(text, &(start..end)) {
        return Err(Error::SafetyNetSpanInvalid {
            start: span.start,
            end: span.end,
            text_len: text.len(),
        });
    }

    Ok(start..end)
}

fn expand_span_to_overlapping_manifest_entries(
    clean: &CleanText,
    span: Range<usize>,
) -> Range<usize> {
    let mut expanded = span;
    loop {
        let mut changed = false;
        for existing in &clean.manifest {
            if ranges_overlap(&existing.clean_span, &expanded) {
                let start = expanded.start.min(existing.clean_span.start);
                let end = expanded.end.max(existing.clean_span.end);
                changed |= start != expanded.start || end != expanded.end;
                expanded = start..end;
            }
        }
        if !changed {
            return expanded;
        }
    }
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
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

fn restore_known_tokens(session: &Session, text: &str) -> Result<String> {
    let Some(re) = session.restore_regex()? else {
        return Ok(text.to_string());
    };
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for matched in re.find_iter(text) {
        out.push_str(&text[last..matched.start()]);
        out.push_str(&session.restore_strict(matched.as_str())?);
        last = matched.end();
    }
    out.push_str(&text[last..]);
    Ok(out)
}

fn count_unknown_restore_tokens(session: &Session, text: &str) -> u64 {
    crate::token_shape::pattern()
        .find_iter(text)
        .filter(|matched| {
            let matched_text = matched.as_str();
            crate::token_shape::is_trap(matched_text) || !session.contains_token(matched_text)
        })
        .count() as u64
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
    restore_boundary_dlp_audit: bool,
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

    pub fn enable_restore_boundary_dlp_audit(mut self) -> Self {
        self.restore_boundary_dlp_audit = true;
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
            restore_boundary_dlp_audit: self.restore_boundary_dlp_audit,
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
    target: &mut ProtectionTarget<'_, '_>,
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
                target,
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
    target: &mut ProtectionTarget<'_, '_>,
    value: Value,
    field_name: &str,
    field_path: &str,
    document_kind: DocumentKind,
    locale_chain: &[crate::LocaleTag],
    dictionaries: &DictionaryBundle,
) -> Result<Value> {
    match value {
        Value::String(text) => Ok(Value::String(pipeline.pseudonymize_text(
            target,
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
                    target,
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
                        target,
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
    target: &mut ProtectionTarget<'_, '_>,
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
                target,
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
    target: &mut ProtectionTarget<'_, '_>,
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
                target,
                &text,
                Some(field_name),
                DocumentKind::Structured,
                locale_chain,
                dictionaries,
            )?;
            // For RawDocument::Structured, locale gating uses the session-level
            // locale chain across all fields; fields have no locale annotations.
            let field_report = pipeline.run_safety_nets(
                target,
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
                    target,
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
                        target,
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
                    target,
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
    target: &mut ProtectionTarget<'_, '_>,
    value: &Value,
    field_path: &str,
    locale_chain: &[crate::LocaleTag],
    report: &mut LeakReport,
) -> Result<()> {
    match value {
        Value::String(text) => {
            if !text.is_empty() {
                let field_report = pipeline.run_safety_nets(
                    target,
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
                    target,
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
                    target,
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
                    target,
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

    fn detect(
        &self,
        input: &str,
        _ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self
            .detector
            .try_detect(input)
            .map_err(|err| gaze_types::DetectError::backend(err.recognizer_id, err.message))?
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
            .collect())
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

    struct FailingRecognizer;

    impl Recognizer for FailingRecognizer {
        fn id(&self) -> &str {
            "ner"
        }

        fn supported_class(&self) -> &PiiClass {
            &PiiClass::Name
        }

        fn detect(
            &self,
            _input: &str,
            _ctx: &DetectContext<'_>,
        ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
            Err(gaze_types::DetectError::backend(
                self.id(),
                "synthetic backend failure",
            ))
        }

        fn token_family(&self) -> &str {
            "counter"
        }
    }

    struct NeedleDetector {
        needle: &'static str,
        class: PiiClass,
        source: &'static str,
    }

    impl Detector for NeedleDetector {
        fn detect(&self, input: &str) -> Vec<Detection> {
            input
                .match_indices(self.needle)
                .map(|(start, matched)| {
                    Detection::new(
                        start..start + matched.len(),
                        self.class.clone(),
                        self.source,
                    )
                })
                .collect()
        }
    }

    struct DictionaryRecognizer {
        class: PiiClass,
    }

    impl Recognizer for DictionaryRecognizer {
        fn id(&self) -> &str {
            "dictionary.synthetic_ids"
        }

        fn supported_class(&self) -> &PiiClass {
            &self.class
        }

        fn detect(
            &self,
            input: &str,
            ctx: &DetectContext<'_>,
        ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
            let Some(dictionary) = ctx.dictionaries.get("synthetic_ids") else {
                return Ok(Vec::new());
            };
            Ok(dictionary
                .terms()
                .iter()
                .flat_map(|term| input.match_indices(term))
                .map(|(start, matched)| {
                    Candidate::new(
                        start..start + matched.len(),
                        self.class.clone(),
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
                .collect())
        }

        fn token_family(&self) -> &str {
            "dictionary"
        }
    }

    #[cfg(feature = "bundled-recognizers")]
    struct DeterministicNerDetector;

    #[cfg(feature = "bundled-recognizers")]
    impl Detector for DeterministicNerDetector {
        fn detect(&self, input: &str) -> Vec<Detection> {
            let Some(start) = input.find("Dr. Schmidt") else {
                return Vec::new();
            };
            let labels =
                gaze_recognizers::LabelMap(BTreeMap::from([("PER".to_string(), PiiClass::Name)]));
            gaze_recognizers::NerDetector::merge_bio_spans(
                &labels,
                &[
                    (start, start + "Dr.".len()),
                    (start + "Dr. ".len(), start + "Dr. Schmidt".len()),
                ],
                &["B-PER", "I-PER"],
                "ner/deterministic",
            )
        }
    }

    struct ResolveSafetyNet;

    impl SafetyNet for ResolveSafetyNet {
        fn id(&self) -> &str {
            "resolve-fixture"
        }

        fn supported_locales(&self) -> &[crate::LocaleTag] {
            &[crate::LocaleTag::Global]
        }

        fn check(
            &self,
            clean_text: &str,
            _context: SafetyNetContext<'_>,
        ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
            Ok(clean_text
                .find("Dr. Schmidt")
                .map(|start| {
                    LeakSuspect::new(
                        start..start + "Dr. Schmidt".len(),
                        PiiClass::Name,
                        self.id(),
                        Some(1.0),
                        LeakKind::Uncovered,
                        "PER",
                        None,
                    )
                })
                .into_iter()
                .collect())
        }
    }

    fn normalized_log_entries(entries: &[RedactionEntry]) -> Vec<RedactionEntry> {
        entries
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.created_at = 0;
                entry
            })
            .collect()
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

    #[test]
    fn recognizer_backend_failure_fails_closed_before_output() {
        let pipeline = Pipeline::builder()
            .recognizer(FailingRecognizer)
            .build()
            .expect("pipeline");
        let session =
            Session::new(Scope::Conversation("detect-failclosed".to_string())).expect("session");

        let err = pipeline
            .redact(&session, RawDocument::Text("Hello Dr. Schmidt".to_string()))
            .expect_err("recognizer backend failure must abort redaction");

        assert!(matches!(
            err,
            Error::RecognizerDetect(gaze_types::DetectError::Backend {
                recognizer_id,
                ..
            }) if recognizer_id == "ner"
        ));
    }

    impl RedactionLogger for CapturingLogger {
        fn log(&self, entry: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
            self.entries.lock().unwrap().push(entry.clone());
            Ok(())
        }
    }

    #[test]
    fn restore_with_telemetry_counts_unknown_tokens_deterministically() {
        let pipeline = Pipeline::builder().build().expect("pipeline");
        let session =
            Session::new(Scope::Conversation("restore-telemetry".to_string())).expect("session");
        let clean = pipeline
            .redact(
                &session,
                RawDocument::Text("Reach alice@example.invalid".to_string()),
            )
            .expect("redact");
        let CleanDocument::Text(clean) = clean else {
            panic!("expected text");
        };
        let input = format!("{clean} <Email_999> <Name_100>");

        let (restored, telemetry) = pipeline
            .restore_with_policy_telemetry(&session, &input, RestorePolicy::Lenient)
            .expect("restore telemetry");
        let (_, telemetry_again) = pipeline
            .restore_with_policy_telemetry(&session, &input, RestorePolicy::Lenient)
            .expect("restore telemetry");

        assert!(restored.text.contains("alice@example.invalid"));
        assert_eq!(telemetry.unknown_token_count, 2);
        assert_eq!(telemetry.manifest_bypass_count, 2);
        assert_eq!(telemetry.fresh_pii_detected_count, 0);
        assert_eq!(telemetry.restore_policy, RestorePolicy::Lenient);
        assert_eq!(telemetry.restore_decision, RestoreDecision::Partial);
        assert_eq!(
            telemetry.phase_execution_mask,
            RESTORE_PHASE_MANIFEST_LOOKUP
                | RESTORE_PHASE_UNKNOWN_TOKEN_SCAN
                | RESTORE_PHASE_MANIFEST_BYPASS_SCAN
        );
        assert_eq!(telemetry_again, telemetry);
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

    /// `validate_clean_manifest` is defense in depth: `redact_text_with_manifest` emits an entry
    /// for every replacing action, so a pipeline-produced manifest is gap-preserving and this
    /// cannot fire from the public surface. Probe it directly so the check is not unfalsifiable.
    #[test]
    fn validate_clean_manifest_rejects_a_manifest_that_contradicts_its_own_alignment() {
        // "<tok> tail" where the entry claims to stand for 21 raw bytes.
        let sound = CleanText {
            text: "<aabbccdd:Email_1> tail".to_string(),
            manifest: vec![EmittedTokenSpan::new(0..18, 0..21, PiiClass::Email)],
        };
        validate_clean_manifest(&sound).expect("a gap-preserving manifest must validate");

        // Same document, but the second entry's raw_span cannot follow the first: one clean byte
        // separates the entries while four raw bytes do, so the manifest describes a document
        // that cannot exist. This is the shape a rewritten clean span leaves behind.
        let corrupt = CleanText {
            text: "<aabbccdd:Email_1> tail".to_string(),
            manifest: vec![
                EmittedTokenSpan::new(0..18, 0..21, PiiClass::Email),
                EmittedTokenSpan::new(19..23, 25..29, PiiClass::Email),
            ],
        };
        let error = validate_clean_manifest(&corrupt)
            .expect_err("a manifest that contradicts its own alignment must fail closed");
        assert!(
            format!("{error}").contains("manifest-integrity"),
            "manifest corruption must surface as the manifest-integrity class, got {error}"
        );
    }

    /// The redact path derives its deletion spans from manifest coordinates
    /// (`expand_span_to_overlapping_manifest_entries`), so it must refuse a manifest that is
    /// already proven inconsistent: expanding against a manifest that misdescribes the document
    /// can delete the wrong bytes and leave the flagged PII in place.
    ///
    /// No public-API fixture can present a corrupt manifest to this path — the primary pass is
    /// gap-preserving by construction — so the precondition is probed here, at the call site, to
    /// keep it falsifiable rather than unverifiable.
    #[test]
    fn redact_safety_net_suspects_refuses_a_proven_inconsistent_manifest() {
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let mut target = ProtectionTarget::Live(&session);
        // One clean byte separates the entries while four raw bytes do: an alignment no document
        // can have.
        let mut clean = CleanText {
            text: "<aabbccdd:Email_1> tail".to_string(),
            manifest: vec![
                EmittedTokenSpan::new(0..18, 0..21, PiiClass::Email),
                EmittedTokenSpan::new(19..23, 25..29, PiiClass::Email),
            ],
        };
        let report = LeakReport::from_parts(
            vec![LeakSuspect::new(
                19..23,
                PiiClass::Email,
                "probe",
                Some(0.99),
                LeakKind::Uncovered,
                "private_email",
                None,
            )],
            Vec::new(),
        );

        let error = pipeline
            .redact_safety_net_suspects(
                &mut target,
                &mut clean,
                &report,
                DocumentKind::Text,
                None,
                None,
                false,
            )
            .expect_err("redaction on a proven-inconsistent manifest must fail closed");
        assert!(
            format!("{error}").contains("manifest-integrity"),
            "expected the manifest-integrity class, got {error}"
        );
    }

    /// A resolution replaces raw bytes with a token restoring to exactly those bytes, so the
    /// restored document is byte-identical before and after. Probed directly for the same reason.
    #[test]
    fn assert_resolution_preserved_restore_rejects_a_changed_restore() {
        assert_resolution_preserved_restore(
            "alice@example.invalid tail",
            "alice@example.invalid tail",
        )
        .expect("an unchanged restore must pass");
        let error = assert_resolution_preserved_restore(
            "alice@example.invalid tail",
            "alice@example.invalid",
        )
        .expect_err("a resolution that changed the restored document must fail closed");
        assert!(
            format!("{error}").contains("manifest-integrity"),
            "broken restore must surface as the manifest-integrity class, got {error}"
        );
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
            fn detect(
                &self,
                input: &str,
                _ctx: &DetectContext<'_>,
            ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
                Ok(input
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
                    .collect())
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
    fn restore_strict_rejection_emits_audit_provenance() {
        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let pipeline = Pipeline::builder()
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");

        let err = pipeline
            .restore_strict_text(&session, "Reply to <deadbeef:Email_1>.")
            .expect_err("unknown token must fail closed");

        assert!(matches!(
            err,
            Error::UnknownToken {
                class: PiiClass::Email,
                ordinal: 1,
                raw,
            } if raw == "<deadbeef:Email_1>"
        ));
        let entries = entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, "restore_strict");
        assert_eq!(entries[0].class, PiiClass::Email);
        assert_eq!(entries[0].action, Action::Preserve);
        assert_eq!(
            entries[0].provenance_stage.as_deref(),
            Some("restore_strict")
        );
    }

    #[test]
    fn restore_boundary_dlp_emits_metadata_only_audit_rows() {
        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let pipeline = Pipeline::builder()
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .enable_restore_boundary_dlp_audit()
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let token = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("token");

        let restored = pipeline
            .restore_strict_text(
                &session,
                &format!("{token} raw alice@example.invalid fresh bob@example.invalid"),
            )
            .expect("restore remains audit-only");

        assert!(restored.contains("alice@example.invalid"));
        let entries = entries.lock().unwrap();
        let dlp_entries = entries
            .iter()
            .filter(|entry| entry.provenance_stage.as_deref() == Some("restore_boundary_dlp"))
            .collect::<Vec<_>>();
        assert_eq!(dlp_entries.len(), 2);
        assert!(dlp_entries
            .iter()
            .any(|entry| entry.source == "manifest_bypass"));
        assert!(dlp_entries
            .iter()
            .any(|entry| entry.source == "fresh_pii_detected"));
        let serialized = serde_json::to_string(&dlp_entries).expect("audit json");
        assert!(!serialized.contains("alice@example.invalid"));
        assert!(!serialized.contains("bob@example.invalid"));
        assert!(serialized.contains("sha256:"));
    }

    #[test]
    fn restore_boundary_dlp_audit_is_default_off() {
        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let pipeline = Pipeline::builder()
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");

        pipeline
            .restore_strict_text(&session, "fresh bob@example.invalid")
            .expect("restore remains audit-only");

        assert!(entries.lock().unwrap().is_empty());
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

            fn detect(
                &self,
                input: &str,
                _ctx: &DetectContext<'_>,
            ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
                let Some(start) = input.find("Dr. Schmidt") else {
                    return Ok(Vec::new());
                };
                let end = start + "Dr. Schmidt".len();
                Ok(vec![Candidate::new(
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
                .with_recognizer_version_id("name.semantic.v2")])
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

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn locale_safety_net_registry_logs_dropped_backends() {
        struct EmptyLocaleModel {
            name: &'static str,
            locales: Vec<crate::LocaleTag>,
        }

        impl gaze_recognizers::LocaleAwareModel for EmptyLocaleModel {
            fn name(&self) -> &str {
                self.name
            }

            fn native_locales(&self) -> &[crate::LocaleTag] {
                &self.locales
            }

            fn infer(
                &self,
                _input: ModelInput,
                _hints: ModelHints,
            ) -> std::result::Result<Vec<ModelSpan>, ModelError> {
                Ok(Vec::new())
            }
        }

        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let registry = LocaleAwareModelRegistry::from_backends(vec![
            Box::new(EmptyLocaleModel {
                name: "opf-primary",
                locales: vec![crate::LocaleTag::EnUs],
            }),
            Box::new(EmptyLocaleModel {
                name: "opf-shadow",
                locales: vec![crate::LocaleTag::EnUs],
            }),
        ]);
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .register_safety_net_registry(registry)
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");

        pipeline
            .scan_safety_nets(&session, "plain text", &[crate::LocaleTag::EnUs])
            .expect("safety net scan");

        let entries = entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].backend_silently_dropped.as_deref(),
            Some(&["opf-shadow".to_string()][..])
        );
        assert_eq!(entries[0].source, "safety_net.opf-primary");
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

            fn detect(
                &self,
                input: &str,
                _ctx: &DetectContext<'_>,
            ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
                let Some(start) = input.find("Dr. Schmidt") else {
                    return Ok(Vec::new());
                };
                let end = start + "Dr. Schmidt".len();
                Ok(vec![Candidate::new(
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
                )])
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
        assert!(regex::Regex::new(r"^<[0-9a-f]{8}:Name_[0-9]+>$")
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

    #[test]
    fn transaction_pipeline_detector_rule_dictionary_parity_without_publish() {
        let custom_class = PiiClass::Custom("synthetic_id".to_string());
        let dictionaries = DictionaryBundle::from_entries([(
            "synthetic_ids".to_string(),
            gaze_types::DictionaryEntry::new(
                "synthetic_ids",
                vec!["SYN-0001".to_string()],
                true,
                gaze_types::DictionarySource::Cli,
            )
            .expect("synthetic dictionary"),
        )]);
        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let pipeline = Pipeline::builder()
            .detector(NeedleDetector {
                needle: "alice@example.invalid",
                class: PiiClass::Email,
                source: "email.fixture",
            })
            .recognizer(DictionaryRecognizer {
                class: custom_class.clone(),
            })
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(ClassRule::new(custom_class, Action::FormatPreserve))
            .rule(DefaultRule::new(Action::Preserve))
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let raw = RawDocument::Structured(BTreeMap::from([
            (
                "contact".to_string(),
                Value::String("alice@example.invalid".to_string()),
            ),
            (
                "reference".to_string(),
                Value::String("SYN-0001".to_string()),
            ),
        ]));

        let mut transaction = session.begin_transaction();
        let staged = pipeline
            .pseudonymize_transaction_with_detect_context(
                &mut transaction,
                raw.clone(),
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("staged protection");
        assert_eq!(session.tokens(), Vec::<String>::new());
        assert_eq!(transaction.tokens().len(), 2);
        let staged_logs = {
            let captured = entries.lock().unwrap();
            assert_eq!(captured.len(), 2);
            normalized_log_entries(&captured)
        };
        drop(transaction);
        assert_eq!(session.tokens(), Vec::<String>::new());
        assert_eq!(
            entries.lock().unwrap().len(),
            2,
            "trusted attempt rows survive discard"
        );
        entries.lock().unwrap().clear();

        let live = pipeline
            .pseudonymize_with_detect_context(
                &session,
                raw,
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("live protection");
        let live_logs = normalized_log_entries(&entries.lock().unwrap());
        let (CleanDocument::Structured(staged), CleanDocument::Structured(live)) = (staged, live)
        else {
            panic!("expected structured outputs");
        };
        assert_eq!(staged, live);
        assert_eq!(staged_logs, live_logs);
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn transaction_pipeline_bundled_deterministic_ner_parity_without_publish() {
        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let pipeline = Pipeline::builder()
            .detector(DeterministicNerDetector)
            .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let dictionaries = DictionaryBundle::default();
        let raw = RawDocument::Text("Assigned to Dr. Schmidt".to_string());

        let mut transaction = session.begin_transaction();
        let staged = pipeline
            .pseudonymize_transaction_with_detect_context(
                &mut transaction,
                raw.clone(),
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("staged NER protection");
        assert_eq!(session.tokens(), Vec::<String>::new());
        let staged_logs = normalized_log_entries(&entries.lock().unwrap());
        drop(transaction);
        entries.lock().unwrap().clear();

        let live = pipeline
            .pseudonymize_with_detect_context(
                &session,
                raw,
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("live NER protection");
        let (CleanDocument::Text(staged), CleanDocument::Text(live)) = (staged, live) else {
            panic!("expected text outputs");
        };
        assert_eq!(staged, live);
        assert_eq!(
            staged_logs,
            normalized_log_entries(&entries.lock().unwrap())
        );
    }

    #[test]
    fn transaction_pipeline_safety_net_resolve_parity_without_publish() {
        let entries = Arc::new(Mutex::new(Vec::<RedactionEntry>::new()));
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(ResolveSafetyNet)
            .redaction_logger(CapturingLogger {
                entries: Arc::clone(&entries),
            })
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let dictionaries = DictionaryBundle::default();
        let raw = RawDocument::Text("Assigned to Dr. Schmidt".to_string());
        let policy = SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Redact);

        let mut transaction = session.begin_transaction();
        let staged = pipeline
            .clean_transaction_with_safety_net_policy_detect_context(
                &mut transaction,
                raw.clone(),
                &[crate::LocaleTag::Global],
                &dictionaries,
                policy,
            )
            .expect("staged safety-net protection");
        assert_eq!(session.tokens(), Vec::<String>::new());
        assert_eq!(transaction.tokens().len(), 1);
        let staged_logs = normalized_log_entries(&entries.lock().unwrap());
        drop(transaction);
        assert_eq!(session.tokens(), Vec::<String>::new());
        entries.lock().unwrap().clear();

        let live = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                raw,
                &[crate::LocaleTag::Global],
                &dictionaries,
                policy,
            )
            .expect("live safety-net protection");
        let (staged_document, staged_spans, staged_report) = staged;
        let (live_document, live_spans, live_report) = live;
        let (CleanDocument::Text(staged_text), CleanDocument::Text(live_text)) =
            (staged_document, live_document)
        else {
            panic!("expected text outputs");
        };
        assert_eq!(staged_text, live_text);
        assert_eq!(staged_spans, live_spans);
        assert_eq!(staged_report, live_report);
        assert_eq!(
            staged_logs,
            normalized_log_entries(&entries.lock().unwrap())
        );
    }

    #[test]
    fn transaction_pipeline_prefix_cache_is_staged_and_drop_discards() {
        let pipeline = Pipeline::builder()
            .detector(NeedleDetector {
                needle: "Dr. Schmidt",
                class: PiiClass::Name,
                source: "name.fixture",
            })
            .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .enable_prefix_cache()
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let dictionaries = DictionaryBundle::default();

        let mut transaction = session.begin_transaction();
        let first = pipeline
            .pseudonymize_transaction_with_detect_context(
                &mut transaction,
                RawDocument::Text("Dr. Schmidt".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("prime staged cache");
        let extended = pipeline
            .pseudonymize_transaction_with_detect_context(
                &mut transaction,
                RawDocument::Text("Dr. Schmidt reports".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("hit staged cache");
        let (CleanDocument::Text(first), CleanDocument::Text(extended)) = (first, extended) else {
            panic!("expected text outputs");
        };
        assert_eq!(extended, format!("{first} reports"));
        assert_eq!(transaction.tokens().len(), 1);
        assert!(session.tokens().is_empty());
        assert!(session.lookup_prefix_cache("Dr. Schmidt reports").is_none());
        drop(transaction);
        assert!(session.tokens().is_empty());
        assert!(session.lookup_prefix_cache("Dr. Schmidt reports").is_none());

        let mut transaction = session.begin_transaction();
        pipeline
            .pseudonymize_transaction_with_detect_context(
                &mut transaction,
                RawDocument::Text("Dr. Schmidt".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("prime committed cache");
        pipeline
            .pseudonymize_transaction_with_detect_context(
                &mut transaction,
                RawDocument::Text("Dr. Schmidt reports".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("hit committed cache");
        let staged_tokens = transaction.tokens();
        let snapshot = transaction.commit().expect("atomic commit");
        assert_eq!(snapshot.tokens(), staged_tokens);
        assert_eq!(session.tokens(), staged_tokens);
        assert!(session.lookup_prefix_cache("Dr. Schmidt reports").is_some());
    }

    #[test]
    fn transaction_pipeline_suppression_bypasses_prefix_cache_lookup_and_store() {
        let pipeline = Pipeline::builder()
            .detector(NeedleDetector {
                needle: "alice@example.invalid",
                class: PiiClass::Email,
                source: "email.fixture",
            })
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .enable_prefix_cache()
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let dictionaries = DictionaryBundle::default();
        let mut transaction = session.begin_transaction();

        let cached_prefix = pipeline
            .pseudonymize_transaction_with_detect_context(
                &mut transaction,
                RawDocument::Text("alice@".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("prime staged prefix cache");
        let CleanDocument::Text(cached_prefix) = cached_prefix else {
            panic!("expected text output");
        };
        assert_eq!(cached_prefix, "alice@");
        assert_eq!(
            transaction
                .lookup_prefix_cache("alice@example.invalid")
                .expect("preseeded prefix cache hit")
                .raw_len,
            "alice@".len()
        );
        #[cfg(feature = "test-support")]
        let staged_cache_entries = transaction.prefix_cache_entry_count();
        #[cfg(feature = "test-support")]
        let live_cache_entries = session.prefix_cache_entry_count();

        let protected = pipeline
            .pseudonymize_transaction_with_detect_context_and_prefix_cache_write_mode(
                &mut transaction,
                RawDocument::Text("alice@example.invalid".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
                PrefixCacheWriteMode::Suppress,
            )
            .expect("suppression probe");
        let CleanDocument::Text(protected) = protected else {
            panic!("expected text output");
        };
        assert_ne!(protected, "alice@example.invalid");
        assert_eq!(transaction.tokens().len(), 1);
        #[cfg(feature = "test-support")]
        {
            assert_eq!(transaction.prefix_cache_entry_count(), staged_cache_entries);
            assert_eq!(session.prefix_cache_entry_count(), live_cache_entries);
        }

        drop(transaction);
        assert!(session.tokens().is_empty());
        assert!(session
            .lookup_prefix_cache("alice@example.invalid")
            .is_none());
    }

    #[test]
    fn transaction_pipeline_generation_conflict_publishes_no_staged_mapping() {
        let pipeline = Pipeline::builder()
            .detector(NeedleDetector {
                needle: "Dr. Schmidt",
                class: PiiClass::Name,
                source: "name.fixture",
            })
            .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .enable_prefix_cache()
            .build()
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");
        let dictionaries = DictionaryBundle::default();
        let mut transaction = session.begin_transaction();
        pipeline
            .pseudonymize_transaction_with_detect_context(
                &mut transaction,
                RawDocument::Text("Dr. Schmidt".to_string()),
                &[crate::LocaleTag::Global],
                &dictionaries,
            )
            .expect("stage mapping");
        let staged_token = transaction.tokens().pop().expect("staged token");
        let winner = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("winning live mutation");

        assert!(matches!(
            transaction.commit(),
            Err(crate::session::SessionTransactionError::GenerationConflict)
        ));
        assert!(session.contains_token(&winner));
        assert!(!session.contains_token(&staged_token));
        assert!(session.lookup_prefix_cache("Dr. Schmidt").is_none());
    }
}
