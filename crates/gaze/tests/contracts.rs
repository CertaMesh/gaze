use std::collections::BTreeMap;
use std::sync::Mutex;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Pipeline, PiiClass, RawDocument,
    RedactionEntry, RedactionLogger, RegexDetector, Scope, Session, Value,
};

#[test]
fn pipeline_is_clone_send_and_sync() {
    fn assert_traits<T: Clone + Send + Sync>() {}
    assert_traits::<Pipeline>();
}

#[test]
fn restore_is_lax_but_restore_strict_fails_closed() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    assert_eq!(session.restore("missing-token"), None);
    assert!(session.restore_strict("missing-token").is_err());
}

#[test]
fn same_session_reuses_token_for_same_raw_value() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline");

    let raw = RawDocument::Structured(BTreeMap::from([
        ("a".to_string(), Value::String("alice@example.com".to_string())),
        ("b".to_string(), Value::String("alice@example.com".to_string())),
    ]));

    let clean = pipeline.redact(&session, raw).expect("redact");
    let CleanDocument::Structured(fields) = clean else {
        panic!("expected structured document");
    };

    assert_eq!(fields["a"], fields["b"]);
}

#[test]
fn raw_document_is_not_serializable() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/raw_document_serialize.rs");
}

#[test]
fn normalization_runs_before_detection() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline");

    let raw = RawDocument::Text("a\u{200D}lice＠example.com".to_string());
    let clean = pipeline.redact(&session, raw).expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    assert_eq!(text, "Email_1");
    assert_eq!(
        session.restore_strict("Email_1").expect("restore"),
        "a\u{200D}lice＠example.com"
    );
}

#[test]
fn longest_span_wins_for_overlaps() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::new("alice@example.com", PiiClass::Name).expect("name detector"))
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Name, Action::Redact))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("reach alice@example.com".to_string()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    assert_eq!(text, "reach [REDACTED]");
}

#[test]
fn first_detector_wins_exact_length_tie() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::new("alice@example.com", PiiClass::Name).expect("name detector"))
        .detector(RegexDetector::new("alice@example.com", PiiClass::Email).expect("email detector"))
        .rule(ClassRule::new(PiiClass::Name, Action::Redact))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("reach alice@example.com".to_string()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    assert_eq!(text, "reach [REDACTED]");
}

#[test]
fn overlap_conflict_logs_losing_detection_without_raw_pii() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let pipeline = Pipeline::builder()
        .detector(
            RegexDetector::with_source("alice@example.com", PiiClass::Name, "name-detector")
                .expect("name detector"),
        )
        .detector(
            RegexDetector::with_source("example.com", PiiClass::Email, "email-detector")
                .expect("email detector"),
        )
        .rule(ClassRule::new(PiiClass::Name, Action::Redact))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("reach alice@example.com".to_string()),
        )
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };
    assert_eq!(text, "reach [REDACTED]");

    let entries = logger.entries();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| !entry.conflict_loser));
    assert!(entries.iter().any(|entry| entry.conflict_loser));
    assert!(entries.iter().all(|entry| entry.field_name.is_none()));
}

#[derive(Clone, Default)]
struct MemoryLogger {
    entries: std::sync::Arc<Mutex<Vec<RedactionEntry>>>,
}

impl MemoryLogger {
    fn entries(&self) -> Vec<RedactionEntry> {
        self.entries.lock().expect("entries lock").clone()
    }
}

impl RedactionLogger for MemoryLogger {
    fn log(&self, entry: &RedactionEntry) -> gaze::Result<()> {
        self.entries.lock().expect("entries lock").push(entry.clone());
        Ok(())
    }
}
