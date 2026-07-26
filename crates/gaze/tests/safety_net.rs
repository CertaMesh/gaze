use std::collections::BTreeMap;
use std::ops::Range;
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
fn clean_with_safety_net_text_path_returns_manifest_and_report() {
    let net = MockNet::new(Some(0..5), PiiClass::Email);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net(
            &session,
            RawDocument::Text("Reach alice@example.invalid".to_string()),
            &[gaze::LocaleTag::Global],
        )
        .expect("safety net clean");

    assert_eq!(text(clean), "Reach alice@example.invalid");
    assert!(manifest.is_empty());
    assert_eq!(report.stats.suspect_count, 1);
    assert_eq!(report.suspects[0].kind, LeakKind::Uncovered);
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
        let (clean, _, report) = pipeline
            .clean_with_safety_net(&session, raw.clone(), &[gaze::LocaleTag::Global])
            .expect("safety net clean");
        assert_eq!(text(clean), baseline);
        assert_eq!(
            report.suspects.first().map(|suspect| &suspect.kind),
            expected_kind.as_ref()
        );
    }

    let skipped =
        MockNet::new(Some(0..9), PiiClass::Email).with_locales(vec![gaze::LocaleTag::EnUs]);
    let (clean, _, report) = tokenizing_pipeline_with_net(skipped)
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::DeDe])
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

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::Global])
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

    let (_, _, report) = pipeline
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::DeDe])
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

    let err = pipeline
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::Global])
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
