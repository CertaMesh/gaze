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
    fn id(&self) -> &str { "synthetic-context" }
    fn supported_locales(&self) -> &[LocaleTag] { &[LocaleTag::Global] }
    fn check(&self, text: &str, context: SafetyNetContext<'_>) -> Result<Vec<LeakSuspect>, SafetyNetError> {
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
                Terminal::Error => return Err(SafetyNetError::Runtime { message: "synthetic terminal failure".into() }),
                Terminal::Token => (context.manifest.spans[0].clean_span.clone(), mismatch()),
                Terminal::Raw | Terminal::ClassMismatch => {
                    let start = text.find("residual").unwrap();
                    (start..start + 8, if matches!(self.terminal, Terminal::Raw) { LeakKind::Uncovered } else { mismatch() })
                }
                Terminal::Empty => (text.len()..text.len(), LeakKind::Uncovered),
                Terminal::OutOfBounds => (0..text.len() + 1, LeakKind::Uncovered),
                Terminal::Reversed => { let end = text.len(); (end..end - 1, LeakKind::Uncovered) }
                Terminal::SplitUtf8 => { let start = text.find('é').unwrap(); (start + 1..start + 2, LeakKind::Uncovered) }
            }
        };
        Ok(vec![LeakSuspect::new(span, gaze::PiiClass::Name, self.id(), None, kind, "synthetic", None)])
    }
}

const RAW: &str = "seed barrier residual é";

fn pipeline(terminal: Terminal) -> (Pipeline, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(ContextNet { terminal, seen: Arc::clone(&seen) })
        .build().unwrap();
    (pipeline, seen)
}

#[derive(Clone, Copy, Debug)]
enum Route { Live, Staged, Trace }

fn run(pipeline: &Pipeline, session: &Session, route: Route, policy: SafetyNetPolicy) -> gaze::Result<(CleanDocument, Vec<gaze::EmittedTokenSpan>, gaze::LeakReport)> {
    let dictionaries = DictionaryBundle::default();
    match route {
        Route::Live => pipeline.clean_with_safety_net_policy_detect_context(session, RawDocument::Text(RAW.into()), &[LocaleTag::Global], &dictionaries, policy),
        Route::Trace => pipeline.clean_text_with_safety_net_policy_detect_context_and_protection_trace(session, RAW, &[LocaleTag::Global], &dictionaries, policy).map(|(doc, spans, report, trace)| {
            assert_eq!(trace.len(), 2);
            (doc, spans, report)
        }),
        Route::Staged => {
            let mut transaction = session.begin_transaction();
            let result = pipeline.clean_transaction_with_safety_net_policy_detect_context(&mut transaction, RawDocument::Text(RAW.into()), &[LocaleTag::Global], &dictionaries, policy);
            assert_eq!(transaction.tokens().len(), 1);
            assert!(session.tokens().is_empty());
            drop(transaction);
            assert!(session.tokens().is_empty());
            result
        }
    }
}

#[test]
fn final_admission_rejects_new_raw_residual_after_fallback_on_every_route() {
    for route in [Route::Live, Route::Staged, Route::Trace] {
        let (pipeline, seen) = pipeline(Terminal::Raw);
        let session = Session::new(Scope::Ephemeral).unwrap();
        let result = run(&pipeline, &session, route, SafetyNetPolicy::default());
        assert!(matches!(result, Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))), "{route:?} admitted a newly detectable raw residual: {result:?}");
        assert_eq!(seen.lock().unwrap().len(), 3);
    }
}

#[test]
fn final_admission_allows_live_token_reflags_without_destructive_fallback() {
    for route in [Route::Live, Route::Staged, Route::Trace] {
        let (pipeline, seen) = pipeline(Terminal::Token);
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (doc, spans, _) = run(&pipeline, &session, route, SafetyNetPolicy::default()).unwrap();
        let CleanDocument::Text(text) = doc else { panic!("text"); };
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].raw_span, 0..4);
        assert!(text.ends_with(" residual é"));
        if !matches!(route, Route::Staged) {
            assert_eq!(session.restore(&text[spans[0].clean_span.clone()]).unwrap(), "seed");
        }
        assert!((2..=3).contains(&seen.lock().unwrap().len()));
    }
}

#[test]
fn final_admission_rejects_terminal_errors_and_malformed_or_mismatched_spans() {
    for terminal in [Terminal::Error, Terminal::ClassMismatch, Terminal::Empty, Terminal::OutOfBounds, Terminal::Reversed, Terminal::SplitUtf8] {
        for route in [Route::Live, Route::Staged, Route::Trace] {
            let (pipeline, seen) = pipeline(terminal);
            let session = Session::new(Scope::Ephemeral).unwrap();
            let result = run(&pipeline, &session, route, SafetyNetPolicy::default());
            match terminal {
                Terminal::Error => assert!(matches!(result, Err(Error::SafetyNet(SafetyNetError::Runtime { .. })))),
                Terminal::ClassMismatch => assert!(matches!(result, Err(Error::SafetyNetFallback(FallbackReason::OverlapConflict)))),
                _ => assert!(matches!(result, Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect)))),
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
        let (doc, spans, report) = run(&pipeline, &session, route, SafetyNetPolicy::default()).unwrap();
        let CleanDocument::Text(text) = doc else { panic!("text"); };
        assert_eq!(spans.len(), 1);
        assert!(text.ends_with(" residual é"));
        assert!(!text.contains("barrier"));
        assert_eq!(report.stats.suspect_count, 2);
        assert_eq!(seen.lock().unwrap().len(), 3);
    }
}

#[test]
fn final_admission_does_not_change_tolerant_fallback_or_no_net_contract() {
    let (pipeline, seen) = pipeline(Terminal::Raw);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (doc, _, _) = run(&pipeline, &session, Route::Live, SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Tolerant)).unwrap();
    let CleanDocument::Text(text) = doc else { panic!("text"); };
    assert!(text.ends_with(" barrier residual é"));
    assert_eq!(seen.lock().unwrap().len(), 2);

    // An empty report with no nets proves no detection completeness.
    let no_net = Pipeline::builder().rule(DefaultRule::new(Action::Preserve)).build().unwrap();
    let (doc, spans, report) = run(&no_net, &session, Route::Live, SafetyNetPolicy::default()).unwrap();
    assert!(matches!(doc, CleanDocument::Text(text) if text == RAW));
    assert!(spans.is_empty());
    assert!(report.suspects.is_empty());
}
