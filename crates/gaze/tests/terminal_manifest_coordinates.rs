//! Synthetic deletion ordering proof, independent of models and evaluation data.
use std::sync::{Arc, Mutex};

use gaze::*;

struct OrderingNet(Arc<Mutex<Vec<String>>>);

impl SafetyNet for OrderingNet {
    fn id(&self) -> &str {
        "synthetic-ordering"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        self.0.lock().unwrap().push(text.into());
        let mismatch = LeakKind::ClassMismatch {
            pipeline_class: PiiClass::Email,
            safety_net_class: PiiClass::Name,
        };
        let hits: Vec<_> = if text.contains("seed") {
            text.match_indices("seed")
                .map(|(i, _)| (i..i + 4, LeakKind::Uncovered))
                .collect()
        } else if text.contains("barrier ") {
            text.match_indices("barrier ")
                .map(|(i, _)| (i..i + 8, mismatch.clone()))
                .collect()
        } else {
            context
                .manifest
                .spans
                .iter()
                .map(|span| (span.clean_span.clone(), mismatch.clone()))
                .collect()
        };
        Ok(hits
            .into_iter()
            .map(|(span, kind)| {
                LeakSuspect::new(
                    span,
                    PiiClass::Name,
                    self.id(),
                    None,
                    kind,
                    "synthetic",
                    None,
                )
            })
            .collect())
    }
}

#[derive(Debug, Clone, Copy)]
enum Route {
    Live,
    Staged,
    Trace,
}

fn prove_ordering(route: Route) {
    for raw in [
        "barrier seed residual é",
        "seed barrier seed residual é",
        "barrier seed barrier seed barrier seed residual é",
        "seed barrier residual é",
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(OrderingNet(seen.clone()))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut transaction = session.begin_transaction();
        let dictionaries = DictionaryBundle::default();
        let result = match route {
            Route::Live => pipeline.clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw.into()),
                &[LocaleTag::Global],
                &dictionaries,
                SafetyNetPolicy::default(),
            ),
            Route::Staged => pipeline.clean_transaction_with_safety_net_policy_detect_context(
                &mut transaction,
                RawDocument::Text(raw.into()),
                &[LocaleTag::Global],
                &dictionaries,
                SafetyNetPolicy::default(),
            ),
            Route::Trace => pipeline
                .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                    &session,
                    raw,
                    &[LocaleTag::Global],
                    &dictionaries,
                    SafetyNetPolicy::default(),
                )
                .map(|(doc, spans, report, trace)| {
                    let mut expected: Vec<_> = raw
                        .match_indices("seed")
                        .map(|(i, _)| (i, i + 4, "resolve"))
                        .chain(
                            raw.match_indices("barrier ")
                                .map(|(i, _)| (i, i + 8, "fallback_redact")),
                        )
                        .collect();
                    expected.sort_unstable();
                    assert_eq!(
                        trace
                            .iter()
                            .map(|t| (t.raw_start(), t.raw_end(), t.decision()))
                            .collect::<Vec<_>>(),
                        expected
                    );
                    (doc, spans, report)
                }),
        };
        let (doc, spans, report) =
            result.unwrap_or_else(|err| panic!("{route:?} {raw:?}: {err:?}"));
        let CleanDocument::Text(text) = doc else {
            panic!("text")
        };
        let marker = redaction_marker(&PiiClass::Name);
        // Every `seed` is a token and every `barrier ` is a marker, each standing for its own
        // ORIGINAL bytes -- which is what makes the manifest describe the whole document where
        // deleting left holes nothing described.
        let seeds: Vec<_> = raw.match_indices("seed").map(|(i, _)| i..i + 4).collect();
        let mut expected_raw: Vec<_> = seeds
            .iter()
            .cloned()
            .chain(raw.match_indices("barrier ").map(|(i, _)| i..i + 8))
            .collect();
        expected_raw.sort_by_key(|r| r.start);
        assert_eq!(
            spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
            expected_raw
        );
        assert_eq!(
            report.suspects.len(),
            seeds.len() + raw.matches("barrier ").count()
        );
        assert!(!text.contains("barrier"));
        assert!(text.ends_with("residual é"));
        // Ordered and disjoint. Not necessarily separated: a marker replaces "barrier " with its
        // trailing space, so it can abut the next token directly.
        assert!(spans
            .windows(2)
            .all(|p| p[0].clean_span.end <= p[1].clean_span.start));
        for span in &spans {
            assert_eq!(span.class, PiiClass::Name);
            let replacement = text.get(span.clean_span.clone()).unwrap();
            let restored = if matches!(route, Route::Staged) {
                transaction.restore(replacement)
            } else {
                session.restore(replacement)
            };
            if is_redaction_marker(replacement) {
                // One-way: a marker restores to nothing, whichever route minted the session.
                assert_eq!(restored, None);
            } else {
                assert_eq!(restored.as_deref(), Some(&raw[span.raw_span.clone()]));
            }
        }
        let restored = if matches!(route, Route::Staged) {
            assert!(session.tokens().is_empty());
            transaction.restore_strict_text(&text).unwrap()
        } else {
            session.restore_strict_text(&text).unwrap()
        };
        assert_eq!(restored, raw.replace("barrier ", &marker));
        let scans = seen.lock().unwrap();
        assert_eq!(scans.len(), 3, "exactly one terminal scan, no retry");
        assert_eq!(scans[0], raw);
        assert_eq!(scans[1].replace("barrier ", &marker), text);
        assert_eq!(scans[2], text, "terminal scan must not mutate");
        drop(transaction);
        if matches!(route, Route::Staged) {
            assert!(session.tokens().is_empty());
        }
    }
}

#[test]
fn terminal_coordinates_live() {
    prove_ordering(Route::Live);
}
#[test]
fn terminal_coordinates_staged() {
    prove_ordering(Route::Staged);
}
#[test]
fn terminal_coordinates_trace() {
    prove_ordering(Route::Trace);
}

#[derive(Clone, Copy, Debug)]
enum Rejection {
    Gap,
    Spill,
    AcrossGap,
    ForeignToken,
    Empty,
    Reversed,
    OutOfBounds,
    SplitUtf8,
    Error,
}

struct RejectingNet {
    ordering: OrderingNet,
    rejection: Rejection,
}
impl SafetyNet for RejectingNet {
    fn id(&self) -> &str {
        self.ordering.id()
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        self.ordering.supported_locales()
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        if text.contains("seed") || text.contains("barrier ") {
            return self.ordering.check(text, context);
        }
        self.ordering.0.lock().unwrap().push(text.into());
        let first = &context.manifest.spans[0].clean_span;
        let span = match self.rejection {
            Rejection::Gap => {
                let Some(i) = text.find("gap") else {
                    return Ok(vec![]);
                };
                i..i + 3
            }
            Rejection::Spill => first.start..first.end + 1,
            Rejection::AcrossGap => first.start..context.manifest.spans[1].clean_span.end,
            Rejection::ForeignToken => {
                let i = text.rfind('<').unwrap();
                i..text[i..].find('>').unwrap() + i + 1
            }
            Rejection::Empty => first.start..first.start,
            Rejection::Reversed => first.end..first.start,
            Rejection::OutOfBounds => first.start..text.len() + 1,
            Rejection::SplitUtf8 => {
                let i = text.find('é').unwrap();
                i + 1..i + 2
            }
            Rejection::Error => {
                return Err(SafetyNetError::Runtime {
                    message: "synthetic terminal error".into(),
                })
            }
        };
        Ok(vec![LeakSuspect::new(
            span,
            PiiClass::Name,
            self.id(),
            None,
            LeakKind::Uncovered,
            "synthetic",
            None,
        )])
    }
}

/// A raw gap the terminal scan reports is the one shape the deletion *does* now authorize acting
/// on — reversibly. It is plain surviving text that touches no token and contains no seam, so the
/// terminal round tokenizes it and the document completes with its original bytes named.
#[test]
fn deletion_authorizes_one_reversible_round_over_a_raw_gap() {
    for route in [Route::Live, Route::Staged, Route::Trace] {
        let foreign = Session::new(Scope::Ephemeral)
            .unwrap()
            .tokenize(&PiiClass::Name, "synthetic")
            .unwrap();
        let raw = format!("barrier seed gap seed residual {foreign} é");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(RejectingNet {
                ordering: OrderingNet(seen.clone()),
                rejection: Rejection::Gap,
            })
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut transaction = session.begin_transaction();
        let dictionaries = DictionaryBundle::default();
        let result = match route {
            Route::Live => pipeline.clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw.clone()),
                &[LocaleTag::Global],
                &dictionaries,
                SafetyNetPolicy::default(),
            ),
            Route::Staged => pipeline.clean_transaction_with_safety_net_policy_detect_context(
                &mut transaction,
                RawDocument::Text(raw.clone()),
                &[LocaleTag::Global],
                &dictionaries,
                SafetyNetPolicy::default(),
            ),
            Route::Trace => pipeline
                .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                    &session,
                    &raw,
                    &[LocaleTag::Global],
                    &dictionaries,
                    SafetyNetPolicy::default(),
                )
                .map(|(doc, spans, report, _)| (doc, spans, report)),
        };
        let (CleanDocument::Text(text), spans, _) =
            result.unwrap_or_else(|error| panic!("{route:?}: {error:?}"))
        else {
            panic!("text")
        };
        assert!(!text.contains("gap"), "{route:?} shipped the gap raw");
        // 0..8 is the redacted "barrier " -- a marker standing for its own original bytes, where
        // deleting used to leave nothing in the manifest to say it had been there.
        assert_eq!(
            spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
            [0..8, 8..12, 13..16, 17..21],
            "{route:?}: the round names the gap's ORIGINAL bytes, past the redacted barrier"
        );
        assert!(is_redaction_marker(&text[spans[0].clean_span.clone()]));
        let target: &dyn Fn(&str) -> Option<String> = &|token| match route {
            Route::Staged => transaction.restore(token),
            _ => session.restore(token),
        };
        assert_eq!(
            target(&text[spans[2].clean_span.clone()]).as_deref(),
            Some("gap")
        );
        assert_eq!(
            seen.lock().unwrap().len(),
            4,
            "{route:?}: one extra sweep, and only one"
        );
        drop(transaction);
    }
}

#[test]
fn deletion_does_not_authorize_spills_foreign_tokens_or_malformed_reports() {
    for rejection in [
        Rejection::Spill,
        Rejection::AcrossGap,
        Rejection::ForeignToken,
        Rejection::Empty,
        Rejection::Reversed,
        Rejection::OutOfBounds,
        Rejection::SplitUtf8,
        Rejection::Error,
    ] {
        for route in [Route::Live, Route::Staged, Route::Trace] {
            let foreign = Session::new(Scope::Ephemeral)
                .unwrap()
                .tokenize(&PiiClass::Name, "synthetic")
                .unwrap();
            let raw = format!("barrier seed gap seed residual {foreign} é");
            let seen = Arc::new(Mutex::new(Vec::new()));
            let pipeline = Pipeline::builder()
                .rule(DefaultRule::new(Action::Preserve))
                .register_safety_net(RejectingNet {
                    ordering: OrderingNet(seen.clone()),
                    rejection,
                })
                .build()
                .unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let mut transaction = session.begin_transaction();
            let dictionaries = DictionaryBundle::default();
            let result = match route {
                Route::Live => pipeline.clean_with_safety_net_policy_detect_context(
                    &session,
                    RawDocument::Text(raw.clone()),
                    &[LocaleTag::Global],
                    &dictionaries,
                    SafetyNetPolicy::default(),
                ),
                Route::Staged => pipeline.clean_transaction_with_safety_net_policy_detect_context(
                    &mut transaction,
                    RawDocument::Text(raw.clone()),
                    &[LocaleTag::Global],
                    &dictionaries,
                    SafetyNetPolicy::default(),
                ),
                Route::Trace => pipeline
                    .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                        &session,
                        &raw,
                        &[LocaleTag::Global],
                        &dictionaries,
                        SafetyNetPolicy::default(),
                    )
                    .map(|(doc, spans, report, _)| (doc, spans, report)),
            };
            if matches!(rejection, Rejection::Error) {
                assert!(
                    matches!(
                        result,
                        Err(Error::SafetyNet(SafetyNetError::Runtime { .. }))
                    ),
                    "{route:?} {rejection:?}: {result:?}"
                );
            } else {
                // Spill and AcrossGap claim `Uncovered` over bytes a live token owns; the rest
                // name no real range at all. Either way the suspect contradicts the document it
                // was computed against, so it cannot be judged — and therefore cannot be
                // resolved, admitted or deleted.
                assert!(
                    matches!(
                        result,
                        Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
                    ),
                    "{route:?} {rejection:?}: {result:?}"
                );
            }
            let scans = seen.lock().unwrap();
            assert_eq!(scans.len(), 3);
            assert_eq!(
                scans[1].replace("barrier ", &redaction_marker(&PiiClass::Name)),
                scans[2]
            );
            if matches!(route, Route::Staged) {
                assert_eq!(transaction.tokens().len(), 1);
                assert!(session.tokens().is_empty());
            } else {
                assert_eq!(session.tokens().len(), 1);
            }
            drop(transaction);
            if matches!(route, Route::Staged) {
                assert!(session.tokens().is_empty());
            }
        }
    }
}
