use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gaze::{
    Action, ClassRule, CleanDocument, ConflictTier, DefaultRule, Detection, Detector, DocumentKind,
    EmittedTokenSpan, FallbackReason, GazeLocalProtectionTraceItem, LeakKind, LeakReport,
    LeakReportTelemetry, LeakSuspect, PiiClass, Pipeline, RawDocument, RedactionEntry,
    RedactionLogError, RedactionLogger, SafetyNet, SafetyNetContext, SafetyNetError, Scope,
    Session, Value,
};

#[derive(Clone)]
struct FixedDetector {
    span: Range<usize>,
    class: PiiClass,
}

impl Detector for FixedDetector {
    fn detect(&self, _input: &str) -> Vec<Detection> {
        vec![Detection::new(
            self.span.clone(),
            self.class.clone(),
            "fixed",
        )]
    }
}

#[derive(Clone)]
struct MockNet {
    locales: Vec<gaze::LocaleTag>,
    span: Option<Range<usize>>,
    class: PiiClass,
    raw_label: &'static str,
    field_path: Option<&'static str>,
    error_on_field_path: Option<&'static str>,
    error_on_text: bool,
    seen: Arc<Mutex<Vec<SeenCheck>>>,
}

#[derive(Clone)]
struct InvalidSpanNet {
    locales: Vec<gaze::LocaleTag>,
    span: Range<usize>,
    class: PiiClass,
}

#[derive(Debug, Clone)]
struct SeenCheck {
    clean_text: String,
    field_path: Option<String>,
    document_kind: DocumentKind,
    manifest: Vec<EmittedTokenSpan>,
}

#[derive(Clone)]
struct MemoryLogger {
    entries: Arc<Mutex<Vec<RedactionEntry>>>,
}

impl MemoryLogger {
    fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn entries(&self) -> Vec<RedactionEntry> {
        self.entries.lock().expect("entries").clone()
    }
}

impl RedactionLogger for MemoryLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.entries.lock().expect("entries").push(entry.clone());
        Ok(())
    }
}

impl MockNet {
    fn new(span: Option<Range<usize>>, class: PiiClass) -> Self {
        Self {
            locales: vec![gaze::LocaleTag::Global],
            span,
            class,
            raw_label: "private_email",
            field_path: None,
            error_on_field_path: None,
            error_on_text: false,
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_locales(mut self, locales: Vec<gaze::LocaleTag>) -> Self {
        self.locales = locales;
        self
    }

    fn with_field_path(mut self, field_path: &'static str) -> Self {
        self.field_path = Some(field_path);
        self
    }

    fn error_on_field_path(mut self, field_path: &'static str) -> Self {
        self.error_on_field_path = Some(field_path);
        self
    }

    fn error_on_text(mut self) -> Self {
        self.error_on_text = true;
        self
    }
}

impl SafetyNet for MockNet {
    fn id(&self) -> &str {
        "mock"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &self.locales
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.seen.lock().unwrap().push(SeenCheck {
            clean_text: clean_text.to_string(),
            field_path: context.field_path.map(str::to_string),
            document_kind: context.document_kind,
            manifest: context.manifest.spans.clone(),
        });

        if (self.error_on_text && context.field_path.is_none())
            || self
                .error_on_field_path
                .is_some_and(|path| Some(path) == context.field_path)
        {
            return Err(SafetyNetError::Runtime {
                message: "field-level failure".to_string(),
            });
        }

        if self.field_path.is_some() && self.field_path != context.field_path {
            return Ok(Vec::new());
        }

        let Some(span) = self.span.clone() else {
            return Ok(Vec::new());
        };
        if span.start > span.end || span.end > clean_text.len() {
            return Ok(Vec::new());
        }
        let Some(kind) = context.manifest.diff_against(&span, &self.class) else {
            return Ok(Vec::new());
        };

        Ok(vec![LeakSuspect::new(
            span,
            self.class.clone(),
            self.id(),
            Some(0.99),
            kind,
            self.raw_label,
            context.field_path.map(str::to_string),
        )])
    }
}

impl SafetyNet for InvalidSpanNet {
    fn id(&self) -> &str {
        "invalid-span"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &self.locales
    }

    fn check(
        &self,
        _clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        Ok(vec![LeakSuspect::new(
            self.span.clone(),
            self.class.clone(),
            self.id(),
            Some(0.99),
            LeakKind::Uncovered,
            "private_email",
            context.field_path.map(str::to_string),
        )])
    }
}

fn session() -> Session {
    Session::new(Scope::Ephemeral).expect("session")
}

fn text(clean: CleanDocument) -> String {
    match clean {
        CleanDocument::Text(text) => text,
        CleanDocument::Structured(_) => panic!("expected text"),
        _ => panic!("expected text"),
    }
}

fn pipeline_with_net(net: Option<MockNet>) -> Pipeline {
    let mut builder = Pipeline::builder().rule(DefaultRule::new(Action::Preserve));
    if let Some(net) = net {
        builder = builder.register_safety_net(net);
    }
    builder.build().expect("pipeline")
}

fn tokenizing_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(FixedDetector {
            span: 0.."alice@example.invalid".len(),
            class: PiiClass::Email,
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline")
}

fn tokenizing_pipeline_with_net(net: MockNet) -> Pipeline {
    Pipeline::builder()
        .detector(FixedDetector {
            span: 0.."alice@example.invalid".len(),
            class: PiiClass::Email,
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(net)
        .build()
        .expect("pipeline")
}

/// The observer-only policy. `clean_with_safety_net*` defaults to `Resolve` + `Redact`
/// (`SafetyNetPolicy::default()`), so a test that wants "report but never mutate" must say so.
fn observer_policy() -> gaze::SafetyNetPolicy {
    gaze::SafetyNetPolicy::new(gaze::SafetyNetMode::Strict, gaze::SafetyNetFallback::Redact)
}

fn clean_with_policy(
    pipeline: &Pipeline,
    session: &Session,
    raw: RawDocument,
    locales: &[gaze::LocaleTag],
    policy: gaze::SafetyNetPolicy,
) -> gaze::Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)> {
    pipeline.clean_with_safety_net_policy_detect_context(
        session,
        raw,
        locales,
        &gaze::DictionaryBundle::default(),
        policy,
    )
}

fn traced_clean(
    pipeline: &Pipeline,
    session: &Session,
    raw: &str,
    policy: gaze::SafetyNetPolicy,
) -> (
    String,
    Vec<EmittedTokenSpan>,
    LeakReport,
    Vec<GazeLocalProtectionTraceItem>,
) {
    let (clean, manifest, report, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            session,
            raw,
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            policy,
        )
        .expect("traced safety-net clean");
    (text(clean), manifest, report, trace)
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn assert_trace_manifest_contract(
    raw: &str,
    clean: &str,
    manifest: &[EmittedTokenSpan],
    trace: &[GazeLocalProtectionTraceItem],
    session: &Session,
) {
    for item in trace {
        let raw_span = item.raw_start()..item.raw_end();
        assert!(raw_span.start < raw_span.end);
        assert!(raw_span.end <= raw.len());
        assert!(raw.is_char_boundary(raw_span.start));
        assert!(raw.is_char_boundary(raw_span.end));

        match item.action() {
            "tokenize" => {
                let matching = manifest
                    .iter()
                    .filter(|span| span.raw_span == raw_span && &span.class == item.class())
                    .count();
                assert_eq!(
                    matching, 1,
                    "reversible trace item must match one manifest entry"
                );
            }
            "redact" => assert!(
                !manifest
                    .iter()
                    .any(|span| ranges_overlap(&span.raw_span, &raw_span)),
                "redact trace item must not retain an overlapping manifest entry"
            ),
            action => panic!("unexpected protection action: {action}"),
        }
    }

    assert_eq!(
        trace
            .iter()
            .filter(|item| item.action() == "tokenize")
            .count(),
        manifest.len(),
        "every final manifest entry must have one reversible trace item"
    );
    for span in manifest {
        assert!(span.raw_span.start < span.raw_span.end);
        assert!(span.raw_span.end <= raw.len());
        assert!(raw.is_char_boundary(span.raw_span.start));
        assert!(raw.is_char_boundary(span.raw_span.end));
        assert!(span.clean_span.start < span.clean_span.end);
        assert!(span.clean_span.end <= clean.len());
        assert!(clean.is_char_boundary(span.clean_span.start));
        assert!(clean.is_char_boundary(span.clean_span.end));
        let token = &clean[span.clean_span.clone()];
        assert_eq!(
            session.restore(token).expect("manifest token restores"),
            raw[span.raw_span.clone()]
        );
    }
}

fn assert_successful_redaction_is_not_reversible(raw: &str, clean: &str, session: &Session) {
    let observed_restore = session
        .restore_strict_text(clean)
        .expect("successful redaction output remains valid clean text");
    assert_ne!(
        observed_restore, raw,
        "an Ok safety-net result must not override an inexact observed restore"
    );
}

#[test]
fn safety_net_trace_promotes_utf8_prefixed_uncovered_suspect_reversibly() {
    let raw = "Grüße von Dr. Schmidt";
    let suspect = "Dr. Schmidt";
    let start = raw.find(suspect).expect("synthetic suspect");
    let net = MockNet::new(Some(start..start + suspect.len()), PiiClass::Name);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();

    let (clean, manifest, report, trace) = traced_clean(
        &pipeline,
        &session,
        raw,
        gaze::SafetyNetPolicy::new(
            gaze::SafetyNetMode::Resolve,
            gaze::SafetyNetFallback::Redact,
        ),
    );

    assert!(!clean.contains(suspect));
    assert_eq!(report.stats.uncovered_count, 1);
    assert_eq!(manifest.len(), 1);
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].raw_start(), start);
    assert_eq!(trace[0].raw_end(), raw.len());
    assert_eq!(trace[0].class(), &PiiClass::Name);
    assert_eq!(trace[0].stage(), "safety_net");
    assert_eq!(trace[0].decision(), "resolve");
    assert_eq!(trace[0].action(), "tokenize");
    assert_eq!(trace[0].source_ids(), &["mock".to_string()]);
    assert_trace_manifest_contract(raw, &clean, &manifest, &trace, &session);
    assert_eq!(
        session.restore_strict_text(&clean).expect("exact restore"),
        raw
    );
}

#[test]
fn safety_net_trace_partial_bleed_tokenizes_only_uncovered_subspan() {
    let raw = "alice@example.invalid tail";
    let baseline_session = session();
    let baseline = text(
        tokenizing_pipeline()
            .redact(&baseline_session, RawDocument::Text(raw.to_string()))
            .expect("baseline"),
    );
    let token_end = baseline.find(" tail").expect("token suffix");
    let net = MockNet::new(Some(0..baseline.len()), PiiClass::Email);
    let pipeline = tokenizing_pipeline_with_net(net);
    let session = session();

    let (clean, manifest, report, trace) = traced_clean(
        &pipeline,
        &session,
        raw,
        gaze::SafetyNetPolicy::new(
            gaze::SafetyNetMode::Resolve,
            gaze::SafetyNetFallback::Redact,
        ),
    );

    assert!(matches!(
        report.suspects[0].kind,
        LeakKind::PartialBleed { ref uncovered }
            if uncovered == &(token_end..baseline.len())
    ));
    assert_eq!(manifest.len(), 2);
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0].raw_start(), 0);
    assert_eq!(trace[0].raw_end(), "alice@example.invalid".len());
    assert_eq!(trace[0].class(), &PiiClass::Email);
    assert_eq!(trace[0].stage(), "primary_pipeline");
    assert_eq!(trace[0].decision(), "policy");
    assert_eq!(trace[0].action(), "tokenize");
    assert_eq!(trace[0].source_ids(), &["fixed".to_string()]);
    let resolved = trace
        .iter()
        .find(|item| item.decision() == "resolve")
        .expect("partial-bleed resolution trace");
    assert_eq!(
        resolved.raw_start()..resolved.raw_end(),
        "alice@example.invalid".len()..raw.len()
    );
    assert_eq!(resolved.class(), &PiiClass::Email);
    assert_eq!(resolved.stage(), "safety_net");
    assert_eq!(resolved.action(), "tokenize");
    assert_eq!(resolved.source_ids(), &["mock".to_string()]);
    assert_trace_manifest_contract(raw, &clean, &manifest, &trace, &session);
    assert_eq!(
        session.restore_strict_text(&clean).expect("exact restore"),
        raw
    );
}

#[test]
fn safety_net_trace_class_mismatch_inside_live_token_is_reversible_noop() {
    let raw = "alice@example.invalid ok";
    let baseline_session = session();
    let baseline = text(
        tokenizing_pipeline()
            .redact(&baseline_session, RawDocument::Text(raw.to_string()))
            .expect("baseline"),
    );
    let token_end = baseline.find(" ok").expect("token suffix");
    let net = MockNet::new(Some(0..token_end), PiiClass::Name);
    let pipeline = tokenizing_pipeline_with_net(net);
    let session = session();

    let (clean, manifest, report, trace) = traced_clean(
        &pipeline,
        &session,
        raw,
        gaze::SafetyNetPolicy::new(
            gaze::SafetyNetMode::Resolve,
            gaze::SafetyNetFallback::Redact,
        ),
    );

    assert_eq!(report.stats.class_mismatch_count, 1);
    assert!(clean.ends_with(" ok"));
    assert_eq!(manifest.len(), 1);
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].raw_start(), 0);
    assert_eq!(trace[0].raw_end(), "alice@example.invalid".len());
    assert_eq!(trace[0].class(), &PiiClass::Email);
    assert_eq!(trace[0].stage(), "primary_pipeline");
    assert_eq!(trace[0].decision(), "policy");
    assert_eq!(trace[0].action(), "tokenize");
    assert_eq!(trace[0].source_ids(), &["fixed".to_string()]);
    assert_trace_manifest_contract(raw, &clean, &manifest, &trace, &session);
    assert_eq!(
        session
            .restore_strict_text(&clean)
            .expect("protected mismatch restores"),
        raw
    );
}

#[test]
fn safety_net_trace_residual_utf8_suspect_fails_closed() {
    let raw = "Grüße von Dr. Schmidt";
    let multibyte = raw.find('ü').expect("multibyte character");
    let net = MockNet::new(Some(multibyte + 1..multibyte + "ü".len()), PiiClass::Name);
    let pipeline = pipeline_with_net(Some(net));
    let traced_session = session();

    let error = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &traced_session,
            raw,
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::new(
                gaze::SafetyNetMode::Resolve,
                gaze::SafetyNetFallback::Strict,
            ),
        )
        .expect_err("invalid UTF-8 residual must fail closed");

    assert!(matches!(
        error,
        gaze::Error::SafetyNetFallback(FallbackReason::ResidualSuspect)
    ));

    let untraced_session = session();
    let untraced_error = pipeline
        .clean_with_safety_net_policy_detect_context(
            &untraced_session,
            RawDocument::Text(raw.to_string()),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::new(
                gaze::SafetyNetMode::Resolve,
                gaze::SafetyNetFallback::Strict,
            ),
        )
        .expect_err("invalid UTF-8 residual must also fail closed without tracing");
    assert!(matches!(
        untraced_error,
        gaze::Error::SafetyNetFallback(FallbackReason::ResidualSuspect)
    ));
}

#[test]
fn safety_net_trace_redact_action_protects_utf8_span_without_claiming_restore() {
    let raw = "Grüße von Dr. Schmidt";
    let suspect = "Grüße";
    let net = MockNet::new(Some(0..suspect.len()), PiiClass::Name);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();

    let (clean, manifest, report, trace) = traced_clean(
        &pipeline,
        &session,
        raw,
        gaze::SafetyNetPolicy::new(gaze::SafetyNetMode::Redact, gaze::SafetyNetFallback::Strict),
    );

    assert_eq!(report.stats.uncovered_count, 1);
    assert_eq!(clean, " von Dr. Schmidt");
    assert!(manifest.is_empty());
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].raw_start(), 0);
    assert_eq!(trace[0].raw_end(), suspect.len());
    assert_eq!(trace[0].class(), &PiiClass::Name);
    assert_eq!(trace[0].stage(), "safety_net");
    assert_eq!(trace[0].decision(), "redact");
    assert_eq!(trace[0].action(), "redact");
    assert_eq!(trace[0].source_ids(), &["mock".to_string()]);
    assert_trace_manifest_contract(raw, &clean, &manifest, &trace, &session);
    assert_successful_redaction_is_not_reversible(raw, &clean, &session);
}

#[test]
fn safety_net_resolve_mode_promotes_suspect_to_manifest_token() {
    let suspect = "alice@example.invalid";
    let raw = format!("Reach {suspect}");
    let start = raw.find(suspect).expect("suspect");
    let net = MockNet::new(Some(start..start + suspect.len()), PiiClass::Email);
    let logger = MemoryLogger::new();
    let pipeline = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(net)
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");
    let session = session();

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(raw),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::default(),
        )
        .expect("resolve");
    let clean_text = text(clean);

    assert!(!clean_text.contains(suspect));
    assert_eq!(manifest.len(), 1);
    assert_eq!(report.stats.uncovered_count, 1);
    assert_eq!(
        session.restore(&clean_text[start..]),
        Some(suspect.to_string())
    );
    assert!(logger
        .entries()
        .iter()
        .any(|entry| entry.decided_by == ConflictTier::Resolve && !entry.conflict_loser));
}

#[test]
fn safety_net_redact_mode_strips_suspect_without_manifest_entry() {
    let suspect = "alice@example.invalid";
    let raw = format!("Reach {suspect}");
    let start = raw.find(suspect).expect("suspect");
    let net = MockNet::new(Some(start..start + suspect.len()), PiiClass::Email);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(raw),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::new(
                gaze::SafetyNetMode::Redact,
                gaze::SafetyNetFallback::Strict,
            ),
        )
        .expect("redact");

    assert_eq!(text(clean), "Reach ");
    assert!(manifest.is_empty());
    assert_eq!(report.stats.uncovered_count, 1);
}

#[test]
fn safety_net_redact_mode_rounds_misaligned_multibyte_suspect_outward() {
    let raw = "Grüße von Dr. Schmidt".to_string();
    let multibyte = raw.find('ü').expect("multibyte char");
    let net = MockNet::new(Some(multibyte + 1..multibyte + "ü".len()), PiiClass::Name);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(raw),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::new(
                gaze::SafetyNetMode::Redact,
                gaze::SafetyNetFallback::Strict,
            ),
        )
        .expect("misaligned redact rounds outward");
    let clean_text = text(clean);

    assert_eq!(clean_text, "Grße von Dr. Schmidt");
    assert!(!clean_text.contains("Grüße"));
    assert!(manifest.is_empty());
    assert_eq!(report.stats.uncovered_count, 1);
}

#[test]
fn safety_net_redact_mode_expands_overlap_to_entire_emitted_token() {
    let session = session();
    let raw = RawDocument::Text("alice@example.invalid ok".to_string());
    let baseline = text(
        tokenizing_pipeline()
            .redact(&session, raw.clone())
            .expect("baseline"),
    );
    let token_len = baseline.find(" ok").expect("token suffix");
    let net = MockNet::new(Some(2..token_len - 2), PiiClass::Name);

    let (clean, manifest, report) = tokenizing_pipeline_with_net(net)
        .clean_with_safety_net_policy_detect_context(
            &session,
            raw,
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::new(
                gaze::SafetyNetMode::Redact,
                gaze::SafetyNetFallback::Strict,
            ),
        )
        .expect("overlap redact expands to emitted token");
    let clean_text = text(clean);

    assert_eq!(clean_text, " ok");
    assert!(!clean_text.contains('<'));
    assert!(!clean_text.contains("Email"));
    assert!(manifest.is_empty());
    assert_eq!(
        session.restore_strict_text(&clean_text).expect("restore"),
        " ok"
    );
    assert_eq!(report.stats.class_mismatch_count, 1);
}

#[test]
fn safety_net_redact_mode_out_of_range_suspect_fails_closed_without_raw_text() {
    let raw = "Reach alice@example.invalid".to_string();
    let net = InvalidSpanNet {
        locales: vec![gaze::LocaleTag::Global],
        span: 6..raw.len() + 1,
        class: PiiClass::Email,
    };
    let pipeline = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(net)
        .build()
        .expect("pipeline");
    let session = session();

    let err = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(raw.clone()),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::new(
                gaze::SafetyNetMode::Redact,
                gaze::SafetyNetFallback::Strict,
            ),
        )
        .expect_err("out-of-range safety net span fails closed");

    assert!(matches!(
        err,
        gaze::Error::SafetyNetSpanInvalid {
            start: 6,
            end: _,
            text_len: _
        }
    ));
    assert!(!err.to_string().contains("alice@example.invalid"));
    assert!(session.tokens().is_empty());
}

#[test]
fn safety_net_redact_mode_keeps_aligned_span_redaction_behavior() {
    let raw = "Hello Dr. Schmidt".to_string();
    let suspect = "Dr. Schmidt";
    let start = raw.find(suspect).expect("suspect");
    let net = MockNet::new(Some(start..start + suspect.len()), PiiClass::Name);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(raw),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::new(
                gaze::SafetyNetMode::Redact,
                gaze::SafetyNetFallback::Strict,
            ),
        )
        .expect("aligned redact");

    assert_eq!(text(clean), "Hello ");
    assert!(manifest.is_empty());
    assert_eq!(report.stats.uncovered_count, 1);
}

/// A class-mismatch suspect that lies wholly inside a live token is the net re-flagging text the
/// pipeline already protected. Resolving or redacting it would destroy a live token (and with it
/// the restore path) to remove nothing, so it is audited as a conflict-loser no-op.
///
/// Before the fail-closed integrity work this fell back to redaction: the clean text became
/// `" ok"`, the manifest was emptied, and the document was permanently irreversible.
#[test]
fn safety_net_resolve_class_mismatch_inside_live_token_is_audited_noop() {
    let session = session();
    let raw = RawDocument::Text("alice@example.invalid ok".to_string());
    let baseline = text(
        tokenizing_pipeline()
            .redact(&session, raw.clone())
            .expect("baseline"),
    );
    let token_len = baseline.find(" ok").expect("token suffix");
    let logger = MemoryLogger::new();
    let net = MockNet::new(Some(0..token_len), PiiClass::Name);
    let pipeline = Pipeline::builder()
        .detector(FixedDetector {
            span: 0.."alice@example.invalid".len(),
            class: PiiClass::Email,
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(net)
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");

    let (clean, _, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            raw,
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::default(),
        )
        .expect("protected mismatch must not fail closed");

    assert_eq!(report.stats.class_mismatch_count, 1);
    let clean = text(clean);
    assert_eq!(
        clean, baseline,
        "protected mismatch must not alter the document"
    );
    assert_eq!(
        session
            .restore_strict_text(&clean)
            .expect("protected mismatch stays restorable"),
        "alice@example.invalid ok"
    );
    assert!(logger.entries().iter().any(|entry| {
        entry.decided_by == ConflictTier::Resolve
            && entry.conflict_loser
            && entry.fallback_triggered.is_none()
    }));
}

#[test]
fn observer_text_path_returns_manifest_and_report_without_mutating() {
    let net = MockNet::new(Some(0..5), PiiClass::Email);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();

    let (clean, manifest, report) = clean_with_policy(
        &pipeline,
        &session,
        RawDocument::Text("Reach alice@example.invalid".to_string()),
        &[gaze::LocaleTag::Global],
        observer_policy(),
    )
    .expect("safety net clean");

    assert_eq!(text(clean), "Reach alice@example.invalid");
    assert!(manifest.is_empty());
    assert_eq!(report.stats.suspect_count, 1);
    assert_eq!(report.suspects[0].kind, LeakKind::Uncovered);
}

/// The policy-less convenience entry points are `SafetyNetPolicy::default()`, not a private
/// observer policy of their own: one documented default across the library surface.
#[test]
fn clean_with_safety_net_convenience_api_uses_the_documented_default() {
    assert_eq!(
        gaze::SafetyNetPolicy::default(),
        gaze::SafetyNetPolicy::new(
            gaze::SafetyNetMode::Resolve,
            gaze::SafetyNetFallback::Redact
        )
    );

    let net = MockNet::new(Some(0..5), PiiClass::Email);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();
    let raw = RawDocument::Text("Reach alice@example.invalid".to_string());

    let (convenience, convenience_manifest, _) = pipeline
        .clean_with_safety_net(&session, raw.clone(), &[gaze::LocaleTag::Global])
        .expect("convenience clean");
    let (explicit, explicit_manifest, _) = clean_with_policy(
        &pipeline,
        &session,
        raw,
        &[gaze::LocaleTag::Global],
        gaze::SafetyNetPolicy::default(),
    )
    .expect("explicit default clean");

    assert_eq!(text(convenience), text(explicit));
    assert_eq!(convenience_manifest, explicit_manifest);
    // Enforcing, not observing: the suspect was promoted into the manifest reversibly.
    assert_eq!(convenience_manifest.len(), 1);
}

#[test]
fn byte_equal_invariance_for_leak_kinds_and_locale_skip() {
    let session = session();
    let raw = RawDocument::Text("alice@example.invalid ok".to_string());
    let baseline = text(
        tokenizing_pipeline()
            .redact(&session, raw.clone())
            .expect("baseline"),
    );
    let token_len = baseline.find(" ok").expect("token suffix");
    let clean_len = baseline.len();

    let cases = [
        (
            MockNet::new(Some(0..clean_len), PiiClass::Email),
            Some(LeakKind::PartialBleed {
                uncovered: token_len..clean_len,
            }),
        ),
        (
            MockNet::new(Some(0..token_len), PiiClass::Name),
            Some(LeakKind::ClassMismatch {
                pipeline_class: PiiClass::Email,
                safety_net_class: PiiClass::Name,
            }),
        ),
        (
            MockNet::new(Some(token_len + 1..clean_len), PiiClass::Email),
            Some(LeakKind::Uncovered),
        ),
    ];

    for (net, expected_kind) in cases {
        let pipeline = tokenizing_pipeline_with_net(net);
        let (clean, _, report) = clean_with_policy(
            &pipeline,
            &session,
            raw.clone(),
            &[gaze::LocaleTag::Global],
            observer_policy(),
        )
        .expect("safety net clean");
        assert_eq!(text(clean), baseline);
        assert_eq!(
            report.suspects.first().map(|suspect| &suspect.kind),
            expected_kind.as_ref()
        );
    }

    let skipped =
        MockNet::new(Some(0..9), PiiClass::Email).with_locales(vec![gaze::LocaleTag::EnUs]);
    let (clean, _, report) = clean_with_policy(
        &tokenizing_pipeline_with_net(skipped),
        &session,
        raw,
        &[gaze::LocaleTag::DeDe],
        observer_policy(),
    )
    .expect("locale-skipped safety net clean");
    assert_eq!(text(clean), baseline);
    assert_eq!(report.stats.locale_skipped_count, 1);
    assert!(report.suspects.is_empty());
}

#[test]
fn safety_net_error_fails_closed_after_observing_byte_equal_clean_text() {
    let session = session();
    let raw = RawDocument::Text("alice@example.invalid ok".to_string());
    let baseline = text(
        tokenizing_pipeline()
            .redact(&session, raw.clone())
            .expect("baseline"),
    );
    let net = MockNet::new(Some(0..9), PiiClass::Email).error_on_text();
    let seen = Arc::clone(&net.seen);

    let err = tokenizing_pipeline_with_net(net)
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::Global])
        .expect_err("safety net errors fail closed");

    assert!(matches!(
        err,
        gaze::Error::SafetyNet(SafetyNetError::Runtime { .. })
    ));
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].clean_text, baseline);
    assert_eq!(
        seen[0].manifest[0].clean_span,
        0..baseline.find(" ok").unwrap()
    );
}

#[test]
fn structured_safety_net_traverses_nested_fields_and_preserves_shape() {
    let net =
        MockNet::new(Some(0..21), PiiClass::Email).with_field_path("$.user.contacts[0].email");
    let pipeline = pipeline_with_net(Some(net.clone()));
    let session = session();

    let mut contact = BTreeMap::new();
    contact.insert(
        "email".to_string(),
        Value::String("alice@example.invalid".to_string()),
    );
    let mut user = BTreeMap::new();
    user.insert(
        "contacts".to_string(),
        Value::Array(vec![Value::Object(contact)]),
    );
    user.insert("empty".to_string(), Value::String(String::new()));
    user.insert("active".to_string(), Value::Bool(true));
    user.insert("count".to_string(), Value::I64(7));

    let raw = RawDocument::Structured(BTreeMap::from([("user".to_string(), Value::Object(user))]));

    let (clean, manifest, report) = clean_with_policy(
        &pipeline,
        &session,
        raw,
        &[gaze::LocaleTag::Global],
        observer_policy(),
    )
    .expect("structured safety net clean");

    assert!(manifest.is_empty(), "structured manifests stay field-local");
    assert_eq!(report.stats.suspect_count, 1);
    assert_eq!(
        report.suspects[0].field_path.as_deref(),
        Some("$.user.contacts[0].email")
    );
    assert_eq!(report.suspects[0].kind, LeakKind::Uncovered);

    let CleanDocument::Structured(fields) = clean else {
        panic!("expected structured output");
    };
    let Value::Object(user) = &fields["user"] else {
        panic!("expected object");
    };
    assert_eq!(user["active"], Value::Bool(true));
    assert_eq!(user["count"], Value::I64(7));

    let seen = net.seen.lock().unwrap();
    assert!(seen.iter().any(|entry| {
        entry.field_path.as_deref() == Some("$.user.contacts[0].email")
            && entry.document_kind == DocumentKind::Structured
    }));
    assert!(
        !seen
            .iter()
            .any(|entry| entry.field_path.as_deref() == Some("$.user.empty")),
        "empty string fields are no-op skips"
    );
}

#[test]
fn structured_locale_skip_uses_session_level_locale_chain() {
    let net = MockNet::new(Some(0..21), PiiClass::Email).with_locales(vec![gaze::LocaleTag::EnUs]);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();
    let raw = RawDocument::Structured(BTreeMap::from([(
        "email".to_string(),
        Value::String("alice@example.invalid".to_string()),
    )]));

    let (_, _, report) = clean_with_policy(
        &pipeline,
        &session,
        raw,
        &[gaze::LocaleTag::DeDe],
        observer_policy(),
    )
    .expect("structured locale skip");

    // For RawDocument::Structured, locale gating uses session-level locale
    // chain across all fields; fields do not carry per-field locale metadata.
    assert_eq!(report.stats.locale_skipped_count, 1);
    assert!(matches!(
        &report.telemetry[0],
        LeakReportTelemetry::LocaleSkipped {
            field_path: Some(path),
            document_kind: DocumentKind::Structured,
            ..
        } if path == "$.email"
    ));
    assert!(report.suspects.is_empty());
}

#[test]
fn structured_field_error_fails_closed_at_doc_level() {
    let net = MockNet::new(None, PiiClass::Email).error_on_field_path("$.profile.email");
    let pipeline = pipeline_with_net(Some(net));
    let session = session();
    let raw = RawDocument::Structured(BTreeMap::from([(
        "profile".to_string(),
        Value::Object(BTreeMap::from([(
            "email".to_string(),
            Value::String("alice@example.invalid".to_string()),
        )])),
    )]));

    let err = clean_with_policy(
        &pipeline,
        &session,
        raw,
        &[gaze::LocaleTag::Global],
        observer_policy(),
    )
    .expect_err("field-level safety net errors fail closed");

    assert!(matches!(
        err,
        gaze::Error::SafetyNet(SafetyNetError::Runtime { .. })
    ));
}

#[test]
fn scan_safety_nets_does_not_mutate_session() {
    let net = MockNet::new(Some(0.."alice@example.invalid".len()), PiiClass::Email);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();
    let before = session.tokens().len();

    let result = pipeline
        .scan_safety_nets(
            &session,
            "alice@example.invalid",
            &[gaze::LocaleTag::Global],
        )
        .expect("observer-only scan");

    assert_eq!(result.nets_run, 1);
    assert_eq!(result.report.stats.suspect_count, 1);
    assert_eq!(session.tokens().len(), before);
}

#[test]
fn scan_safety_nets_structured_does_not_mutate_session() {
    let net = MockNet::new(Some(0.."alice@example.invalid".len()), PiiClass::Email)
        .with_field_path("profile.email");
    let pipeline = pipeline_with_net(Some(net));
    let session = session();
    let document = BTreeMap::from([(
        "profile".to_string(),
        Value::Object(BTreeMap::from([(
            "email".to_string(),
            Value::String("alice@example.invalid".to_string()),
        )])),
    )]);
    let before = session.tokens().len();

    let result = pipeline
        .scan_safety_nets_structured(&session, &document, &[gaze::LocaleTag::Global])
        .expect("observer-only structured scan");

    assert_eq!(result.nets_run, 1);
    assert_eq!(result.report.stats.suspect_count, 1);
    assert_eq!(
        result.report.suspects[0].field_path.as_deref(),
        Some("profile.email")
    );
    assert_eq!(session.tokens().len(), before);
}

#[test]
fn scan_safety_nets_structured_covers_scalar_leaves() {
    let net =
        MockNet::new(Some(0..2), PiiClass::custom("customer_id")).with_field_path("customer_id");
    let pipeline = pipeline_with_net(Some(net));
    let session = session();
    let document = BTreeMap::from([("customer_id".to_string(), Value::I64(42))]);

    let result = pipeline
        .scan_safety_nets_structured(&session, &document, &[gaze::LocaleTag::Global])
        .expect("structured scalar scan");

    assert_eq!(result.nets_run, 1);
    assert_eq!(result.report.stats.suspect_count, 1);
    assert_eq!(
        result.report.suspects[0].field_path.as_deref(),
        Some("customer_id")
    );
}

/// Reports a residual only from the second `check` onward.
///
/// This is the shape the `Resolve` fallback exists for: the primary pass converges (nothing to
/// resolve), the post-resolution re-run flags something, and the fallback must act on *that*
/// report. A net that flags on the first pass cannot distinguish "acted on the primary report"
/// from "acted on the residual report", so it cannot pin the contract.
#[derive(Clone)]
struct SecondPassNet {
    marker: &'static str,
    calls: Arc<AtomicUsize>,
}

impl SecondPassNet {
    fn new(marker: &'static str) -> Self {
        Self {
            marker,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl SafetyNet for SecondPassNet {
    fn id(&self) -> &str {
        "second-pass"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(Vec::new());
        }
        let Some(start) = clean_text.find(self.marker) else {
            return Ok(Vec::new());
        };
        let span = start..start + self.marker.len();
        let Some(kind) = context.manifest.diff_against(&span, &PiiClass::Name) else {
            return Ok(Vec::new());
        };
        Ok(vec![LeakSuspect::new(
            span,
            PiiClass::Name,
            self.id(),
            Some(0.99),
            kind,
            PiiClass::Name.to_canonical_str(),
            context.field_path.map(str::to_string),
        )])
    }
}

const RESIDUAL_MARKER: &str = "residue";
const RESIDUAL_RAW: &str = "alice@example.invalid residue";

fn residual_pipeline(logger: MemoryLogger) -> Pipeline {
    Pipeline::builder()
        .detector(FixedDetector {
            span: 0.."alice@example.invalid".len(),
            class: PiiClass::Email,
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(SecondPassNet::new(RESIDUAL_MARKER))
        .redaction_logger(logger)
        .build()
        .expect("pipeline")
}

fn fallback_rows(logger: &MemoryLogger) -> Vec<RedactionEntry> {
    logger
        .entries()
        .into_iter()
        .filter(|entry| entry.decided_by == ConflictTier::Fallback)
        .collect()
}

/// Exhaustive behavioural table over every representable `(mode, fallback)` pair.
///
/// The two public fields spell 4 x 3 = 12 pairs; the pipeline has 6 behaviours. This asserts the
/// lowering is total (every pair maps to a `SafetyNetDecision`) *and* that the mapping is honest
/// — each pair is run through the public entry point twice and the observable outcome is pinned,
/// so a pair whose `fallback` is documented as "not consulted" provably ignores it rather than
/// silently doing something else.
#[test]
fn safety_net_policy_lowering_covers_all_twelve_representable_pairs() {
    let modes = [
        gaze::SafetyNetMode::Strict,
        gaze::SafetyNetMode::Tolerant,
        gaze::SafetyNetMode::Redact,
        gaze::SafetyNetMode::Resolve,
    ];
    let fallbacks = [
        gaze::SafetyNetFallback::Strict,
        gaze::SafetyNetFallback::Tolerant,
        gaze::SafetyNetFallback::Redact,
    ];

    // Tokens are session-scoped, so every baseline and every case share one session per fixture.
    let session_a = session();
    let session_b = session();

    // Fixture A baseline: primary tokenizes the email, "ok" is left uncovered.
    let first_pass_raw = "alice@example.invalid ok";
    let baseline_a = text(
        tokenizing_pipeline()
            .redact(&session_a, RawDocument::Text(first_pass_raw.to_string()))
            .expect("baseline"),
    );
    let token_len = baseline_a.find(" ok").expect("token suffix");
    let uncovered = token_len + 1..baseline_a.len();

    // Fixture B baseline: nothing is flagged on the first pass.
    let baseline_b = text(
        tokenizing_pipeline()
            .redact(&session_b, RawDocument::Text(RESIDUAL_RAW.to_string()))
            .expect("baseline"),
    );
    assert!(baseline_b.contains(RESIDUAL_MARKER));

    let mut covered = Vec::new();
    for mode in modes {
        for fallback in fallbacks {
            let policy = gaze::SafetyNetPolicy::new(mode, fallback);
            let decision = policy.decision();
            covered.push((mode, fallback));

            // 1. The lowering itself.
            match mode {
                gaze::SafetyNetMode::Strict => {
                    assert_eq!(decision, gaze::SafetyNetDecision::Observe { strict: true })
                }
                gaze::SafetyNetMode::Tolerant => {
                    assert_eq!(decision, gaze::SafetyNetDecision::Observe { strict: false })
                }
                gaze::SafetyNetMode::Redact => {
                    assert_eq!(decision, gaze::SafetyNetDecision::Redact)
                }
                gaze::SafetyNetMode::Resolve => assert_eq!(
                    decision,
                    gaze::SafetyNetDecision::Resolve {
                        on_residual: fallback
                    }
                ),
                _ => panic!("unhandled mode {mode:?}"),
            }

            // 2. Fixture A — the net flags an uncovered span on the first pass.
            let (clean_a, manifest_a, report_a) = clean_with_policy(
                &tokenizing_pipeline_with_net(MockNet::new(
                    Some(uncovered.clone()),
                    PiiClass::Email,
                )),
                &session_a,
                RawDocument::Text(first_pass_raw.to_string()),
                &[gaze::LocaleTag::Global],
                policy,
            )
            .unwrap_or_else(|err| panic!("{mode:?}/{fallback:?} fixture A: {err:?}"));
            let clean_a = text(clean_a);
            assert_eq!(report_a.stats.suspect_count, 1, "{mode:?}/{fallback:?}");
            match mode {
                // Observer modes never mutate, whatever the fallback says.
                gaze::SafetyNetMode::Strict | gaze::SafetyNetMode::Tolerant => {
                    assert_eq!(clean_a, baseline_a, "{mode:?}/{fallback:?}");
                    assert_eq!(manifest_a.len(), 1, "{mode:?}/{fallback:?}");
                }
                // Redact deletes the suspect bytes one-way; no fallback is consulted.
                gaze::SafetyNetMode::Redact => {
                    assert_eq!(
                        clean_a,
                        baseline_a[..uncovered.start],
                        "{mode:?}/{fallback:?}"
                    );
                    assert_eq!(manifest_a.len(), 1, "{mode:?}/{fallback:?}");
                }
                // Resolve promotes the suspect reversibly and converges, so the fallback is
                // never reached and restore still round-trips.
                gaze::SafetyNetMode::Resolve => {
                    assert_ne!(clean_a, baseline_a, "{mode:?}/{fallback:?}");
                    assert_eq!(manifest_a.len(), 2, "{mode:?}/{fallback:?}");
                    assert_eq!(
                        session_a.restore_strict_text(&clean_a).expect("restore"),
                        first_pass_raw,
                        "{mode:?}/{fallback:?}"
                    );
                }
                _ => panic!("unhandled mode {mode:?}"),
            }

            // 3. Fixture B — nothing on the first pass, a residual on the re-run. Only
            //    `Resolve` re-runs the nets, so only `Resolve` can reach the fallback column.
            let logger = MemoryLogger::new();
            let result_b = clean_with_policy(
                &residual_pipeline(logger.clone()),
                &session_b,
                RawDocument::Text(RESIDUAL_RAW.to_string()),
                &[gaze::LocaleTag::Global],
                policy,
            );
            let rows = fallback_rows(&logger);
            match (mode, fallback) {
                (
                    gaze::SafetyNetMode::Strict
                    | gaze::SafetyNetMode::Tolerant
                    | gaze::SafetyNetMode::Redact,
                    _,
                ) => {
                    let (clean_b, _, report_b) =
                        result_b.unwrap_or_else(|err| panic!("{mode:?}/{fallback:?} B: {err:?}"));
                    assert_eq!(text(clean_b), baseline_b, "{mode:?}/{fallback:?}");
                    assert_eq!(report_b.stats.suspect_count, 0, "{mode:?}/{fallback:?}");
                    assert!(rows.is_empty(), "{mode:?}/{fallback:?}");
                }
                (gaze::SafetyNetMode::Resolve, gaze::SafetyNetFallback::Strict) => {
                    let err = result_b.expect_err("residual must fail closed");
                    assert!(
                        matches!(
                            err,
                            gaze::Error::SafetyNetFallback(FallbackReason::ResidualSuspect)
                        ),
                        "{mode:?}/{fallback:?}: {err:?}"
                    );
                    assert_eq!(rows.len(), 1, "{mode:?}/{fallback:?}");
                    assert_eq!(rows[0].action, Action::Preserve, "{mode:?}/{fallback:?}");
                }
                (gaze::SafetyNetMode::Resolve, gaze::SafetyNetFallback::Tolerant) => {
                    let (clean_b, _, _) =
                        result_b.unwrap_or_else(|err| panic!("{mode:?}/{fallback:?} B: {err:?}"));
                    let clean_b = text(clean_b);
                    // Tolerant ships the residual bytes by design, and the row says so.
                    assert!(clean_b.contains(RESIDUAL_MARKER), "{mode:?}/{fallback:?}");
                    assert_eq!(rows.len(), 1, "{mode:?}/{fallback:?}");
                    assert_eq!(rows[0].action, Action::Preserve, "{mode:?}/{fallback:?}");
                }
                (gaze::SafetyNetMode::Resolve, gaze::SafetyNetFallback::Redact) => {
                    let (clean_b, _, _) =
                        result_b.unwrap_or_else(|err| panic!("{mode:?}/{fallback:?} B: {err:?}"));
                    let clean_b = text(clean_b);
                    // The shipped default: the residual is gone, and the row says Redact.
                    assert!(!clean_b.contains(RESIDUAL_MARKER), "{mode:?}/{fallback:?}");
                    assert_eq!(rows.len(), 1, "{mode:?}/{fallback:?}");
                    assert_eq!(rows[0].action, Action::Redact, "{mode:?}/{fallback:?}");
                }
                _ => panic!("unhandled pair {mode:?}/{fallback:?}"),
            }
        }
    }
    assert_eq!(covered.len(), 12, "all representable pairs must be covered");
}

/// Axis-1 + axis-4: the `Resolve` fallback must act on the report that produced the reason.
///
/// When the resolve pass converges and the post-resolution re-run flags a residual, the residual
/// lives in the *re-run* report at post-resolve coordinates. Acting on the primary report there
/// redacts stale spans (or, when the primary report was empty, nothing at all) and ships the
/// residual bytes under a policy that promised to remove them.
#[test]
fn resolve_fallback_redacts_the_residual_report_not_the_stale_primary_report() {
    let logger = MemoryLogger::new();
    let session = session();
    let (clean, manifest, _) = clean_with_policy(
        &residual_pipeline(logger.clone()),
        &session,
        RawDocument::Text(RESIDUAL_RAW.to_string()),
        &[gaze::LocaleTag::Global],
        gaze::SafetyNetPolicy::default(),
    )
    .expect("default policy clean");
    let clean = text(clean);

    // The residual is gone...
    assert!(!clean.contains(RESIDUAL_MARKER));
    // ...and the email token the primary pass minted is intact, so restore still works for it.
    assert_eq!(manifest.len(), 1);
    assert_eq!(
        session.restore_strict_text(&clean).expect("restore"),
        "alice@example.invalid "
    );

    // The audit row names the residual suspect and states what was done to its bytes.
    let rows = fallback_rows(&logger);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].action, Action::Redact);
    assert_eq!(rows[0].class, PiiClass::Name);
    assert_eq!(rows[0].source, "safety_net.second-pass");
    assert!(rows[0].conflict_loser);
    assert_eq!(
        rows[0].fallback_triggered,
        Some(FallbackReason::ResidualSuspect)
    );
}

/// `GazeLocalProtectionTraceKind::SafetyNetFallbackRedact` is reachable and distinct from the
/// primary-redact kind: a fallback deletion is traced as `fallback_redact`, not `redact`. The
/// benchmark harness asserts on exactly this `(stage, decision, action)` triple.
#[test]
fn fallback_redaction_is_traced_as_fallback_redact() {
    let session = session();
    let (_, _, _, trace) = traced_clean(
        &residual_pipeline(MemoryLogger::new()),
        &session,
        RESIDUAL_RAW,
        gaze::SafetyNetPolicy::default(),
    );

    let fallback = trace
        .iter()
        .find(|item| item.decision() == "fallback_redact")
        .expect("fallback redaction must be traced");
    assert_eq!(fallback.stage(), "safety_net");
    assert_eq!(fallback.action(), "redact");
    assert_eq!(fallback.source_ids(), &["second-pass".to_string()]);
}

/// Reports nothing on the first `check`, then a *mixed* residual: one class mismatch lying wholly
/// inside a minted token (already protected — acting on it destroys restore) plus one genuine
/// uncovered residual.
///
/// The single-suspect residual fixture cannot tell "the fallback acted on the residual report"
/// apart from "the fallback acted on every suspect in it". This one can.
#[derive(Clone)]
struct MixedSecondPassNet {
    residual_marker: &'static str,
    /// Kind reported for the actionable residual. `Uncovered` drives the fallback through
    /// `ResidualSuspect`; `ClassMismatch` drives it through `OverlapConflict`. Both reasons reach
    /// the same unfiltered redaction path, so both must be covered.
    residual_kind: LeakKind,
    calls: Arc<AtomicUsize>,
}

impl MixedSecondPassNet {
    fn new(residual_marker: &'static str, residual_kind: LeakKind) -> Self {
        Self {
            residual_marker,
            residual_kind,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl SafetyNet for MixedSecondPassNet {
    fn id(&self) -> &str {
        "mixed-second-pass"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(Vec::new());
        }
        let mut suspects = Vec::new();
        let suspect = |span: Range<usize>, kind| {
            LeakSuspect::new(
                span,
                PiiClass::Name,
                "mixed-second-pass",
                Some(0.99),
                kind,
                PiiClass::Name.to_canonical_str(),
                None,
            )
        };
        // (a) A class mismatch covering the token the primary pass minted. It is already
        //     protected; the resolver audits it as a no-op and must never redact it.
        if let Some(minted) = context.manifest.spans.first() {
            let span = minted.clean_span.clone();
            if let Some(kind) = context.manifest.diff_against(&span, &PiiClass::Name) {
                suspects.push(suspect(span, kind));
            }
        }
        // (b) The genuine residual, outside every token. Its kind is dictated by the fixture
        //     rather than by the manifest, so one net can drive both fallback reasons.
        if let Some(start) = clean_text.find(self.residual_marker) {
            let span = start..start + self.residual_marker.len();
            suspects.push(suspect(span, self.residual_kind.clone()));
        }
        Ok(suspects)
    }
}

/// Axis-2: the `Resolve` fallback must redact the residual **without** touching suspects that are
/// already protected by a live token.
///
/// Handing the fallback the whole residual report is necessary (the residual is only in there) but
/// not sufficient: `post_resolution_fallback_reason` classifies some of those suspects as
/// protected precisely because redacting them would delete a minted token and destroy its restore
/// path. The fallback acts on the actionable subset, and the protected ones still get their
/// `Preserve` audit row.
#[test]
fn resolve_fallback_redacts_the_residual_without_deleting_protected_live_tokens() {
    let cases = [
        (LeakKind::Uncovered, FallbackReason::ResidualSuspect),
        (
            LeakKind::ClassMismatch {
                pipeline_class: PiiClass::Email,
                safety_net_class: PiiClass::Name,
            },
            FallbackReason::OverlapConflict,
        ),
    ];

    for (residual_kind, expected_reason) in cases {
        let logger = MemoryLogger::new();
        let pipeline = Pipeline::builder()
            .detector(FixedDetector {
                span: 0.."alice@example.invalid".len(),
                class: PiiClass::Email,
            })
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(MixedSecondPassNet::new(RESIDUAL_MARKER, residual_kind))
            .redaction_logger(logger.clone())
            .build()
            .expect("pipeline");
        let session = session();

        let (clean, manifest, _) = clean_with_policy(
            &pipeline,
            &session,
            RawDocument::Text(RESIDUAL_RAW.to_string()),
            &[gaze::LocaleTag::Global],
            gaze::SafetyNetPolicy::default(),
        )
        .unwrap_or_else(|err| panic!("{expected_reason:?}: {err:?}"));
        let clean = text(clean);

        // The residual is gone...
        assert!(
            !clean.contains(RESIDUAL_MARKER),
            "{expected_reason:?}: residual must be redacted"
        );
        // ...and the minted token survived intact, so its manifest entry and restore path live.
        assert_eq!(
            manifest.len(),
            1,
            "{expected_reason:?}: the protected token must survive fallback"
        );
        assert_eq!(
            session.restore_strict_text(&clean).expect("restore"),
            "alice@example.invalid ",
            "{expected_reason:?}: restore must round-trip the protected token"
        );

        // Both dispositions are on the record: the residual was redacted, the protected suspect
        // was preserved. A protected suspect that vanishes from the audit is an axis-4 hole of
        // its own — filtering it out of the redaction set is only half the fix.
        let rows = logger.entries();
        let redacted = rows
            .iter()
            .filter(|row| row.decided_by == ConflictTier::Fallback && row.action == Action::Redact)
            .collect::<Vec<_>>();
        let preserved = rows
            .iter()
            .filter(|row| row.action == Action::Preserve)
            .count();
        assert_eq!(
            redacted.len(),
            1,
            "{expected_reason:?}: one redacted residual"
        );
        assert_eq!(
            redacted[0].fallback_triggered,
            Some(expected_reason),
            "{expected_reason:?}: the row names the reason that drove the fallback"
        );
        assert_eq!(
            preserved, 1,
            "{expected_reason:?}: the protected suspect keeps its Preserve row"
        );
    }
}

/// Reports `first` on the first `check` and `second` on every later one, so the resolve pass
/// converges on a real suspect before the re-run finds a different residual.
#[derive(Clone)]
struct TwoMarkerNet {
    first: &'static str,
    second: &'static str,
    calls: Arc<AtomicUsize>,
}

impl SafetyNet for TwoMarkerNet {
    fn id(&self) -> &str {
        "two-marker"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let marker = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.first
        } else {
            self.second
        };
        let Some(start) = clean_text.find(marker) else {
            return Ok(Vec::new());
        };
        let span = start..start + marker.len();
        let Some(kind) = context.manifest.diff_against(&span, &PiiClass::Name) else {
            return Ok(Vec::new());
        };
        Ok(vec![LeakSuspect::new(
            span,
            PiiClass::Name,
            "two-marker",
            Some(0.99),
            kind,
            PiiClass::Name.to_canonical_str(),
            None,
        )])
    }
}

/// The other half of the stale-report defect: a **non-empty** primary report.
///
/// The resolve pass tokenizes the first-pass suspect, which shifts every later offset. The
/// primary report still describes pre-resolve coordinates, so a fallback that reads it redacts
/// bytes that have since become part of a minted token — deleting the token and leaving the actual
/// residual in place.
#[test]
fn resolve_fallback_does_not_redact_stale_pre_resolve_spans() {
    let pipeline = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(TwoMarkerNet {
            first: "Dr. Schmidt",
            second: "tail",
            calls: Arc::new(AtomicUsize::new(0)),
        })
        .build()
        .expect("pipeline");
    let session = session();
    let raw = "Dr. Schmidt tail";

    let (clean, manifest, _) = clean_with_policy(
        &pipeline,
        &session,
        RawDocument::Text(raw.to_string()),
        &[gaze::LocaleTag::Global],
        gaze::SafetyNetPolicy::default(),
    )
    .expect("default policy clean");
    let clean = text(clean);

    // The first-pass suspect was resolved into a token, and that token is intact: the fallback
    // did not delete it by acting on its pre-resolve span.
    assert_eq!(manifest.len(), 1, "the resolved name token must survive");
    assert!(
        clean.starts_with(&clean[..manifest[0].clean_span.end]),
        "the minted token must be whole"
    );
    assert_eq!(
        session.restore_strict_text(&clean).expect("restore"),
        "Dr. Schmidt ",
        "the resolved name restores; only the residual is gone"
    );
    // The second-pass residual is the thing that got removed.
    assert!(!clean.contains("tail"), "the residual must be redacted");
}

/// The returned `LeakReport` must mention a residual that only the post-resolution re-run saw.
///
/// The report handed back to the caller is the first pass's. When the residual arrives on the
/// re-run, a boundary that decides on that report — the CLI's tolerant-mode deprecation warning,
/// an adopter's "did anything leak?" check — was told nothing was found, while under `tolerant`
/// the residual bytes shipped and under `redact` they were destroyed one-way. Both are outcomes a
/// caller must be able to see.
#[test]
fn returned_report_surfaces_a_residual_that_only_the_re_run_found() {
    for (on_residual, residual_ships) in [
        (gaze::SafetyNetFallback::Tolerant, true),
        (gaze::SafetyNetFallback::Redact, false),
    ] {
        let session = session();
        let (clean, _, report) = clean_with_policy(
            &residual_pipeline(MemoryLogger::new()),
            &session,
            RawDocument::Text(RESIDUAL_RAW.to_string()),
            &[gaze::LocaleTag::Global],
            gaze::SafetyNetPolicy::new(gaze::SafetyNetMode::Resolve, on_residual),
        )
        .unwrap_or_else(|err| panic!("{on_residual:?}: {err:?}"));

        assert_eq!(
            text(clean).contains(RESIDUAL_MARKER),
            residual_ships,
            "{on_residual:?}: fixture must actually exercise the residual path"
        );
        assert_eq!(
            report.stats.suspect_count, 1,
            "{on_residual:?}: the caller must see the re-run's residual"
        );
        assert_eq!(
            report.stats.uncovered_count, 1,
            "{on_residual:?}: the CLI's tolerant warning gates on this count"
        );
        assert_eq!(report.suspects[0].safety_net_id, "second-pass");
    }
}

/// The converse: a converged resolve must not inflate the report with re-run duplicates.
///
/// The re-run happens on every `Resolve` pass, and a deterministic net that re-reports a suspect
/// it already reported would double every entry if the merge were unconditional.
#[test]
fn returned_report_is_not_inflated_when_resolve_converges() {
    let net = MockNet::new(Some(0..5), PiiClass::Email);
    let session = session();
    let (_, _, report) = clean_with_policy(
        &pipeline_with_net(Some(net)),
        &session,
        RawDocument::Text("Reach alice@example.invalid".to_string()),
        &[gaze::LocaleTag::Global],
        gaze::SafetyNetPolicy::default(),
    )
    .expect("converged resolve");

    assert_eq!(report.stats.suspect_count, 1, "no duplicate suspects");
}

/// Which up-front refusal the first-pass fixture drives.
#[derive(Debug, Clone, Copy)]
enum FirstPassRefusal {
    /// A class mismatch outside every token: `resolve_safety_net_suspects` refuses in its
    /// classification loop with `OverlapConflict`.
    ClassMismatchOutsideTokens,
    /// An uncovered span slicing a multi-byte character: phase 2's char-boundary check refuses
    /// with `ResidualSuspect`.
    NonCharBoundary,
}

/// Reports on **every** check — no call counter, no statefulness.
///
/// This is the shape that makes the first-pass refusal branch reachable with an ordinary
/// deterministic backend: one span inside the token the primary pass already minted, plus one
/// span that makes the resolver refuse before it mutates anything.
#[derive(Clone)]
struct DeterministicMixedNet {
    refusal: FirstPassRefusal,
    residual: &'static str,
}

impl SafetyNet for DeterministicMixedNet {
    fn id(&self) -> &str {
        "deterministic-mixed"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let suspect = |span: Range<usize>, kind| {
            LeakSuspect::new(
                span,
                PiiClass::Name,
                "deterministic-mixed",
                Some(0.99),
                kind,
                PiiClass::Name.to_canonical_str(),
                None,
            )
        };
        let mut suspects = Vec::new();
        // (a) Inside the minted token: already protected, must survive whatever happens next.
        if let Some(minted) = context.manifest.spans.first() {
            let span = minted.clean_span.clone();
            if let Some(kind) = context.manifest.diff_against(&span, &PiiClass::Name) {
                suspects.push(suspect(span, kind));
            }
        }
        // (b) The suspect that makes the resolver refuse up front.
        if let Some(start) = clean_text.find(self.residual) {
            match self.refusal {
                FirstPassRefusal::ClassMismatchOutsideTokens => suspects.push(suspect(
                    start..start + self.residual.len(),
                    LeakKind::ClassMismatch {
                        pipeline_class: PiiClass::Email,
                        safety_net_class: PiiClass::Name,
                    },
                )),
                // Two bytes into "ré" lands mid-character.
                FirstPassRefusal::NonCharBoundary => {
                    suspects.push(suspect(start..start + 2, LeakKind::Uncovered))
                }
            }
        }
        Ok(suspects)
    }
}

/// The **first-pass** half of the fallback-filtering contract, reachable with a plain
/// deterministic net.
///
/// When `resolve_safety_net_suspects` refuses up front, it returns from inside its classification
/// loop — above its own `Preserve` logging. So without filtering, a protected suspect on this path
/// is both redacted (deleting a minted token, destroying restore) and absent from the audit. This
/// path needs no stateful backend at all: one net, one pass, one span inside a token and one span
/// that cannot be honored.
#[test]
fn first_pass_refusal_redacts_only_the_actionable_suspect_and_audits_the_protected_one() {
    let email = "alice@example.invalid";
    let cases = [
        (
            FirstPassRefusal::ClassMismatchOutsideTokens,
            "residue",
            format!("{email} residue"),
            FallbackReason::OverlapConflict,
        ),
        (
            FirstPassRefusal::NonCharBoundary,
            "rés",
            format!("{email} rés"),
            FallbackReason::ResidualSuspect,
        ),
    ];

    for (refusal, residual, raw, expected_reason) in cases {
        let logger = MemoryLogger::new();
        let pipeline = Pipeline::builder()
            .detector(FixedDetector {
                span: 0..email.len(),
                class: PiiClass::Email,
            })
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(DeterministicMixedNet { refusal, residual })
            .redaction_logger(logger.clone())
            .build()
            .expect("pipeline");
        let session = session();

        let (clean, manifest, _) = clean_with_policy(
            &pipeline,
            &session,
            RawDocument::Text(raw.clone()),
            &[gaze::LocaleTag::Global],
            gaze::SafetyNetPolicy::default(),
        )
        .unwrap_or_else(|err| panic!("{refusal:?}: {err:?}"));
        let clean = text(clean);

        // The protected token survived, so its manifest entry and restore path are intact.
        assert_eq!(
            manifest.len(),
            1,
            "{refusal:?}: the protected token must survive the fallback"
        );
        let restored = session.restore_strict_text(&clean).expect("restore");
        assert!(
            restored.starts_with(email),
            "{refusal:?}: restore must return the protected value byte-exactly, got {restored:?}"
        );

        // Exactly one row for each disposition, and the redaction names the refusal reason.
        let rows = logger.entries();
        let redacted = rows
            .iter()
            .filter(|row| row.decided_by == ConflictTier::Fallback && row.action == Action::Redact)
            .collect::<Vec<_>>();
        let preserved = rows
            .iter()
            .filter(|row| row.action == Action::Preserve)
            .count();
        assert_eq!(
            redacted.len(),
            1,
            "{refusal:?}: one redacted actionable row"
        );
        assert_eq!(
            redacted[0].fallback_triggered,
            Some(expected_reason),
            "{refusal:?}: the row names the refusal reason"
        );
        assert_eq!(
            preserved, 1,
            "{refusal:?}: the protected suspect keeps its Preserve row"
        );
    }
}

/// Axis-1 contract hole: a caller asking for enforcement on a structured document was given
/// observation. The structured arm of `clean_target_with_safety_net_policy_detect_context` ran the
/// nets per field, collected the report, and returned `Ok` — the suspect bytes were still in the
/// document, and nothing in the return value said the requested action had not been performed.
#[test]
fn structured_documents_do_not_silently_observe_when_enforcement_is_requested() {
    let raw_email = "alice@example.invalid";
    for mode in [gaze::SafetyNetMode::Redact, gaze::SafetyNetMode::Resolve] {
        let net =
            MockNet::new(Some(0..raw_email.len()), PiiClass::Email).with_field_path("$.email");
        let pipeline = pipeline_with_net(Some(net));
        let session = session();
        let raw = RawDocument::Structured(BTreeMap::from([(
            "email".to_string(),
            Value::String(raw_email.to_string()),
        )]));

        let result = clean_with_policy(
            &pipeline,
            &session,
            raw,
            &[gaze::LocaleTag::Global],
            gaze::SafetyNetPolicy::new(mode, gaze::SafetyNetFallback::Redact),
        );

        match result {
            // Fail closed: the request is refused with a typed error.
            Err(err) => assert!(
                matches!(
                    err,
                    gaze::Error::UnsupportedSafetyNetModeForStructured { .. }
                ),
                "{mode:?}: unexpected error {err:?}"
            ),
            // Or the enforcement actually happened. What must never occur is Ok with the
            // suspect bytes still present.
            Ok((clean, _, _)) => {
                let CleanDocument::Structured(fields) = clean else {
                    panic!("expected structured output");
                };
                let Value::String(email) = &fields["email"] else {
                    panic!("expected string leaf");
                };
                assert!(
                    !email.contains(raw_email),
                    "{mode:?}: enforcement requested, observation performed — suspect bytes shipped"
                );
            }
        }
    }
}

/// Nested-traversal parity across all three `LeafOp`s.
///
/// The three structured walkers were near-identical copies and had already drifted (see the
/// root-path note below). Folding them into one `walk_structured` makes divergence a compile-time
/// impossibility for everything except what the op deliberately varies; this pins the parts that
/// must stay identical.
#[test]
fn structured_walk_has_nested_parity_across_every_leaf_op() {
    fn document() -> BTreeMap<String, Value> {
        let contact = BTreeMap::from([(
            "email".to_string(),
            Value::String("alice@example.invalid".to_string()),
        )]);
        let user = BTreeMap::from([
            (
                "contacts".to_string(),
                Value::Array(vec![Value::Object(contact)]),
            ),
            ("empty".to_string(), Value::String(String::new())),
            ("active".to_string(), Value::Bool(true)),
            ("count".to_string(), Value::I64(7)),
        ]);
        BTreeMap::from([("user".to_string(), Value::Object(user))])
    }

    fn seen_paths(net: &MockNet) -> Vec<String> {
        let mut paths = net
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|entry| entry.field_path.clone().unwrap_or_default())
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    // Leg 1: Pseudonymize, through the plain structured entry point.
    let pseudonymize_net = MockNet::new(None, PiiClass::Email);
    let pseudonymized = pipeline_with_net(Some(pseudonymize_net.clone()))
        .redact(&session(), RawDocument::Structured(document()))
        .expect("pseudonymize structured");
    let CleanDocument::Structured(pseudonymized) = pseudonymized else {
        panic!("expected structured output");
    };
    // Pseudonymize runs no safety net at all — not even over the scalar leaves.
    assert!(seen_paths(&pseudonymize_net).is_empty());

    // Leg 2: CleanAndScan, through the observer-policy safety-net entry point.
    let clean_net = MockNet::new(None, PiiClass::Email);
    let (cleaned, _, _) = clean_with_policy(
        &pipeline_with_net(Some(clean_net.clone())),
        &session(),
        RawDocument::Structured(document()),
        &[gaze::LocaleTag::Global],
        observer_policy(),
    )
    .expect("clean structured");
    let CleanDocument::Structured(cleaned) = cleaned else {
        panic!("expected structured output");
    };

    // Leg 3: ScanOnly, through the observer scan API.
    let scan_net = MockNet::new(None, PiiClass::Email);
    pipeline_with_net(Some(scan_net.clone()))
        .scan_safety_nets_structured(&session(), &document(), &[gaze::LocaleTag::Global])
        .expect("scan structured");

    // Both rebuilding ops reconstruct the same document: nested objects, arrays, the empty
    // string, and both scalar kinds survive the walk under a preserve-only rule set.
    assert_eq!(pseudonymized, cleaned);
    assert_eq!(pseudonymized, document());

    // Both scanning ops visit the same leaves, in the same order, skipping the empty string and
    // covering both scalars.
    let cleaned_paths = seen_paths(&clean_net);
    let scanned_paths = seen_paths(&scan_net);
    assert_eq!(
        cleaned_paths,
        vec![
            "$.user.active".to_string(),
            "$.user.contacts[0].email".to_string(),
            "$.user.count".to_string(),
        ]
    );
    // Documented divergence, preserved rather than silently unified: `scan_safety_nets_structured`
    // reports bare-key roots where the cleaning path reports JSONPath-style ones (todo #2958).
    assert_eq!(
        scanned_paths,
        cleaned_paths
            .iter()
            .map(|path| path.trim_start_matches("$.").to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        !cleaned_paths.iter().any(|path| path.ends_with("empty")),
        "empty string leaves are skipped by both scanning ops"
    );
}
