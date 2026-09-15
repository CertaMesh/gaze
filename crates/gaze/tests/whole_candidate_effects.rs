//! Public callback, logger, ownership and failure boundaries for whole recovery.
// Each range is one synthetic detection, not a sequence of byte offsets.
#![allow(clippy::single_range_in_vec_init)]
use gaze::*;
use std::sync::{Arc, Mutex};

const RAW: &str = "password: \"left right\"\nmarker      tail       ";
type Events = Arc<Mutex<Vec<String>>>;

#[derive(Clone)]
struct Fixed {
    id: &'static str,
    class: PiiClass,
    spans: Vec<std::ops::Range<usize>>,
}
impl Recognizer for Fixed {
    fn id(&self) -> &str {
        self.id
    }
    fn supported_class(&self) -> &PiiClass {
        &self.class
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        _: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self
            .spans
            .iter()
            .map(|span| {
                Candidate::new(
                    span.clone(),
                    self.class.clone(),
                    self.id,
                    0.9,
                    0,
                    None,
                    "counter",
                    self.id,
                    ConflictTier::None,
                    vec![],
                )
            })
            .collect())
    }
}
struct ObservingRule {
    session: Arc<Session>,
    events: Events,
    action: Action,
}
impl Rule for ObservingRule {
    fn action(&self, class: &PiiClass, _: &RuleContext) -> Option<Action> {
        self.events.lock().unwrap().push(format!(
            "rule:{}:{}",
            class.to_canonical_str(),
            self.session.tokens().len()
        ));
        Some(self.action)
    }
}
struct ObservingLogger {
    session: Arc<Session>,
    events: Events,
    fail: Option<usize>,
    calls: Mutex<usize>,
}
impl RedactionLogger for ObservingLogger {
    fn log(&self, e: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        self.events.lock().unwrap().push(format!(
            "log:{}:{}:{}",
            e.source,
            e.conflict_loser,
            self.session.tokens().len()
        ));
        let mut calls = self.calls.lock().unwrap();
        let index = *calls;
        *calls += 1;
        if self.fail == Some(index) {
            return Err(RedactionLogError::Backend("synthetic fault".into()));
        }
        Ok(())
    }
}
fn setup(action: Action, fail: Option<usize>) -> (Arc<Session>, Events, Pipeline) {
    setup_class(action, fail, PiiClass::Name)
}
fn setup_class(
    action: Action,
    fail: Option<usize>,
    second_class: PiiClass,
) -> (Arc<Session>, Events, Pipeline) {
    let session = Arc::new(Session::new(Scope::Ephemeral).unwrap());
    let events = Events::default();
    let pipeline = Pipeline::builder()
        .recognizer(Fixed {
            id: "a",
            class: PiiClass::Name,
            spans: vec![11..15],
        })
        .recognizer(Fixed {
            id: "f",
            class: PiiClass::custom("password").unwrap(),
            spans: vec![11..21],
        })
        .recognizer(Fixed {
            id: "b",
            class: PiiClass::Name,
            spans: vec![16..29],
        })
        .recognizer(Fixed {
            id: "d",
            class: second_class,
            spans: vec![35..40],
        })
        .rule(ObservingRule {
            session: session.clone(),
            events: events.clone(),
            action,
        })
        .redaction_logger(ObservingLogger {
            session: session.clone(),
            events: events.clone(),
            fail,
            calls: Mutex::new(0),
        })
        .build()
        .unwrap();
    (session, events, pipeline)
}
fn clean(
    pipeline: &Pipeline,
    session: &Session,
) -> std::result::Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport), Error> {
    pipeline.clean_with_safety_net_policy_detect_context(
        session,
        RawDocument::Text(RAW.into()),
        &[LocaleTag::Global],
        &DictionaryBundle::default(),
        SafetyNetPolicy::default(),
    )
}
fn expected(counts: [usize; 4]) -> Vec<String> {
    let mut out = vec![];
    for ((class, source, loser), count) in [
        ("custom:password", "f", true),
        ("name", "b", false),
        ("name", "d", false),
        ("name", "a", false),
    ]
    .into_iter()
    .zip(counts)
    {
        out.push(format!("rule:{class}:{count}"));
        out.push(format!("log:{source}:{loser}:{count}"));
    }
    out
}
#[test]
fn all_actions_keep_primary_effects_interleaved_before_recovery() {
    for action in [
        Action::Tokenize,
        Action::FormatPreserve,
        Action::Preserve,
        Action::Redact,
        Action::Generalize,
    ] {
        let (session, events, pipeline) = setup(action, None);
        let (clean, spans, _) = clean(&pipeline, &session).unwrap();
        let allocating = matches!(action, Action::Tokenize | Action::FormatPreserve);
        assert_eq!(
            *events.lock().unwrap(),
            expected(if allocating {
                [0, 0, 1, 2]
            } else {
                [0, 0, 0, 0]
            })
        );
        let CleanDocument::Text(text) = clean else {
            panic!("text")
        };
        if allocating {
            assert_eq!(
                spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
                vec![11..15, 16..29, 35..40]
            );
            for span in spans {
                assert_eq!(
                    session.restore(&text[span.clean_span]).unwrap(),
                    &RAW[span.raw_span]
                );
            }
            assert_eq!(session.restore_strict_text(&text).unwrap(), RAW);
            assert_eq!(session.tokens().len(), 3);
        } else if action == Action::Preserve {
            assert_eq!(text, RAW);
            assert!(spans.is_empty());
        }
    }
}
#[test]
fn logger_faults_stop_later_primary_and_recovery_without_live_rollback() {
    for action in [
        Action::Tokenize,
        Action::FormatPreserve,
        Action::Redact,
        Action::Generalize,
        Action::Preserve,
    ] {
        for failure in 0usize..4 {
            let allocating = matches!(action, Action::Tokenize | Action::FormatPreserve);
            let (session, events, pipeline) = setup(action, Some(failure));
            assert!(clean(&pipeline, &session).is_err());
            assert_eq!(
                *events.lock().unwrap(),
                expected(if allocating {
                    [0, 0, 1, 2]
                } else {
                    [0, 0, 0, 0]
                })[..2 * (failure + 1)]
            );
            assert_eq!(
                session.tokens().len(),
                if allocating {
                    failure.saturating_sub(1)
                } else {
                    0
                }
            );
        }
    }
}

#[test]
fn unsupported_trace_actions_stop_before_logger_or_allocation() {
    for action in [Action::Redact, Action::FormatPreserve, Action::Generalize] {
        let (session, events, pipeline) = setup(action, None);
        assert!(matches!(
            pipeline.clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                RAW,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default()
            ),
            Err(Error::UnsupportedActionVariant)
        ));
        assert_eq!(*events.lock().unwrap(), expected([0, 0, 0, 0])[..3]);
        assert!(session.tokens().is_empty());
    }
}
#[test]
fn strict_staged_recovers_both_fragments_and_discard_preserves_live_state() {
    let (session, events, pipeline) = setup(Action::Tokenize, None);
    let mut tx = session.begin_transaction();
    let output = pipeline
        .protect_text_transaction(
            &mut tx,
            RAW,
            ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default()),
        )
        .unwrap();
    assert!(!output.contains("left"));
    assert!(!output.contains("right"));
    assert_eq!(tx.restore_strict_text(&output).unwrap(), RAW);
    assert_eq!(*events.lock().unwrap(), expected([0, 0, 0, 0]));
    drop(tx);
    assert!(session.tokens().is_empty());
    for failure in 0..4 {
        let (session, _, pipeline) = setup(Action::Tokenize, Some(failure));
        let mut tx = session.begin_transaction();
        assert!(pipeline
            .protect_text_transaction(
                &mut tx,
                RAW,
                ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default())
            )
            .is_err());
        drop(tx);
        assert!(session.tokens().is_empty());
    }
}

#[test]
fn allocation_failure_keeps_earlier_mapping_and_skips_recovery() {
    let (session, events, pipeline) =
        setup_class(Action::Tokenize, None, PiiClass::Custom(String::new()));
    assert!(matches!(
        clean(&pipeline, &session),
        Err(Error::EmptyCustomClassName(_))
    ));
    assert_eq!(session.tokens().len(), 1);
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "rule:custom:password:0",
            "log:f:true:0",
            "rule:name:0",
            "log:b:false:0",
            "rule:custom::1",
            "log:d:false:1"
        ]
    );
}

#[test]
fn recovered_trace_names_original_span_and_sources() {
    let (session, _, pipeline) = setup(Action::Tokenize, None);
    let (clean, spans, _, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            RAW,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        vec![11..15, 16..29, 35..40]
    );
    assert_eq!(
        trace
            .iter()
            .map(|t| t.raw_start()..t.raw_end())
            .collect::<Vec<_>>(),
        vec![11..15, 16..29, 35..40]
    );
    assert_eq!(trace[0].source_ids(), &["a"]);
    let CleanDocument::Text(text) = clean else {
        panic!("text")
    };
    assert_eq!(session.restore_strict_text(&text).unwrap(), RAW);
}

#[test]
fn preserve_primary_stays_frozen_while_disjoint_name_recovers() {
    let pipeline = Pipeline::builder()
        .recognizer(Fixed {
            id: "a",
            class: PiiClass::Name,
            spans: vec![11..15],
        })
        .recognizer(Fixed {
            id: "f",
            class: PiiClass::custom("password").unwrap(),
            spans: vec![11..21],
        })
        .recognizer(Fixed {
            id: "b",
            class: PiiClass::Email,
            spans: vec![16..29],
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Preserve))
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (output, spans, _) = clean(&pipeline, &session).unwrap();
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        vec![11..15]
    );
    let CleanDocument::Text(text) = output else {
        panic!("text")
    };
    assert!(text.contains(&RAW[16..29]));
    assert!(!text.contains("left"));
    assert_eq!(session.restore_strict_text(&text).unwrap(), RAW);
}

struct Veto;
impl Recognizer for Veto {
    fn id(&self) -> &str {
        "veto"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Email
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn validator_kind(&self) -> Option<gaze_types::ValidatorKind> {
        Some(gaze_types::ValidatorKind::EmailRfc)
    }
    fn detect(
        &self,
        _: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(vec![Candidate::new(
            0..8,
            PiiClass::Email,
            "veto",
            0.9,
            0,
            None,
            "counter",
            "veto",
            ConflictTier::None,
            vec![],
        )])
    }
}
struct Skipped;
impl Recognizer for Skipped {
    fn id(&self) -> &str {
        "skipped"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn locales(&self) -> &[LocaleTag] {
        &[LocaleTag::DeDe]
    }
    fn detect(
        &self,
        _: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        panic!("inactive locale must not run")
    }
}
#[test]
fn loser_then_veto_then_primary_schedule_excludes_vetoed_and_skipped_originals() {
    let session = Arc::new(Session::new(Scope::Ephemeral).unwrap());
    let events = Events::default();
    let pipeline = Pipeline::builder()
        .recognizer(Veto)
        .recognizer(Skipped)
        .recognizer(Fixed {
            id: "a",
            class: PiiClass::Name,
            spans: vec![11..15],
        })
        .recognizer(Fixed {
            id: "f",
            class: PiiClass::custom("password").unwrap(),
            spans: vec![11..21],
        })
        .recognizer(Fixed {
            id: "b",
            class: PiiClass::Name,
            spans: vec![16..29],
        })
        .rule(ObservingRule {
            session: session.clone(),
            events: events.clone(),
            action: Action::Tokenize,
        })
        .redaction_logger(ObservingLogger {
            session: session.clone(),
            events: events.clone(),
            fail: None,
            calls: Mutex::new(0),
        })
        .build()
        .unwrap();
    let (_, spans, _) = clean(&pipeline, &session).unwrap();
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        vec![11..15, 16..29]
    );
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "rule:custom:password:0",
            "log:f:true:0",
            "rule:email:0",
            "log:veto:true:0",
            "rule:name:0",
            "log:b:false:0",
            "rule:name:1",
            "log:a:false:1"
        ]
    );
}

#[test]
fn inferred_loser_rule_still_runs_without_any_logger() {
    let session = Arc::new(Session::new(Scope::Ephemeral).unwrap());
    let events = Events::default();
    let pipeline = Pipeline::builder()
        .recognizer(Fixed {
            id: "a",
            class: PiiClass::Name,
            spans: vec![11..15],
        })
        .recognizer(Fixed {
            id: "f",
            class: PiiClass::custom("password").unwrap(),
            spans: vec![11..21],
        })
        .recognizer(Fixed {
            id: "b",
            class: PiiClass::Name,
            spans: vec![16..29],
        })
        .rule(ObservingRule {
            session: session.clone(),
            events: events.clone(),
            action: Action::Tokenize,
        })
        .build()
        .unwrap();
    clean(&pipeline, &session).unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec!["rule:custom:password:0", "rule:name:0", "rule:name:1"]
    );
}
