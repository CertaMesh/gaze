//! Synthetic terminal-admission regressions; no model or evaluation data.
use std::sync::{Arc, Mutex};

use gaze::{
    Action, CleanDocument, DefaultRule, DictionaryBundle, Error, FallbackReason, LeakKind,
    LeakSuspect, LocaleTag, Pipeline, RawDocument, SafetyNet, SafetyNetContext, SafetyNetError,
    SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};

#[derive(Clone, Copy, Debug)]
enum Terminal {
    Raw,
    Token,
    Clear,
    Error,
    ClassMismatch,
    Empty,
    OutOfBounds,
    Reversed,
    SplitUtf8,
}

struct ContextNet {
    terminal: Terminal,
    seen: Arc<Mutex<Vec<String>>>,
}

impl SafetyNet for ContextNet {
    fn id(&self) -> &str {
        "synthetic-context"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.seen.lock().unwrap().push(text.to_owned());
        let mismatch = || LeakKind::ClassMismatch {
            pipeline_class: gaze::PiiClass::Email,
            safety_net_class: gaze::PiiClass::Name,
        };
        let (span, kind) = if let Some(start) = text.find("seed") {
            (start..start + 4, LeakKind::Uncovered)
        } else if let Some(start) = text.find("barrier ") {
            // Resolve has minted a token. Force fallback without deleting that token.
            (start..start + 8, mismatch())
        } else {
            match self.terminal {
                Terminal::Clear => return Ok(vec![]),
                Terminal::Error => {
                    return Err(SafetyNetError::Runtime {
                        message: "synthetic terminal failure".into(),
                    })
                }
                Terminal::Token => (context.manifest.spans[0].clean_span.clone(), mismatch()),
                Terminal::Raw | Terminal::ClassMismatch => {
                    // Gone once the terminal round has tokenized it: the settled sweep sees a
                    // document with nothing left to report.
                    let Some(start) = text.find("residual") else {
                        return Ok(vec![]);
                    };
                    (
                        start..start + 8,
                        if matches!(self.terminal, Terminal::Raw) {
                            LeakKind::Uncovered
                        } else {
                            mismatch()
                        },
                    )
                }
                Terminal::Empty => (text.len()..text.len(), LeakKind::Uncovered),
                Terminal::OutOfBounds => (0..text.len() + 1, LeakKind::Uncovered),
                Terminal::Reversed => {
                    let end = text.len();
                    (end..end - 1, LeakKind::Uncovered)
                }
                Terminal::SplitUtf8 => {
                    let start = text.find('é').unwrap();
                    (start + 1..start + 2, LeakKind::Uncovered)
                }
            }
        };
        Ok(vec![LeakSuspect::new(
            span,
            gaze::PiiClass::Name,
            self.id(),
            None,
            kind,
            "synthetic",
            None,
        )])
    }
}

const RAW: &str = "seed barrier residual é";

fn pipeline(terminal: Terminal) -> (Pipeline, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(ContextNet {
            terminal,
            seen: Arc::clone(&seen),
        })
        .build()
        .unwrap();
    (pipeline, seen)
}

#[derive(Clone, Copy, Debug)]
enum Route {
    Live,
    Staged,
    Trace,
}

fn run(
    pipeline: &Pipeline,
    session: &Session,
    route: Route,
    policy: SafetyNetPolicy,
) -> gaze::Result<(CleanDocument, Vec<gaze::EmittedTokenSpan>, gaze::LeakReport)> {
    let dictionaries = DictionaryBundle::default();
    match route {
        Route::Live => pipeline.clean_with_safety_net_policy_detect_context(
            session,
            RawDocument::Text(RAW.into()),
            &[LocaleTag::Global],
            &dictionaries,
            policy,
        ),
        Route::Trace => pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                session,
                RAW,
                &[LocaleTag::Global],
                &dictionaries,
                policy,
            )
            .map(|(doc, spans, report, trace)| {
                // The terminal round adds a third item on the routes that reach it, and it
                // projects as an ordinary `resolve`/`tokenize` — no new wire key for the
                // benchmark scorer to learn.
                assert_eq!(
                    trace
                        .iter()
                        .map(|item| (
                            item.raw_start(),
                            item.raw_end(),
                            item.stage(),
                            item.decision(),
                            item.action()
                        ))
                        .collect::<Vec<_>>()[..2],
                    [
                        (0, 4, "safety_net", "resolve", "tokenize"),
                        (5, 13, "safety_net", "fallback_redact", "redact"),
                    ]
                );
                (doc, spans, report)
            }),
        Route::Staged => {
            let mut transaction = session.begin_transaction();
            let result = pipeline.clean_transaction_with_safety_net_policy_detect_context(
                &mut transaction,
                RawDocument::Text(RAW.into()),
                &[LocaleTag::Global],
                &dictionaries,
                policy,
            );
            // One token per emitted span on success; on failure, only the `seed` token the
            // first resolve minted before the document was refused.
            // One token per TOKENIZING span. A redaction is a manifest span too now, but a
            // one-way marker mints no token — asked through the shared predicate rather than by
            // re-spelling the marker shape here.
            assert_eq!(
                transaction.tokens().len(),
                result.as_ref().map_or(1, |(doc, spans, _)| {
                    let CleanDocument::Text(text) = doc else {
                        panic!("text");
                    };
                    spans
                        .iter()
                        .filter(|span| {
                            !gaze::is_redaction_marker(&text[span.clean_span.clone()])
                        })
                        .count()
                })
            );
            if let Ok((CleanDocument::Text(text), spans, _)) = &result {
                assert_eq!(
                    transaction
                        .restore(&text[spans[0].clean_span.clone()])
                        .unwrap(),
                    "seed"
                );
            }
            assert!(session.tokens().is_empty());
            drop(transaction);
            assert!(session.tokens().is_empty());
            result
        }
    }
}

/// A raw residual that only the terminal scan reports is no longer a denial: it is a finding on
/// text no earlier pass saw, and the terminal round tokenizes it reversibly. What has not changed
/// is that it never *ships* raw while a round is still available to protect it.
#[test]
fn a_new_raw_residual_after_fallback_is_resolved_on_every_route() {
    for route in [Route::Live, Route::Staged, Route::Trace] {
        let (pipeline, seen) = pipeline(Terminal::Raw);
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (doc, spans, report) = run(&pipeline, &session, route, SafetyNetPolicy::default())
            .unwrap_or_else(|error| panic!("{route:?} denied a resolvable residual: {error:?}"));
        let CleanDocument::Text(text) = doc else {
            panic!("text");
        };
        assert!(
            !text.contains("residual"),
            "{route:?} shipped the terminal finding raw"
        );
        // 5..13 is the fallback redaction. It used to leave no span at all, because the redactor
        // cut the bytes out; it now stands for its own ORIGINAL bytes like every other one-way
        // replacement, which is the same property this assertion always demanded of the residual.
        assert_eq!(
            spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
            [0..4, 5..13, 13..21],
            "{route:?} must name the residual's ORIGINAL bytes, not post-deletion ones"
        );
        assert!(
            report
                .suspects
                .iter()
                .any(|s| s.safety_net_id == "synthetic-context"),
            "{route:?} must surface what the terminal round acted on"
        );
        assert_eq!(
            seen.lock().unwrap().len(),
            4,
            "{route:?}: exactly one extra sweep, after the terminal round"
        );
        if matches!(route, Route::Trace) {
            // Pinned here rather than in `run`, because only a completing document has one.
            assert_eq!(spans[2].raw_span, 13..21);
        }
    }
}

#[test]
fn final_admission_allows_live_token_reflags_without_destructive_fallback() {
    for route in [Route::Live, Route::Staged, Route::Trace] {
        let (pipeline, seen) = pipeline(Terminal::Token);
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (doc, spans, _) = run(&pipeline, &session, route, SafetyNetPolicy::default()).unwrap();
        let CleanDocument::Text(text) = doc else {
            panic!("text");
        };
        // Two spans now: the seed token and the fallback redaction, which records the bytes it
        // replaced instead of removing them without trace.
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].raw_span, 0..4);
        assert_eq!(spans[1].raw_span, 5..13);
        // The marker replaced "barrier " including its trailing space, so it abuts "residual".
        assert!(text.ends_with(&format!(
            "{}residual é",
            gaze::redaction_marker(&gaze::PiiClass::Name)
        )));
        if !matches!(route, Route::Staged) {
            assert_eq!(
                session.restore(&text[spans[0].clean_span.clone()]).unwrap(),
                "seed"
            );
        }
        assert!((2..=3).contains(&seen.lock().unwrap().len()));
    }
}

#[test]
fn final_admission_rejects_terminal_errors_and_malformed_or_mismatched_spans() {
    for terminal in [
        Terminal::Error,
        Terminal::ClassMismatch,
        Terminal::Empty,
        Terminal::OutOfBounds,
        Terminal::Reversed,
        Terminal::SplitUtf8,
    ] {
        for route in [Route::Live, Route::Staged, Route::Trace] {
            let (pipeline, seen) = pipeline(terminal);
            let session = Session::new(Scope::Ephemeral).unwrap();
            let result = run(&pipeline, &session, route, SafetyNetPolicy::default());
            match terminal {
                Terminal::Error => assert!(matches!(
                    result,
                    Err(Error::SafetyNet(SafetyNetError::Runtime { .. }))
                )),
                Terminal::ClassMismatch => assert!(matches!(
                    result,
                    Err(Error::SafetyNetFallback(FallbackReason::OverlapConflict))
                )),
                _ => assert!(
                    matches!(
                        result,
                        Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
                    ),
                    "{terminal:?}/{route:?}: {result:?}"
                ),
            }
            assert_eq!(seen.lock().unwrap().len(), 3);
        }
    }
}

#[test]
fn final_admission_clear_output_retains_default_one_way_fallback_and_trace() {
    for route in [Route::Live, Route::Staged, Route::Trace] {
        let (pipeline, seen) = pipeline(Terminal::Clear);
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (doc, spans, report) =
            run(&pipeline, &session, route, SafetyNetPolicy::default()).unwrap();
        let CleanDocument::Text(text) = doc else {
            panic!("text");
        };
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].raw_span, 5..13);
        // The marker replaced "barrier " including its trailing space, so it abuts "residual".
        assert!(text.ends_with(&format!(
            "{}residual é",
            gaze::redaction_marker(&gaze::PiiClass::Name)
        )));
        // The flagged bytes are gone from the output; a marker stands where they were.
        assert!(!text.contains("barrier"));
        assert_eq!(report.stats.suspect_count, 2);
        assert_eq!(seen.lock().unwrap().len(), 3);
    }
}

#[test]
fn final_admission_does_not_change_tolerant_fallback_or_no_net_contract() {
    let (pipeline, seen) = pipeline(Terminal::Raw);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (doc, _, _) = run(
        &pipeline,
        &session,
        Route::Live,
        SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Tolerant),
    )
    .unwrap();
    let CleanDocument::Text(text) = doc else {
        panic!("text");
    };
    assert!(text.ends_with(" barrier residual é"));
    assert_eq!(seen.lock().unwrap().len(), 2);

    // An empty report with no nets proves no detection completeness.
    let no_net = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap();
    let (doc, spans, report) =
        run(&no_net, &session, Route::Live, SafetyNetPolicy::default()).unwrap();
    assert!(matches!(doc, CleanDocument::Text(text) if text == RAW));
    assert!(spans.is_empty());
    assert!(report.suspects.is_empty());
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn final_admission_rejects_malformed_registry_spans_before_manifest_filtering() {
    use gaze_recognizers::{
        LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
    };

    struct MalformedModel(Terminal);
    impl LocaleAwareModel for MalformedModel {
        fn name(&self) -> &str {
            "synthetic-malformed"
        }
        fn native_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::Global]
        }
        fn infer(&self, input: ModelInput, _: ModelHints) -> Result<Vec<ModelSpan>, ModelError> {
            let text = input.text;
            let spans = if text.contains("seed") {
                // Overlapping raw hits force initial fallback, without a resolve mutation.
                vec![0..4, 1..3]
            } else {
                let end = text.len();
                vec![match self.0 {
                    Terminal::Empty => end..end,
                    Terminal::Reversed => end..end - 1,
                    Terminal::OutOfBounds => 0..end + 1,
                    Terminal::SplitUtf8 => end - 1..end,
                    _ => unreachable!(),
                }]
            };
            Ok(spans
                .into_iter()
                .map(|byte_range| ModelSpan {
                    text: String::new(),
                    byte_range,
                    class: gaze::PiiClass::Name,
                    confidence: None,
                    model_name: self.name().into(),
                })
                .collect())
        }
    }
    for terminal in [
        Terminal::Empty,
        Terminal::Reversed,
        Terminal::OutOfBounds,
        Terminal::SplitUtf8,
    ] {
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .unwrap()
            .with_safety_net_registry(LocaleAwareModelRegistry::from_backends(vec![Box::new(
                MalformedModel(terminal),
            )]));
        let session = Session::new(Scope::Ephemeral).unwrap();
        let result = run(&pipeline, &session, Route::Live, SafetyNetPolicy::default());
        assert!(
            matches!(
                result,
                Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
            ),
            "{terminal:?}: {result:?}"
        );
    }
}

#[test]
fn final_admission_reversible_success_preserves_trace_and_staged_restore() {
    let (pipeline, seen) = pipeline(Terminal::Token);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "seed residual é";
    let dictionaries = DictionaryBundle::default();
    let mut transaction = session.begin_transaction();
    let (staged_doc, staged_spans, staged_report) = pipeline
        .clean_transaction_with_safety_net_policy_detect_context(
            &mut transaction,
            RawDocument::Text(raw.into()),
            &[LocaleTag::Global],
            &dictionaries,
            SafetyNetPolicy::default(),
        )
        .unwrap();
    let CleanDocument::Text(staged_text) = staged_doc else {
        panic!("text");
    };
    assert_eq!(transaction.restore_strict_text(&staged_text).unwrap(), raw);
    assert!(session.tokens().is_empty());
    let (live_doc, live_spans, live_report, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global],
            &dictionaries,
            SafetyNetPolicy::default(),
        )
        .unwrap();
    let CleanDocument::Text(live_text) = live_doc else {
        panic!("text");
    };
    assert_eq!(session.restore_strict_text(&live_text).unwrap(), raw);
    assert_eq!(staged_text, live_text);
    assert_eq!(staged_spans, live_spans);
    assert_eq!(staged_report, live_report);
    assert_eq!(trace.len(), 1);
    assert_eq!(seen.lock().unwrap().len(), 4);
}
