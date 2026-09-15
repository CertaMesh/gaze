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
        let expected_raw: Vec<_> = raw.match_indices("seed").map(|(i, _)| i..i + 4).collect();
        assert_eq!(
            spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
            expected_raw
        );
        assert_eq!(
            report.suspects.len(),
            expected_raw.len() + raw.matches("barrier ").count()
        );
        assert!(!text.contains("barrier"));
        assert!(text.ends_with(" residual é"));
        assert!(spans
            .windows(2)
            .all(|p| p[0].clean_span.end < p[1].clean_span.start));
        for span in &spans {
            assert_eq!(span.class, PiiClass::Name);
            let token = text.get(span.clean_span.clone()).unwrap();
            let restored = if matches!(route, Route::Staged) {
                transaction.restore(token)
            } else {
                session.restore(token)
            };
            assert_eq!(restored.as_deref(), Some(&raw[span.raw_span.clone()]));
        }
        let restored = if matches!(route, Route::Staged) {
            assert!(session.tokens().is_empty());
            transaction.restore_strict_text(&text).unwrap()
        } else {
            session.restore_strict_text(&text).unwrap()
        };
        assert_eq!(restored, raw.replace("barrier ", ""));
        let scans = seen.lock().unwrap();
        assert_eq!(scans.len(), 3, "exactly one terminal scan, no retry");
        assert_eq!(scans[0], raw);
        assert_eq!(scans[1].replace("barrier ", ""), text);
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
