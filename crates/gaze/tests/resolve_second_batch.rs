//! Scripted policy-boundary proofs. All values are synthetic; no model evidence.
use gaze::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type Step = (
    String,
    std::result::Result<Vec<LeakSuspect>, SafetyNetError>,
);
struct Script(Arc<Mutex<VecDeque<Step>>>);
impl SafetyNet for Script {
    fn id(&self) -> &str {
        "second.fixture"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        let (expected, result) = self.0.lock().unwrap().pop_front().expect("no extra sweep");
        assert_eq!(
            text, expected,
            "each sweep must inspect its actual phase output"
        );
        result
    }
}
fn raw(span: std::ops::Range<usize>) -> LeakSuspect {
    LeakSuspect::new(
        span,
        PiiClass::Name,
        "second.fixture",
        Some(1.0),
        LeakKind::Uncovered,
        "synthetic",
        Some("field".into()),
    )
}
fn pipeline(steps: Vec<Step>) -> (Pipeline, Arc<Mutex<VecDeque<Step>>>) {
    let steps = Arc::new(Mutex::new(steps.into()));
    (
        Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(Script(steps.clone()))
            .build()
            .unwrap(),
        steps,
    )
}

#[test]
fn second_batch_after_noop_preserves_raw_and_scans_exact_reversible_output() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    // Pre-own the expected replacement to make the exact random-session bytes deterministic.
    let token = session
        .tokenize_with_family("safety_net", &PiiClass::Name, "pré")
        .unwrap();
    let (pipeline, steps) = pipeline(vec![
        ("pré tail".into(), Ok(vec![])),
        ("pré tail".into(), Ok(vec![raw(0..4)])),
        (format!("{token} tail"), Ok(vec![raw(0..token.len())])),
    ]);
    let (doc, manifest, report, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            "pré tail",
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .expect("eligible no-op first resolve must get one complete reversible batch");
    let CleanDocument::Text(text) = doc else {
        panic!("text")
    };
    assert_eq!(text, format!("{token} tail"));
    assert_eq!(session.restore_strict_text(&text).unwrap(), "pré tail");
    assert_eq!(manifest[0].raw_span, 0..4);
    assert_eq!(trace[0].raw_start()..trace[0].raw_end(), 0..4);
    assert_eq!(trace[0].source_ids(), &["second.fixture"]);
    assert_eq!(report.suspects.len(), 1);
    assert_eq!(report.suspects[0].field_path.as_deref(), Some("field"));
    assert!(steps.lock().unwrap().is_empty());
}

fn mismatch(span: std::ops::Range<usize>) -> LeakSuspect {
    let mut s = raw(span);
    s.kind = LeakKind::ClassMismatch {
        pipeline_class: PiiClass::Email,
        safety_net_class: PiiClass::Name,
    };
    s
}
fn run(
    p: &Pipeline,
    s: &Session,
    text: &str,
    policy: SafetyNetPolicy,
) -> gaze::Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)> {
    p.clean_with_safety_net_policy_detect_context(
        s,
        RawDocument::Text(text.into()),
        &[LocaleTag::Global],
        &DictionaryBundle::default(),
        policy,
    )
}
struct Capture {
    rows: Arc<Mutex<Vec<RedactionEntry>>>,
    fail_at: Option<usize>,
}
impl RedactionLogger for Capture {
    fn log(&self, row: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        let mut rows = self.rows.lock().unwrap();
        rows.push(row.clone());
        if self.fail_at == Some(rows.len()) {
            Err(RedactionLogError::Backend("synthetic failure".into()))
        } else {
            Ok(())
        }
    }
}
struct Primary;
impl Detector for Primary {
    fn detect(&self, text: &str) -> Vec<Detection> {
        text.match_indices("primary")
            .map(|(i, _)| Detection::new(i..i + 7, PiiClass::Email, "primary.fixture"))
            .collect()
    }
}

#[test]
fn second_batch_latest_report_deletes_new_interval_and_retains_both_batches_and_primary() {
    for terminal in [0, 1, 2, 3, 4] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let a = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "a")
            .unwrap();
        let b = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "bé")
            .unwrap();
        let tail = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "tail")
            .unwrap();
        let first = "a bé delete primary tail";
        let scan1 = "a bé delete [REDACTED] tail";
        let scan2 = format!("{a} bé delete [REDACTED] tail");
        let scan3 = format!("{a} {b} delete [REDACTED] tail");
        // "delete " is the fallback's residual: it becomes one `[REDACTED:name]` marker, which
        // sits directly against the primary pass's own `[REDACTED]` for "primary".
        let marker = redaction_marker(&PiiClass::Name);
        let final_text = format!("{a} {b} {marker}[REDACTED] tail");
        let terminal_report = match terminal {
            0 => Ok(vec![
                raw(0..a.len()),
                raw(a.len() + 1..a.len() + 1 + b.len()),
            ]),
            1 => Ok(vec![raw(final_text.len() - 4..final_text.len())]),
            2 => Err(SafetyNetError::Runtime {
                message: "terminal fixture".into(),
            }),
            3 => Ok(vec![raw(0..0)]),
            _ => Ok(vec![raw(0..final_text.len() + 1)]),
        };
        let mut sweeps = vec![
            (scan1.into(), Ok(vec![raw(0..1)])),
            (scan2.clone(), Ok(vec![raw(a.len() + 1..a.len() + 4)])),
            (
                scan3.clone(),
                Ok(vec![raw(a.len() + b.len() + 2..a.len() + b.len() + 9)]),
            ),
            (final_text.clone(), terminal_report),
        ];
        // Case 1 is the only terminal report the round can act on: a fresh finding on plain
        // surviving text. It gets one reversible round and one settled sweep.
        let resolved = format!("{a} {b} {marker}[REDACTED] {tail}");
        if terminal == 1 {
            sweeps.push((resolved.clone(), Ok(vec![])));
        }
        let queue = Arc::new(Mutex::new(VecDeque::from(sweeps)));
        let rows = Arc::new(Mutex::new(vec![]));
        let p = Pipeline::builder()
            .detector(Primary)
            .rule(ClassRule::new(PiiClass::Email, Action::Redact))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(Script(queue.clone()))
            .redaction_logger(Capture {
                rows: rows.clone(),
                fail_at: None,
            })
            .build()
            .unwrap();
        let result = run(&p, &session, first, SafetyNetPolicy::default());
        if terminal == 1 {
            let (CleanDocument::Text(text), spans, _) = result.unwrap() else {
                panic!("text")
            };
            assert_eq!(text, resolved);
            assert_eq!(
                spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
                [0..1, 2..5, 6..13, 13..20, 21..25],
                "the terminal round names the tail's ORIGINAL bytes across the redacted range"
            );
            assert_eq!(
                session.restore_strict_text(&text).unwrap(),
                format!("a bé {marker}[REDACTED] tail")
            );
            assert!(queue.lock().unwrap().is_empty());
            assert_eq!(
                rows.lock()
                    .unwrap()
                    .iter()
                    .map(|r| (r.action, r.fallback_triggered.is_some()))
                    .collect::<Vec<_>>(),
                [
                    (Action::Redact, false),
                    (Action::Tokenize, false),
                    (Action::Tokenize, false),
                    (Action::Redact, true),
                    // The terminal round's row carries the reason that made it run, which is
                    // what separates it from the second batch's rows above.
                    (Action::Tokenize, true),
                ]
            );
            continue;
        }
        if terminal == 0 {
            let (CleanDocument::Text(text), spans, report) = result.unwrap() else {
                panic!("text")
            };
            assert_eq!(text, final_text);
            assert_eq!(
                spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
                [0..1, 2..5, 6..13, 13..20]
            );
            // The fallback's marker and the primary pass's own redaction are distinct entries:
            // only the former is the safety net's one-way marker.
            assert!(is_redaction_marker(&text[spans[2].clean_span.clone()]));
            assert_eq!(&text[spans[3].clean_span.clone()], "[REDACTED]");
            assert!(!is_redaction_marker("[REDACTED]"));
            assert!(!session.contains_token("[REDACTED]"));
            assert_eq!(report.suspects.len(), 3);
            assert_eq!(report.suspects[1].span, a.len() + 1..a.len() + 4);
            assert_eq!(
                report.suspects[2].span,
                a.len() + b.len() + 2..a.len() + b.len() + 9
            );
        } else {
            assert!(
                result.is_err(),
                "terminal output must enforce, case {terminal}"
            );
        }
        assert!(queue.lock().unwrap().is_empty());
        assert_eq!(
            rows.lock()
                .unwrap()
                .iter()
                .map(|r| r.action)
                .collect::<Vec<_>>(),
            [
                Action::Redact,
                Action::Tokenize,
                Action::Tokenize,
                Action::Redact
            ]
        );
    }
}

#[test]
fn second_batch_ineligible_and_not_applicable_paths_keep_exact_sweeps() {
    for mode in [
        SafetyNetMode::Strict,
        SafetyNetMode::Tolerant,
        SafetyNetMode::Redact,
        SafetyNetMode::Resolve,
    ] {
        for fallback in [
            SafetyNetFallback::Strict,
            SafetyNetFallback::Tolerant,
            SafetyNetFallback::Redact,
        ] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let steps = if mode == SafetyNetMode::Resolve {
                let mut steps = vec![
                    ("raw".into(), Ok(vec![])),
                    ("raw".into(), Ok(vec![mismatch(0..3)])),
                ];
                if fallback == SafetyNetFallback::Redact {
                    // The terminal sweep sees the marker, not an empty document.
                    steps.push((redaction_marker(&PiiClass::Name), Ok(vec![])));
                }
                steps
            } else {
                vec![("raw".into(), Ok(vec![raw(0..3)]))]
            };
            let (p, steps) = pipeline(steps);
            let result = run(&p, &session, "raw", SafetyNetPolicy::new(mode, fallback));
            if mode == SafetyNetMode::Resolve && fallback == SafetyNetFallback::Strict {
                assert!(matches!(
                    result,
                    Err(Error::SafetyNetFallback(FallbackReason::OverlapConflict))
                ));
            } else {
                let (CleanDocument::Text(text), _, _) = result.unwrap() else {
                    panic!("text")
                };
                assert_eq!(
                    text,
                    if mode == SafetyNetMode::Redact
                        || (mode == SafetyNetMode::Resolve && fallback == SafetyNetFallback::Redact)
                    {
                        redaction_marker(&PiiClass::Name)
                    } else {
                        "raw".to_string()
                    }
                );
            }
            assert!(steps.lock().unwrap().is_empty());
            assert!(session.tokens().is_empty());
        }
    }
    // First refusal has no eligible follow-up, and a scan2 that is only the fallback's marker
    // needs no third sweep: there is nothing left in it that a net could report.
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (p, steps) = pipeline(vec![
        ("raw".into(), Ok(vec![mismatch(0..3)])),
        (redaction_marker(&PiiClass::Name), Ok(vec![])),
    ]);
    run(&p, &session, "raw", SafetyNetPolicy::default()).unwrap();
    assert!(steps.lock().unwrap().is_empty());
    for protected in [false, true] {
        let token = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "raw")
            .unwrap();
        let (p, steps) = pipeline(vec![
            ("raw".into(), Ok(vec![raw(0..3)])),
            (
                token.clone(),
                Ok(if protected {
                    vec![raw(0..token.len())]
                } else {
                    vec![]
                }),
            ),
        ]);
        run(&p, &session, "raw", SafetyNetPolicy::default()).unwrap();
        assert!(steps.lock().unwrap().is_empty());
    }
}

#[test]
fn second_batch_all_item_preflight_cannot_hide_fatal_after_unsupported() {
    let mut invalid_class = raw(0..2);
    invalid_class.class = PiiClass::Custom("!!!".into());
    let mut blank_id = raw(0..2);
    blank_id.safety_net_id = " \t".into();
    let mut false_gap = raw(0..4);
    false_gap.kind = LeakKind::PartialBleed { uncovered: 0..2 };
    let bad = vec![
        invalid_class,
        blank_id,
        raw(0..0),
        raw(std::ops::Range { start: 4, end: 2 }),
        raw(0..99),
        raw(1..2),
        false_gap,
    ];
    for suspect in bad {
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let items = [mismatch(3..4), raw(0..2), suspect.clone()];
            let mixed = order.into_iter().map(|i| items[i].clone()).collect();
            let steps = Arc::new(Mutex::new(VecDeque::from(vec![
                ("é x".into(), Ok(vec![])),
                ("é x".into(), Ok(mixed)),
            ])));
            let rows = Arc::new(Mutex::new(vec![]));
            let p = Pipeline::builder()
                .rule(DefaultRule::new(Action::Preserve))
                .register_safety_net(Script(steps.clone()))
                .redaction_logger(Capture {
                    rows: rows.clone(),
                    fail_at: None,
                })
                .build()
                .unwrap();
            let result = p.clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                "é x",
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            );
            assert!(matches!(
                result,
                Err(Error::SafetyNetSpanInvalid { .. }
                    | Error::SafetyNet(SafetyNetError::InvalidOutput { .. }))
            ));
            assert!(session.tokens().is_empty());
            assert!(rows.lock().unwrap().is_empty());
            assert!(steps.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn second_batch_logger_failure_keeps_live_mappings_and_immediate_audits_in_both_targets() {
    for staged in [false, true] {
        for fail_at in [1, 2] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let steps = Arc::new(Mutex::new(VecDeque::from(vec![
                ("aa bb".into(), Ok(vec![])),
                ("aa bb".into(), Ok(vec![raw(0..2), raw(3..5)])),
            ])));
            let rows = Arc::new(Mutex::new(vec![]));
            let p = Pipeline::builder()
                .rule(DefaultRule::new(Action::Preserve))
                .register_safety_net(Script(steps.clone()))
                .redaction_logger(Capture {
                    rows: rows.clone(),
                    fail_at: Some(fail_at),
                })
                .build()
                .unwrap();
            let mut tx = session.begin_transaction();
            let result = if staged {
                p.clean_transaction_with_safety_net_policy_detect_context(
                    &mut tx,
                    RawDocument::Text("aa bb".into()),
                    &[LocaleTag::Global],
                    &DictionaryBundle::default(),
                    SafetyNetPolicy::default(),
                )
            } else {
                run(&p, &session, "aa bb", SafetyNetPolicy::default())
            };
            assert!(result.is_err());
            assert_eq!(rows.lock().unwrap().len(), fail_at);
            assert!(steps.lock().unwrap().is_empty());
            if staged {
                assert_eq!(
                    tx.tokens().len(),
                    fail_at,
                    "pipeline leaves caller-owned transaction intact"
                );
                assert!(session.tokens().is_empty());
                drop(tx);
                assert!(session.tokens().is_empty());
            } else {
                assert_eq!(session.tokens().len(), fail_at);
            }
            assert_eq!(
                rows.lock().unwrap().len(),
                fail_at,
                "discard cannot roll back external audits"
            );
        }
    }
}

#[test]
fn second_batch_success_then_backend_failure_retains_new_mapping_without_fallback() {
    for staged in [false, true] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let seed = session.tokenize(&PiiClass::Email, "synthetic").unwrap();
        let prefix = seed.split(':').next().unwrap();
        let expected = format!("{prefix}:Name_1>");
        let (p, steps) = pipeline(vec![
            ("raw".into(), Ok(vec![])),
            ("raw".into(), Ok(vec![raw(0..3)])),
            (
                expected.clone(),
                Err(SafetyNetError::Runtime {
                    message: "scan3".into(),
                }),
            ),
        ]);
        let mut tx = session.begin_transaction();
        let result = if staged {
            p.clean_transaction_with_safety_net_policy_detect_context(
                &mut tx,
                RawDocument::Text("raw".into()),
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
        } else {
            run(&p, &session, "raw", SafetyNetPolicy::default())
        };
        assert!(matches!(
            result,
            Err(Error::SafetyNet(SafetyNetError::Runtime { .. }))
        ));
        assert!(steps.lock().unwrap().is_empty());
        if staged {
            assert_eq!(tx.restore(&expected).as_deref(), Some("raw"));
            assert!(!session.contains_token(&expected));
            drop(tx);
            assert_eq!(session.tokens().len(), 1);
        } else {
            assert_eq!(session.restore(&expected).as_deref(), Some("raw"));
        }
    }
}

#[test]
fn five_sweeps_can_mean_ten_backend_calls_and_still_only_one_second_batch() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let a = session
        .tokenize_with_family("safety_net", &PiiClass::Name, "a")
        .unwrap();
    let b = session
        .tokenize_with_family("safety_net", &PiiClass::Name, "b")
        .unwrap();
    let c = session
        .tokenize_with_family("safety_net", &PiiClass::Name, "é")
        .unwrap();
    let marker = redaction_marker(&PiiClass::Name);
    let texts = [
        "a b c é".to_owned(),
        format!("{a} b c é"),
        format!("{a} {b} c é"),
        format!("{a} {b} {marker} é"),
        format!("{a} {b} {marker} {c}"),
    ];
    let active = Arc::new(Mutex::new(VecDeque::from(vec![
        (texts[0].clone(), Ok(vec![raw(0..1)])),
        (texts[1].clone(), Ok(vec![raw(a.len() + 1..a.len() + 2)])),
        (
            texts[2].clone(),
            Ok(vec![raw(a.len() + b.len() + 2..a.len() + b.len() + 3)]),
        ),
        (
            texts[3].clone(),
            Ok(vec![raw(texts[3].len() - 2..texts[3].len())]),
        ),
        (texts[4].clone(), Ok(vec![])),
    ])));
    let passive = Arc::new(Mutex::new(
        texts
            .iter()
            .map(|t| (t.clone(), Ok(vec![])))
            .collect::<VecDeque<_>>(),
    ));
    let p = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(Script(active.clone()))
        .register_safety_net(Script(passive.clone()))
        .build()
        .unwrap();
    let (CleanDocument::Text(text), spans, _) =
        run(&p, &session, &texts[0], SafetyNetPolicy::default()).unwrap()
    else {
        panic!("text")
    };
    assert_eq!(text, texts[4]);
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..1, 2..3, 4..5, 6..8]
    );
    // Restore is exact for everything the fallback did not redact; `c` is gone by the documented
    // `Redact` contract, and its marker survives restore in its place.
    assert_eq!(
        session.restore_strict_text(&text).unwrap(),
        format!("a b {marker} é")
    );
    assert!(active.lock().unwrap().is_empty());
    assert!(passive.lock().unwrap().is_empty());
    assert_eq!(
        session.tokens().len(),
        3,
        "one token per batch: first resolve, second batch, terminal round — and no more"
    );
}

#[test]
fn second_batch_protected_first_noop_and_staged_success_publish_only_on_commit() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    // A deterministic expected token is derived from a separately owned class prefix.
    let primary = session.tokenize(&PiiClass::Email, "primary").unwrap();
    let prefix = primary.split(':').next().unwrap();
    let expected = format!("{primary} {prefix}:Name_1>");
    let initial = format!("{primary} raw");
    let queue = Arc::new(Mutex::new(VecDeque::from(vec![
        (initial.clone(), Ok(vec![raw(0..primary.len())])),
        (initial, Ok(vec![raw(primary.len() + 1..primary.len() + 4)])),
        (expected.clone(), Ok(vec![])),
    ])));
    let p = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .register_safety_net(Script(queue.clone()))
        .build()
        .unwrap();
    let mut tx = session.begin_transaction();
    let (CleanDocument::Text(text), spans, _) = p
        .clean_transaction_with_safety_net_policy_detect_context(
            &mut tx,
            RawDocument::Text("primary raw".into()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap()
    else {
        panic!("text")
    };
    assert_eq!(text, expected);
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..7, 8..11]
    );
    assert_eq!(tx.restore_strict_text(&text).unwrap(), "primary raw");
    assert_eq!(session.tokens().len(), 1);
    assert_eq!(tx.tokens().len(), 2);
    tx.commit().unwrap();
    assert_eq!(session.restore_strict_text(&text).unwrap(), "primary raw");
    assert_eq!(session.tokens().len(), 2);
    assert!(queue.lock().unwrap().is_empty());
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn second_batch_terminal_registry_malformed_spans_are_enforced_before_conversion() {
    use gaze_recognizers::{
        LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
    };
    struct RegistryScript {
        inputs: Arc<Mutex<VecDeque<String>>>,
        bad: usize,
    }
    impl LocaleAwareModel for RegistryScript {
        fn name(&self) -> &str {
            "registry.fixture"
        }
        fn native_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::Global]
        }
        fn infer(
            &self,
            input: ModelInput,
            _: ModelHints,
        ) -> std::result::Result<Vec<ModelSpan>, ModelError> {
            let mut inputs = self.inputs.lock().unwrap();
            assert_eq!(input.text, inputs.pop_front().expect("four sweeps only"));
            if !inputs.is_empty() {
                return Ok(vec![]);
            }
            let end = input.text.len();
            let byte_range = match self.bad {
                0 => 0..0,
                1 => end..end - 1,
                2 => 0..end + 1,
                _ => end - 1..end,
            };
            Ok(vec![ModelSpan {
                text: String::new(),
                byte_range,
                class: PiiClass::Name,
                confidence: None,
                model_name: self.name().into(),
            }])
        }
    }
    for bad in 0..4 {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let a = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "a")
            .unwrap();
        let b = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "b")
            .unwrap();
        let marker = redaction_marker(&PiiClass::Name);
        let texts = [
            "a b c é".to_owned(),
            format!("{a} b c é"),
            format!("{a} {b} c é"),
            format!("{a} {b} {marker} é"),
        ];
        let queue = Arc::new(Mutex::new(VecDeque::from(vec![
            (texts[0].clone(), Ok(vec![raw(0..1)])),
            (texts[1].clone(), Ok(vec![raw(a.len() + 1..a.len() + 2)])),
            (
                texts[2].clone(),
                Ok(vec![raw(a.len() + b.len() + 2..a.len() + b.len() + 3)]),
            ),
            (texts[3].clone(), Ok(vec![])),
        ])));
        let inputs = Arc::new(Mutex::new(VecDeque::from(texts.clone())));
        let p = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(Script(queue.clone()))
            .register_safety_net_registry(LocaleAwareModelRegistry::from_backends(vec![Box::new(
                RegistryScript {
                    inputs: inputs.clone(),
                    bad,
                },
            )]))
            .build()
            .unwrap();
        assert!(matches!(
            run(&p, &session, &texts[0], SafetyNetPolicy::default()),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ));
        assert!(queue.lock().unwrap().is_empty());
        assert!(inputs.lock().unwrap().is_empty());
    }
}

#[test]
fn second_batch_then_fallback_trace_keeps_original_raw_coordinates() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let a = session
        .tokenize_with_family("safety_net", &PiiClass::Name, "a")
        .unwrap();
    let b = session
        .tokenize_with_family("safety_net", &PiiClass::Name, "b")
        .unwrap();
    let marker = redaction_marker(&PiiClass::Name);
    let final_text = format!("{a} {b} {marker}");
    let (p, steps) = pipeline(vec![
        ("a b c".into(), Ok(vec![raw(0..1)])),
        (format!("{a} b c"), Ok(vec![raw(a.len() + 1..a.len() + 2)])),
        (
            format!("{a} {b} c"),
            Ok(vec![raw(a.len() + b.len() + 2..a.len() + b.len() + 3)]),
        ),
        (
            final_text.clone(),
            Ok(vec![raw(a.len() + 1..a.len() + 1 + b.len())]),
        ),
    ]);
    let (CleanDocument::Text(text), spans, report, trace) = p
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            "a b c",
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap()
    else {
        panic!("text")
    };
    assert_eq!(text, final_text);
    assert_eq!(
        session.restore_strict_text(&text).unwrap(),
        format!("a b {marker}")
    );
    // The marker stands for `c`'s ORIGINAL byte, 4..5, exactly where the trace says the
    // fallback redacted.
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..1, 2..3, 4..5]
    );
    assert_eq!(
        trace
            .iter()
            .map(|t| (t.raw_start(), t.raw_end(), t.decision()))
            .collect::<Vec<_>>(),
        [
            (0, 1, "resolve"),
            (2, 3, "resolve"),
            (4, 5, "fallback_redact")
        ]
    );
    assert_eq!(report.stats.suspect_count, 3);
    assert!(steps.lock().unwrap().is_empty());
}
