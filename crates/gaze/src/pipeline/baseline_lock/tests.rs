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
        crate::LocaleTag::parse("en-US").unwrap(),
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
            PiiClass::family("synthetic-family")
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

#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
mod live {
    use super::*;
    use gaze_recognizers::redact_live::{label_class, RedactDetector};
    use serde_json::{json, Value};
    use std::os::unix::fs::PermissionsExt;

    // Real checked-provider boundary, fabricated protocol only. No model files or environment.
    fn bridge(reply: Value) -> (tempfile::TempDir, RedactDetector) {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("synthetic-bridge");
        let fixture = serde_json::to_string(&reply).unwrap();
        let program = format!(r##"#!/usr/bin/python3
import json, pathlib, sys
reply = json.loads({fixture:?})
for line in sys.stdin:
    request = json.loads(line)
    with (pathlib.Path(sys.argv[1]) / 'calls.jsonl').open('a') as log:
        log.write(json.dumps(request) + '\n')
    response = dict(reply)
    response['id'] = request['id']
    print(json.dumps(response), flush=True)
"##);
        std::fs::write(&executable, program).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let detector = RedactDetector::new(executable, dir.path().to_path_buf()).unwrap();
        (dir, detector)
    }
    fn reply(spans: Value) -> Value {
        json!({"version":1,"id":1,"kind":"primary","complete":true,"threshold":0.6,"org":true,
            "content_tokens":1,"planned_windows":1,"completed_windows":1,"spans":spans,
            "dispositions":{"threshold":0,"arbitration":0,"cleanup":0,"special_tokens":0},"error":null})
    }
    fn span(start: usize, end: usize, label: &str) -> Value {
        // A low but valid score must not become a second confidence filter.
        json!({"start":start,"end":end,"label":label,"score":0.1})
    }
    fn closed(rules: Vec<crate::RuleSpec>, base: Vec<Candidate>) -> BenchmarkBaselineLock {
        BenchmarkLockPolicy::try_from(rules).unwrap().bind(builder(base).build().unwrap()).unwrap()
    }
    fn token_rules() -> Vec<crate::RuleSpec> {
        vec![crate::RuleSpec::Default { action: Action::Tokenize }]
    }
    fn prove(raw: &str, output: &BenchmarkLockedText) {
        let (mut raw_end, mut clean_end) = (0, 0);
        let mut restored = String::new();
        for item in &output.manifest {
            assert!(raw_end <= item.raw_span.start);
            assert_eq!(&raw[raw_end..item.raw_span.start], &output.text[clean_end..item.clean_span.start]);
            restored.push_str(&output.text[clean_end..item.clean_span.start]);
            let value = output.session.restore_strict(&output.text[item.clean_span.clone()]).unwrap();
            assert_eq!(value, raw[item.raw_span.clone()]);
            restored.push_str(&value);
            raw_end = item.raw_span.end;
            clean_end = item.clean_span.end;
        }
        assert_eq!(&raw[raw_end..], &output.text[clean_end..]);
        restored.push_str(&output.text[clean_end..]);
        assert_eq!(restored, raw);
    }

    #[test]
    fn public_provider_all_labels_routing_and_actual_output() {
        let labels = ["GIVEN_NAME", "SURNAME", "ORG", "STREET_NAME", "BUILDING_NUMBER", "SECONDARY_ADDRESS",
            "CITY", "STATE", "EMAIL", "ZIP_CODE", "PHONE", "URL", "CREDIT_CARD", "SSN", "PASSPORT",
            "DRIVERS_LICENSE", "TAX_ID", "BANK_ACCOUNT", "ROUTING_NUMBER", "GOVERNMENT_ID", "IMEI", "IP_ADDRESS"];
        let raw = vec!["xy"; labels.len()].join(" ");
        let spans = labels.iter().enumerate().map(|(i, label)| span(i * 3, i * 3 + 2, label)).collect::<Vec<_>>();
        let (_dir, detector) = bridge(reply(json!(spans)));
        let pipeline = closed(token_rules(), vec![]);
        let output = pipeline.clean_redact_text(&raw, [1,2,3,4], &[], &detector).unwrap();
        assert_eq!(output.manifest.len(), 22);
        assert_eq!(output.dispositions, vec![Disposition::Admitted; 22]);
        for (item, label) in output.manifest.iter().zip(labels) {
            assert_eq!(item.class, label_class(label).unwrap());
        }
        let detections = detector.try_detect(&raw).unwrap();
        validate_redact_detections(&raw, &detections).unwrap();
        for detection in detections {
            let converted = candidate_from_legacy_detection(detection.clone());
            assert_eq!(converted.score, 1.0);
            assert_eq!(converted.priority, 0);
            assert_eq!(converted.canonical_form, None);
            assert_eq!(converted.token_family, "counter");
            assert_eq!(converted.source, detection.source);
            assert_eq!(converted.recognizer_id, detection.source);
            assert_eq!(converted.decided_by, ConflictTier::None);
            assert!(converted.merged_sources.is_empty());
        }
        prove(&raw, &output);
    }

    #[test]
    fn public_provider_preserves_raw_baseline_and_joiner_gaps() {
        let raw = "\u{200d}ａｂ\u{200d}cd xy\u{200d}";
        let baseline = vec![candidate(0..4, false)];
        let (_dir, empty) = bridge(reply(json!([])));
        let pipeline = closed(token_rules(), baseline);
        let control = pipeline.clean_redact_text(raw, [1,2,3,4], &[], &empty).unwrap();
        let (dir2, detector) = bridge(reply(json!([span(0, 2, "GIVEN_NAME"), span(5, 7, "ORG")])));
        let output = pipeline.clean_redact_text(raw, [1,2,3,4], &[], &detector).unwrap();
        assert_eq!(output.dispositions, [Disposition::BaselineOverlap, Disposition::Admitted]);
        for baseline in &control.manifest {
            for byte in baseline.raw_span.clone() {
                assert!(output.manifest.iter().any(|item| item.raw_span.contains(&byte)));
            }
        }
        prove(raw, &control);
        prove(raw, &output);
        let calls = std::fs::read_to_string(dir2.path().join("calls.jsonl")).unwrap();
        assert_eq!(calls.lines().count(), 1);
        assert_eq!(serde_json::from_str::<Value>(calls.trim()).unwrap()["text"], "abcd xy");
    }

    struct ContextBaseline(Arc<AtomicUsize>);
    impl Recognizer for ContextBaseline {
        fn id(&self) -> &str { "synthetic.context" }
        fn token_family(&self) -> &str { "counter" }
        fn supported_class(&self) -> &PiiClass { &PiiClass::Name }
        fn locale_basis(&self) -> gaze_types::LocaleBasis { gaze_types::LocaleBasis::Format }
        fn detect(&self, text: &str, ctx: &DetectContext<'_>) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
            assert!(text.is_empty());
            assert_eq!(ctx.locale_chain, [crate::LocaleTag::DeCh, crate::LocaleTag::EnUs, crate::LocaleTag::DeCh]);
            assert!(ctx.dictionaries.stats().is_empty());
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(vec![])
        }
    }
    #[test]
    fn empty_normalized_empty_and_all_special_still_call_checked_provider_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let pipeline = BenchmarkLockPolicy::try_from(vec![]).unwrap().bind(
            Pipeline::builder().recognizer(ContextBaseline(calls.clone())).build().unwrap()).unwrap();
        let mut response = reply(json!([]));
        response["content_tokens"] = json!(0);
        response["planned_windows"] = json!(0);
        response["completed_windows"] = json!(0);
        response["dispositions"]["special_tokens"] = json!(2);
        let (dir, detector) = bridge(response);
        for raw in ["", "\u{200c}\u{200d}"] {
            let output = pipeline.clean_redact_text(raw, [1,2,3,4], &[crate::LocaleTag::DeCh, crate::LocaleTag::EnUs, crate::LocaleTag::DeCh], &detector).unwrap();
            assert_eq!(output.text, raw);
            assert!(output.session.snapshot_entries().is_empty());
            prove(raw, &output);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let requests = std::fs::read_to_string(dir.path().join("calls.jsonl")).unwrap();
        let requests = requests.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).collect::<Vec<_>>();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| request["text"] == ""));
        assert_eq!(requests[0]["id"], 1);
        assert_eq!(requests[1]["id"], 2);
    }

    #[test]
    fn malformed_later_protocol_values_refuse_before_logging_or_success() {
        let valid = reply(json!([span(0, 1, "GIVEN_NAME"), span(2, 4, "EMAIL")]));
        let mut cases = Vec::new();
        for (field, value) in [("label", json!("UNKNOWN")), ("score", json!(1.1)), ("score", json!(-0.1)),
            ("score", Value::Null), ("start", json!(0)), ("end", json!(99)), ("start", json!(3))] {
            let mut response = valid.clone(); response["spans"][1][field] = value; cases.push(response);
        }
        for (field, value) in [("complete", json!(false)), ("completed_windows", json!(0)),
            ("planned_windows", json!(2)), ("threshold", json!(0.7)), ("org", json!(false)),
            ("version", json!(2)), ("kind", json!("other")), ("content_tokens", json!(32769)),
            ("error", json!("backend"))] {
            let mut response = valid.clone(); response[field] = value; cases.push(response);
        }
        let mut oversized = valid.clone(); oversized["spans"] = json!(vec![span(0, 1, "EMAIL"); 4097]); cases.push(oversized);
        let mut dispositions = valid; dispositions["dispositions"]["cleanup"] = json!(1000001); cases.push(dispositions);
        for response in cases {
            let expected = if response["error"].is_null() && (response["complete"] == false
                || response["planned_windows"] != 1 || response["completed_windows"] != 1
                || response["content_tokens"] == 32769) { "LOCK_INCOMPLETE" } else { "LOCK_PROVIDER_FAILED" };
            let (_dir, detector) = bridge(response);
            let events = Arc::new(Mutex::new(Vec::new()));
            let pipeline = BenchmarkLockPolicy::try_from(token_rules()).unwrap().bind(
                builder(vec![candidate(0..1, false)]).redaction_logger(OrderedFailLog(events.clone())).build().unwrap()).unwrap();
            error_code(pipeline.clean_redact_text("x é", [1,2,3,4], &[], &detector), expected);
            assert!(events.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn structural_adapter_validates_entire_batch_and_exact_source_class() {
        let valid = Detection::new(0..1, PiiClass::Name, "redact-patched-coreml-v1:given_name");
        for detection in [
            Detection::new(2..3, PiiClass::Name, "redact-patched-coreml-v1:GIVEN_NAME"),
            Detection::new(2..3, PiiClass::Email, "redact-patched-coreml-v1:given_name"),
            Detection::new(2..3, PiiClass::Name, "other:given_name"),
            Detection::new(0..1, PiiClass::Name, "redact-patched-coreml-v1:given_name"),
        ] {
            assert!(validate_redact_detections("x y", &[valid.clone(), detection]).is_err());
        }
    }

    #[test]
    fn consuming_bind_preserves_identity_options_rules_and_restore_method() {
        let (_dir, detector) = bridge(reply(json!([])));
        for restore_audit in [false, true] {
            let mut original = builder(vec![candidate(0..2, false)]).build().unwrap();
            original.restore_boundary_dlp_audit = restore_audit;
            original.optimization_config.skip_class_gating = true;
            let events = Arc::new(Mutex::new(Vec::new()));
            original.redaction_loggers.push(Arc::new(OrderedFailLog(events)));
            let reference = original.clone();
            let locked = BenchmarkLockPolicy::try_from(vec![]).unwrap().bind(original).unwrap();
            assert!(Arc::ptr_eq(&reference.registry, &locked.0.registry));
            assert!(Arc::ptr_eq(&reference.redaction_loggers[0], &locked.0.redaction_loggers[0]));
            assert_eq!(reference.optimization_config, locked.0.optimization_config);
            assert_eq!(restore_audit, locked.0.restore_boundary_dlp_audit);
            assert_eq!(locked.0.action_for(&Detection::new(0..2, PiiClass::Name, "synthetic"), &build_context(None)), Action::Preserve);
            let session = Session::new(crate::Scope::Ephemeral).unwrap();
            let expected = reference.restore_with_telemetry(&session, "xy").unwrap();
            let actual = locked.restore_with_telemetry(&session, "xy").unwrap();
            assert_eq!(expected.0.text, actual.0.text);
            assert_eq!(expected.1.phase_execution_mask, actual.1.phase_execution_mask);
            // Even Preserve logs through the original logger and returns no partial output on failure.
            assert!(matches!(locked.clean_redact_text("xy", [1,2,3,4], &[], &detector), Err(Error::RedactionLog(_))));
        }
        let pipeline = closed(vec![crate::RuleSpec::Default { action: Action::Preserve }, crate::RuleSpec::Class { class: PiiClass::Name, action: Action::Tokenize }], vec![candidate(0..2, false)]);
        let output = pipeline.clean_redact_text("xy", [1,2,3,4], &[], &detector).unwrap();
        assert_eq!(output.text, "xy"); assert!(output.manifest.is_empty());
        for rules in [vec![crate::RuleSpec::Default { action: Action::Tokenize }, crate::RuleSpec::Default { action: Action::Generalize }],
            vec![crate::RuleSpec::Default { action: Action::Tokenize }, crate::RuleSpec::Column { column: "synthetic".into(), action: Action::Preserve }]] {
            assert!(BenchmarkLockPolicy::try_from(rules).is_err());
        }
        assert!(BenchmarkLockPolicy::try_from(token_rules()).unwrap().bind(Pipeline::builder().build().unwrap()).is_err());
        assert!(BenchmarkLockPolicy::try_from(token_rules()).unwrap().bind(builder(vec![]).enable_prefix_cache().build().unwrap()).is_err());
        assert!(BenchmarkLockPolicy::try_from(token_rules()).unwrap().bind(builder(vec![]).rule(DefaultRule::new(Action::Tokenize)).build().unwrap()).is_err());
    }

    struct CountLog { calls: Arc<AtomicUsize>, fail_at: usize }
    impl RedactionLogger for CountLog {
        fn log(&self, _: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
            let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if count == self.fail_at { return Err(RedactionLogError::Backend("synthetic stop".into())); }
            Ok(())
        }
    }
    #[test]
    fn public_logger_success_and_failure_during_emission_keep_session_private() {
        let (_dir, detector) = bridge(reply(json!([])));
        for fail_at in [1, 2, usize::MAX] {
            let calls = Arc::new(AtomicUsize::new(0));
            let pipeline = BenchmarkLockPolicy::try_from(token_rules()).unwrap().bind(
                builder(vec![candidate(0..2, false), candidate(3..5, false)])
                    .redaction_logger(CountLog { calls: calls.clone(), fail_at }).build().unwrap()).unwrap();
            let output = pipeline.clean_redact_text("xy xy", [1,2,3,4], &[], &detector);
            if fail_at == usize::MAX {
                let output = output.unwrap(); prove("xy xy", &output);
                assert_eq!(calls.load(Ordering::SeqCst), 2);
                assert_eq!(output.manifest.len(), 2);
            } else {
                assert!(matches!(output, Err(Error::RedactionLog(_))));
                assert_eq!(calls.load(Ordering::SeqCst), fail_at);
            }
        }
    }
    struct FailBaseline;
    impl Recognizer for FailBaseline {
        fn id(&self) -> &str { "synthetic.failure" }
        fn token_family(&self) -> &str { "counter" }
        fn supported_class(&self) -> &PiiClass { &PiiClass::Name }
        fn locale_basis(&self) -> gaze_types::LocaleBasis { gaze_types::LocaleBasis::Format }
        fn detect(&self, _: &str, _: &DetectContext<'_>) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
            Err(gaze_types::DetectError::backend("synthetic.failure", "synthetic failure"))
        }
    }
    #[test]
    fn failed_baseline_never_invokes_provider_and_empty_policy_preserves_all_items() {
        let (dir, detector) = bridge(reply(json!([span(3, 5, "ORG")])));
        let pipeline = BenchmarkLockPolicy::try_from(token_rules()).unwrap().bind(
            Pipeline::builder().recognizer(FailBaseline).build().unwrap()).unwrap();
        assert!(pipeline.clean_redact_text("xy xy", [1,2,3,4], &[], &detector).is_err());
        assert!(!dir.path().join("calls.jsonl").exists());
        let preserve = closed(vec![], vec![candidate(0..2, false)]);
        let output = preserve.clean_redact_text("xy xy", [1,2,3,4], &[], &detector).unwrap();
        assert_eq!(output.text, "xy xy"); assert!(output.manifest.is_empty());
        assert_eq!(output.dispositions, [Disposition::Admitted]);
        assert!(output.session.snapshot_entries().is_empty());
    }
}
