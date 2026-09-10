use super::*;
use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Clone)]
struct Baseline {
    candidates: Vec<Candidate>,
    calls: Arc<AtomicUsize>,
}
impl Recognizer for Baseline {
    fn id(&self) -> &str {
        "synthetic.baseline"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn locale_basis(&self) -> gaze_types::LocaleBasis {
        gaze_types::LocaleBasis::Format
    }
    fn detect(
        &self,
        _: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.candidates.clone())
    }
}
fn builder(candidates: Vec<Candidate>) -> PipelineBuilder {
    Pipeline::builder().recognizer(Baseline {
        candidates,
        calls: Arc::new(AtomicUsize::new(0)),
    })
}
fn candidate(span: Range<usize>, supplemental: bool) -> Candidate {
    let source = if supplemental {
        "synthetic.supplement"
    } else {
        "synthetic.baseline"
    };
    Candidate::new(
        span,
        PiiClass::Name,
        source,
        0.95,
        4,
        Some("synthetic canonical".into()),
        "counter",
        source,
        ConflictTier::None,
        vec![],
    )
    .with_recognizer_version_id("synthetic-v1")
}
fn tokenize() -> Vec<FrozenRule> {
    vec![FrozenRule::Default(Action::Tokenize)]
}
fn run(
    raw: &str,
    base: Vec<Candidate>,
    extras: Vec<Candidate>,
    rules: &[FrozenRule],
) -> LockOutput {
    run_request(
        builder(base),
        rules,
        raw,
        DocumentKind::Text,
        PrefixCacheWriteMode::Allow,
        &[],
        &DictionaryBundle::default(),
        |_, _| Ok(SupplementalBatch::Complete(extras)),
    )
    .unwrap()
}
fn error_code<T>(result: Result<T>, expected: &str) {
    match result {
        Err(Error::RecognizerDetect(gaze_types::DetectError::Backend { message, .. })) => {
            assert_eq!(message, expected)
        }
        Err(other) => panic!("unexpected error: {other}"),
        Ok(_) => panic!("expected failure {expected}"),
    }
}
// This oracle observes actual emitted bytes, session lookup and original gaps, not admission logic.
fn prove_output(raw: &str, output: &LockOutput) {
    let mut restored = String::new();
    let (mut raw_cursor, mut clean_cursor) = (0, 0);
    for span in &output.clean.manifest {
        assert!(span.raw_span.start >= raw_cursor);
        assert!(span.clean_span.start >= clean_cursor);
        assert_eq!(
            &raw[raw_cursor..span.raw_span.start],
            &output.clean.text[clean_cursor..span.clean_span.start]
        );
        restored.push_str(&output.clean.text[clean_cursor..span.clean_span.start]);
        let value = output
            .session
            .restore_strict(&output.clean.text[span.clean_span.clone()])
            .unwrap();
        assert_eq!(value, raw[span.raw_span.clone()]);
        restored.push_str(&value);
        raw_cursor = span.raw_span.end;
        clean_cursor = span.clean_span.end;
    }
    assert_eq!(&raw[raw_cursor..], &output.clean.text[clean_cursor..]);
    restored.push_str(&output.clean.text[clean_cursor..]);
    assert_eq!(restored, raw);
    assert_eq!(output.trace.len(), output.clean.manifest.len());
    for (trace, span) in output.trace.iter().zip(&output.clean.manifest) {
        assert_eq!(trace.raw_start()..trace.raw_end(), span.raw_span);
    }
}
fn prove_pair(raw: &str, control: &LockOutput, augmented: &LockOutput) {
    assert_eq!(control.plan.baseline, augmented.plan.baseline);
    for base in &control.plan.baseline {
        assert!(augmented.plan.final_items.contains(base));
    }
    for span in &control.clean.manifest {
        for byte in span.raw_span.clone() {
            assert!(augmented
                .clean
                .manifest
                .iter()
                .any(|item| item.raw_span.contains(&byte)));
        }
    }
    for pair in augmented.plan.final_items.windows(2) {
        assert!(pair[0].raw.end <= pair[1].raw.start);
    }
    prove_output(raw, control);
    prove_output(raw, augmented);
}

#[test]
fn overlap_geometry_locks_complete_baseline_including_preserve() {
    let raw = "abcdefghijklmnop";
    let base = vec![candidate(3..7, false), candidate(10..13, false)];
    for preserve in [false, true] {
        let rules = vec![FrozenRule::Default(if preserve {
            Action::Preserve
        } else {
            Action::Tokenize
        })];
        let control = run(raw, base.clone(), vec![], &rules);
        for span in [3..7, 4..6, 1..8, 1..5, 5..9, 4..12, 0..16] {
            let mut challenger = candidate(span, true);
            challenger.priority = i32::MAX;
            challenger.score = 1.0;
            let augmented = run(raw, base.clone(), vec![challenger], &rules);
            assert_eq!(augmented.plan.dispositions, [Disposition::BaselineOverlap]);
            assert_eq!(augmented.clean.text, control.clean.text);
            prove_pair(raw, &control, &augmented);
        }
        let augmented = run(
            raw,
            base.clone(),
            vec![
                candidate(0..3, true),
                candidate(7..10, true),
                candidate(13..16, true),
            ],
            &rules,
        );
        assert_eq!(augmented.plan.dispositions, [Disposition::Admitted; 3]);
        prove_pair(raw, &control, &augmented);
    }
}

#[test]
fn same_original_normalized_context_once_even_empty_preserve() {
    for raw in ["", "\u{200c}\u{200d}", "\u{200d}Ａé\u{200c}B\u{200d}"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let dictionaries = DictionaryBundle::default();
        let locales = [crate::LocaleTag::Global];
        let baseline = Baseline {
            candidates: vec![],
            calls: calls.clone(),
        };
        let mut supplement_calls = 0;
        let output = run_request(
            Pipeline::builder().recognizer(baseline),
            &[],
            raw,
            DocumentKind::Text,
            PrefixCacheWriteMode::Suppress,
            &locales,
            &dictionaries,
            |text, ctx| {
                supplement_calls += 1;
                assert_eq!(text, normalize(raw).text);
                assert!(std::ptr::eq(ctx.dictionaries, &dictionaries));
                assert_eq!(ctx.locale_chain, locales);
                Ok(SupplementalBatch::Complete(vec![]))
            },
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(supplement_calls, 1);
        assert_eq!(output.clean.text, raw);
        prove_output(raw, &output);
    }
}

#[test]
fn normalization_expansion_raw_overlap_and_joiner_gaps() {
    // U+0344 NFC expands to two combining scalars with one raw origin.
    let raw = "\u{200d}Ａ\u{200c}\u{0344}B\u{200d}";
    assert_eq!(normalize(raw).text, "A\u{0308}\u{0301}B");
    let control = run(raw, vec![candidate(1..3, false)], vec![], &tokenize());
    let augmented = run(
        raw,
        vec![candidate(1..3, false)],
        vec![candidate(3..5, true)],
        &tokenize(),
    );
    assert_eq!(augmented.plan.dispositions, [Disposition::BaselineOverlap]);
    prove_pair(raw, &control, &augmented);
    let extras = run(
        raw,
        vec![],
        vec![candidate(1..3, true), candidate(3..5, true)],
        &tokenize(),
    );
    assert_eq!(
        extras.plan.dispositions,
        [Disposition::Admitted, Disposition::SupplementalOverlap]
    );
    prove_output(raw, &extras);
    let interior = run(raw, vec![candidate(0..6, false)], vec![], &tokenize());
    assert_eq!(interior.plan.baseline[0].raw, 3..raw.len() - 3);
    prove_output(raw, &interior);
    error_code(
        run_request(
            builder(vec![candidate(1..3, false), candidate(3..5, false)]),
            &tokenize(),
            raw,
            DocumentKind::Text,
            PrefixCacheWriteMode::Allow,
            &[],
            &DictionaryBundle::default(),
            |_, _| Ok(SupplementalBatch::Complete(vec![])),
        ),
        "LOCK_INVALID_MAP",
    );
}

#[test]
fn frozen_builtin_policy_order_custom_classes_and_defaults() {
    let mut custom = candidate(0..2, false);
    custom.class = PiiClass::custom("arbitrary-baseline-metadata");
    let cases = [
        (vec![], false),
        (tokenize(), true),
        (
            vec![
                FrozenRule::Default(Action::Preserve),
                FrozenRule::Class(custom.class.clone(), Action::Tokenize),
            ],
            false,
        ),
        (
            vec![
                FrozenRule::Class(custom.class.clone(), Action::Tokenize),
                FrozenRule::Default(Action::Preserve),
            ],
            true,
        ),
    ];
    for (rules, emitted) in cases {
        let output = run(
            "abcd",
            vec![custom.clone()],
            vec![candidate(2..4, true)],
            &rules,
        );
        assert_eq!(
            output
                .clean
                .manifest
                .iter()
                .any(|span| span.raw_span == (0..2)),
            emitted
        );
        assert_eq!(output.plan.baseline[0].candidate.class, custom.class);
        assert_eq!(
            output.plan.baseline[0].candidate.canonical_form,
            custom.canonical_form
        );
        prove_output("abcd", &output);
    }
}

#[test]
fn entire_late_batch_validation_precedes_any_audit_or_emission() {
    let mut invalid = vec![
        candidate(3..9, true),
        candidate(3..3, true),
        candidate(0..2, true),
    ];
    for score in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        let mut c = candidate(3..4, true);
        c.score = score;
        invalid.push(c);
    }
    for late in invalid {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let result = run_request(
            builder(vec![candidate(0..2, false)]).redaction_logger(CaptureLog {
                entries: logs.clone(),
            }),
            &tokenize(),
            "abcdef",
            DocumentKind::Text,
            PrefixCacheWriteMode::Allow,
            &[],
            &DictionaryBundle::default(),
            |_, _| {
                Ok(SupplementalBatch::Complete(vec![
                    candidate(0..1, true),
                    late,
                ]))
            },
        );
        error_code(result, "LOCK_INVALID_BATCH");
        assert!(logs.lock().unwrap().is_empty());
    }
    let mut unknown = candidate(3..4, true);
    unknown.class = PiiClass::custom("unknown");
    error_code(
        run_request(
            builder(vec![]),
            &tokenize(),
            "abcdef",
            DocumentKind::Text,
            PrefixCacheWriteMode::Allow,
            &[],
            &DictionaryBundle::default(),
            |_, _| Ok(SupplementalBatch::Complete(vec![unknown])),
        ),
        "LOCK_UNKNOWN_LABEL",
    );
    for batch in [
        Err(LockError::ProviderFailed),
        Ok(SupplementalBatch::Incomplete),
    ] {
        let expected = if batch.is_err() {
            "LOCK_PROVIDER_FAILED"
        } else {
            "LOCK_INCOMPLETE"
        };
        error_code(
            run_request(
                builder(vec![]),
                &tokenize(),
                "",
                DocumentKind::Text,
                PrefixCacheWriteMode::Allow,
                &[],
                &DictionaryBundle::default(),
                |_, _| batch,
            ),
            expected,
        );
    }
}

#[test]
fn invalid_final_baseline_and_maps_fail_closed_without_lossy_conversion() {
    let pipeline = baseline_pipeline(builder(vec![]), &tokenize(), DocumentKind::Text).unwrap();
    for span in [0..0, 0..8, 1..2] {
        error_code(
            prepare_plan(
                &pipeline,
                "éabc",
                &normalize("éabc"),
                vec![candidate(span, false)],
                SupplementalBatch::Complete(vec![]),
            ),
            "LOCK_INVALID_BATCH",
        );
    }
    for supplemental in [false, true] {
        let batch = vec![candidate(0..1, supplemental); MAX_CANDIDATES + 1];
        error_code(
            prepare_batch(&pipeline, "abc", &normalize("abc"), batch, supplemental),
            "LOCK_INVALID_BATCH",
        );
        let batch = vec![candidate(1..3, supplemental), candidate(2..3, supplemental)];
        error_code(
            prepare_batch(&pipeline, "abc", &normalize("abc"), batch, supplemental),
            "LOCK_INVALID_BATCH",
        );
    }
    for spans in [
        vec![],
        vec![(0, 9); 3],
        vec![(0, 1), (2, 3), (1, 2)],
        vec![(0, 2), (1, 3), (2, 3)],
    ] {
        error_code(
            prepare_plan(
                &pipeline,
                "abc",
                &NormalizedText {
                    text: "abc".into(),
                    spans,
                },
                vec![],
                SupplementalBatch::Complete(vec![]),
            ),
            "LOCK_INVALID_MAP",
        );
    }
    error_code(
        prepare_plan(
            &pipeline,
            "é",
            &NormalizedText {
                text: "é".into(),
                spans: vec![(0, 1), (0, 1)],
            },
            vec![],
            SupplementalBatch::Complete(vec![]),
        ),
        "LOCK_INVALID_MAP",
    );
}

struct OpaqueRule(Arc<AtomicUsize>);
impl Rule for OpaqueRule {
    fn action(&self, _: &PiiClass, _: &RuleContext) -> Option<Action> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Some(Action::Tokenize)
    }
}
#[test]
fn guards_precede_detection_and_supplement_cannot_supply_floor() {
    let calls = Arc::new(AtomicUsize::new(0));
    let opaque_calls = Arc::new(AtomicUsize::new(0));
    for kind in [DocumentKind::Text, DocumentKind::Structured] {
        for config in 0..4 {
            let mut b = Pipeline::builder().recognizer(Baseline {
                candidates: vec![],
                calls: calls.clone(),
            });
            if config == 0 {
                b = b.rule(OpaqueRule(opaque_calls.clone()));
            }
            if config == 1 {
                b = b.enable_prefix_cache();
            }
            if config == 2 {
                b = b.register_safety_net(GuardNet);
            }
            if config == 3 && kind == DocumentKind::Text {
                continue;
            }
            error_code(
                run_request(
                    b,
                    &[],
                    "",
                    kind,
                    PrefixCacheWriteMode::Suppress,
                    &[],
                    &DictionaryBundle::default(),
                    |_, _| panic!("supplement must not run"),
                ),
                "LOCK_UNSUPPORTED_SCOPE",
            );
        }
    }
    error_code(
        run_request(
            Pipeline::builder(),
            &tokenize(),
            "abcd",
            DocumentKind::Text,
            PrefixCacheWriteMode::Allow,
            &[],
            &DictionaryBundle::default(),
            |_, _| Ok(SupplementalBatch::Complete(vec![candidate(0..4, true)])),
        ),
        "LOCK_MISSING_BASELINE",
    );
    error_code(
        run_request(
            builder(vec![]),
            &[
                FrozenRule::Class(PiiClass::Name, Action::Tokenize),
                FrozenRule::Default(Action::Redact),
            ],
            "",
            DocumentKind::Text,
            PrefixCacheWriteMode::Allow,
            &[],
            &DictionaryBundle::default(),
            |_, _| panic!("must preflight all rules"),
        ),
        "LOCK_UNSUPPORTED_ACTION",
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(opaque_calls.load(Ordering::SeqCst), 0);
    let output = run("abcd", vec![], vec![candidate(0..4, true)], &tokenize());
    assert_eq!(output.clean.manifest.len(), 1);
    prove_output("abcd", &output);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    #[test]
    fn mapped_plans_preserve_actual_baseline_bytes_and_exact_restore(
        segments in prop::collection::vec((0u8..4, any::<bool>(), any::<bool>()), 1..30)
    ) {
        let mut raw = String::from("\u{200d}");
        let mut base = Vec::new();
        let mut extras = Vec::new();
        for (shape, is_base, preserve) in segments {
            let start = normalize(&raw).text.len();
            raw.push_str(match shape { 0 => "Ａ", 1 => "é", 2 => "xy", _ => "\u{0344}" });
            let end = normalize(&raw).text.len();
            let mut c = candidate(start..end, !is_base);
            if preserve { c.class = PiiClass::Email; }
            if is_base { base.push(c); } else { extras.push(c); }
            raw.push('\u{200c}');
        }
        let rules = [FrozenRule::Class(PiiClass::Email, Action::Preserve), FrozenRule::Default(Action::Tokenize)];
        let control = run(&raw, base.clone(), vec![], &rules);
        let augmented = run(&raw, base, extras, &rules);
        prove_pair(&raw, &control, &augmented);
    }
}

struct CaptureLog {
    entries: Arc<Mutex<Vec<RedactionEntry>>>,
}
impl RedactionLogger for CaptureLog {
    fn log(&self, entry: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        self.entries.lock().unwrap().push(entry.clone());
        Ok(())
    }
}
struct GuardNet;
impl SafetyNet for GuardNet {
    fn id(&self) -> &str {
        "synthetic.guard"
    }
    fn supported_locales(&self) -> &[crate::LocaleTag] {
        &[crate::LocaleTag::Global]
    }
    fn check(
        &self,
        _: &str,
        _: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        panic!("guard must reject before scan")
    }
}

#[test]
fn empty_supplement_matches_existing_pipeline_emission_and_complete_resolved_metadata() {
    let raw = "abcd abcd";
    let mut original = candidate(0..4, false);
    original.merged_sources = vec!["synthetic.loser".into()];
    original.priority = 17;
    let base = vec![original, candidate(5..9, false)];
    let pipeline =
        baseline_pipeline(builder(base.clone()), &tokenize(), DocumentKind::Text).unwrap();
    let dictionaries = DictionaryBundle::default();
    let (expected, _) = pipeline
        .registry
        .detect_all_resolved(raw, &DetectContext::new(&[], &dictionaries))
        .unwrap();
    let output = run(raw, base, vec![], &tokenize());
    assert_eq!(
        output
            .plan
            .baseline
            .iter()
            .map(|item| item.candidate.clone())
            .collect::<Vec<_>>(),
        expected
    );
    let session =
        Session::new_with_session_hex_for_tests(crate::Scope::Ephemeral, [1, 2, 3, 4]).unwrap();
    let clean = pipeline
        .redact_text_with_manifest_uncached(
            &mut ProtectionTarget::Live(&session),
            raw,
            None,
            DocumentKind::Text,
            &[],
            &dictionaries,
            None,
        )
        .unwrap();
    assert_eq!(clean.text, output.clean.text);
    assert_eq!(clean.manifest, output.clean.manifest);
    prove_output(raw, &output);
}

#[derive(Clone)]
struct LocaleBaseline {
    id: &'static str,
    locale: crate::LocaleTag,
    score: f32,
    calls: Arc<AtomicUsize>,
}
impl Recognizer for LocaleBaseline {
    fn id(&self) -> &str {
        self.id
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn locales(&self) -> &[crate::LocaleTag] {
        std::slice::from_ref(&self.locale)
    }
    fn locale_basis(&self) -> gaze_types::LocaleBasis {
        gaze_types::LocaleBasis::Document
    }
    fn detect(
        &self,
        _: &str,
        ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(ctx.locale_chain, std::slice::from_ref(&self.locale));
        let mut c = candidate(0..4, false);
        c.recognizer_id = self.id.into();
        c.source = self.id.into();
        c.score = self.score;
        Ok(vec![c])
    }
}
#[test]
fn locale_fallback_score_filtering_remain_baseline_owned() {
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let locales = [
        crate::LocaleTag::parse("de-DE").unwrap(),
        crate::LocaleTag::Global,
    ];
    let build = || {
        Pipeline::builder()
            .recognizer(LocaleBaseline {
                id: "synthetic.low",
                locale: locales[0].clone(),
                score: -0.01,
                calls: first.clone(),
            })
            .recognizer(LocaleBaseline {
                id: "synthetic.fallback",
                locale: locales[1].clone(),
                score: 0.95,
                calls: second.clone(),
            })
    };
    let dictionaries = DictionaryBundle::default();
    let control = run_request(
        build(),
        &tokenize(),
        "abcdefgh",
        DocumentKind::Text,
        PrefixCacheWriteMode::Allow,
        &locales,
        &dictionaries,
        |_, _| Ok(SupplementalBatch::Complete(vec![])),
    )
    .unwrap();
    let augmented = run_request(
        build(),
        &tokenize(),
        "abcdefgh",
        DocumentKind::Text,
        PrefixCacheWriteMode::Suppress,
        &locales,
        &dictionaries,
        |_, _| Ok(SupplementalBatch::Complete(vec![candidate(4..8, true)])),
    )
    .unwrap();
    assert_eq!(first.load(Ordering::SeqCst), 2);
    assert_eq!(second.load(Ordering::SeqCst), 2);
    assert_eq!(
        control.plan.baseline[0].candidate.recognizer_id,
        "synthetic.fallback"
    );
    prove_pair("abcdefgh", &control, &augmented);
}

#[test]
fn family_resolution_and_missing_anchor_are_frozen_before_supplement() {
    for tie in [false, true] {
        let mut alpha = candidate(0..4, false);
        alpha.recognizer_id = "synthetic.alpha".into();
        alpha.source = alpha.recognizer_id.clone();
        alpha.class = PiiClass::custom("alpha");
        let mut beta = alpha.clone();
        beta.recognizer_id = "synthetic.beta".into();
        beta.source = beta.recognizer_id.clone();
        beta.class = PiiClass::custom("beta");
        let make_builder = || {
            builder(if tie {
                vec![alpha.clone(), beta.clone()]
            } else {
                vec![alpha.clone()]
            })
            .register_collision(
                "synthetic.alpha",
                CollisionMembership::new(
                    "synthetic-family",
                    "alpha",
                    10,
                    if tie {
                        None
                    } else {
                        Some("synthetic-anchor".into())
                    },
                ),
            )
            .register_collision(
                "synthetic.beta",
                CollisionMembership::new("synthetic-family", "beta", 10, None),
            )
        };
        let dictionaries = DictionaryBundle::default();
        let pipeline = baseline_pipeline(make_builder(), &tokenize(), DocumentKind::Text).unwrap();
        let (expected, _) = pipeline
            .registry
            .detect_all_resolved("abcd efgh", &DetectContext::new(&[], &dictionaries))
            .unwrap();
        let output = run_request(
            make_builder(),
            &tokenize(),
            "abcd efgh",
            DocumentKind::Text,
            PrefixCacheWriteMode::Allow,
            &[],
            &dictionaries,
            |_, _| Ok(SupplementalBatch::Complete(vec![candidate(5..9, true)])),
        )
        .unwrap();
        assert_eq!(
            output
                .plan
                .baseline
                .iter()
                .map(|item| item.candidate.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            output.plan.baseline[0].candidate.class,
            PiiClass::custom("family:synthetic-family")
        );
        prove_output("abcd efgh", &output);
    }
}

struct FailLog;
impl RedactionLogger for FailLog {
    fn log(&self, _: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        Err(RedactionLogError::Backend("synthetic failure".into()))
    }
}
#[test]
fn audit_failure_returns_only_whole_request_error() {
    let result = run_request(
        builder(vec![candidate(0..2, false)]).redaction_logger(FailLog),
        &tokenize(),
        "abcd",
        DocumentKind::Text,
        PrefixCacheWriteMode::Allow,
        &[],
        &DictionaryBundle::default(),
        |_, _| Ok(SupplementalBatch::Complete(vec![candidate(2..4, true)])),
    );
    assert!(matches!(result, Err(Error::RedactionLog(_))));
}

#[test]
fn prepared_actions_drive_audit_manifest_and_trace_consistently() {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let mut preserved = candidate(0..2, false);
    preserved.class = PiiClass::Email;
    let output = run_request(
        builder(vec![preserved]).redaction_logger(CaptureLog {
            entries: logs.clone(),
        }),
        &[
            FrozenRule::Class(PiiClass::Email, Action::Preserve),
            FrozenRule::Default(Action::Tokenize),
        ],
        "abcd",
        DocumentKind::Text,
        PrefixCacheWriteMode::Allow,
        &[],
        &DictionaryBundle::default(),
        |_, _| Ok(SupplementalBatch::Complete(vec![candidate(2..4, true)])),
    )
    .unwrap();
    let entries = logs.lock().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].action, Action::Preserve);
    assert_eq!(entries[1].action, Action::Tokenize);
    assert_eq!(output.clean.manifest.len(), 1);
    assert_eq!(output.clean.manifest[0].raw_span, 2..4);
    prove_output("abcd", &output);
}

#[test]
fn invalid_final_baseline_score_refuses_instead_of_becoming_empty() {
    let pipeline = baseline_pipeline(builder(vec![]), &tokenize(), DocumentKind::Text).unwrap();
    for score in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        let mut late = candidate(2..4, false);
        late.score = score;
        error_code(
            prepare_plan(
                &pipeline,
                "abcd",
                &normalize("abcd"),
                vec![candidate(0..2, false), late],
                SupplementalBatch::Complete(vec![]),
            ),
            "LOCK_INVALID_BATCH",
        );
    }
}

struct OrderedRule(Arc<Mutex<Vec<&'static str>>>);
impl Rule for OrderedRule {
    fn action(&self, _: &PiiClass, _: &RuleContext) -> Option<Action> {
        self.0.lock().unwrap().push("rule");
        Some(Action::Tokenize)
    }
}
struct OrderedFailLog(Arc<Mutex<Vec<&'static str>>>);
impl RedactionLogger for OrderedFailLog {
    fn log(&self, _: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        self.0.lock().unwrap().push("log");
        Err(RedactionLogError::Backend("synthetic stop".into()))
    }
}
#[test]
fn ordinary_emitter_keeps_stateful_rule_and_logger_failure_short_circuit_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let pipeline = builder(vec![candidate(0..2, false), candidate(2..4, false)])
        .rule(OrderedRule(events.clone()))
        .redaction_logger(OrderedFailLog(events.clone()))
        .build()
        .unwrap();
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let result = pipeline.redact_text_with_manifest_uncached(
        &mut ProtectionTarget::Live(&session),
        "abcd",
        None,
        DocumentKind::Text,
        &[],
        &DictionaryBundle::default(),
        None,
    );
    assert!(matches!(result, Err(Error::RedactionLog(_))));
    assert_eq!(*events.lock().unwrap(), ["rule", "log"]);
    assert!(session.snapshot_entries().is_empty());
}
