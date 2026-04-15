use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::Arc;
use std::time::Duration;

use gaze::{
    Action, ClassRule, CleanDocument, ColumnRule, DefaultRule, Pipeline, PiiClass, RawDocument,
    RedactionEntry, RedactionLogger, RegexDetector, Scope, Session, SqliteLogger, Value,
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

#[test]
fn persistent_session_snapshot_roundtrips() {
    let session = Session::new(Scope::Persistent {
        ttl: Duration::from_secs(300),
    })
    .expect("session");
    let token = session
        .tokenize(&PiiClass::Email, "alice@example.com")
        .expect("tokenize");

    let snapshot = session.export().expect("export snapshot");
    let imported = Session::import(snapshot).expect("import snapshot");

    assert_eq!(
        imported.restore_strict(&token).expect("restore"),
        "alice@example.com"
    );
}

#[test]
fn snapshot_import_rejects_tampering() {
    let session = Session::new(Scope::Conversation("msg-42".to_string())).expect("session");
    session
        .tokenize(&PiiClass::Email, "alice@example.com")
        .expect("tokenize");

    let mut bytes = session.export().expect("export snapshot").into_bytes();
    let last = bytes.last_mut().expect("snapshot bytes");
    *last ^= 0x01;

    assert!(Session::import(bytes.into()).is_err());
}

#[test]
fn ephemeral_session_cannot_export() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    assert!(session.export().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_redact_reuses_same_token_across_tasks() {
    let session = Arc::new(
        Session::new(Scope::Conversation("msg-42".to_string())).expect("session"),
    );
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .build()
        .expect("pipeline");

    let left_pipeline = pipeline.clone();
    let left_session = Arc::clone(&session);
    let left = tokio::spawn(async move {
        left_pipeline.redact(
            &left_session,
            RawDocument::Text("alice@example.com".to_string()),
        )
    });

    let right_pipeline = pipeline.clone();
    let right_session = Arc::clone(&session);
    let right = tokio::spawn(async move {
        right_pipeline.redact(
            &right_session,
            RawDocument::Text("alice@example.com".to_string()),
        )
    });

    let left = left.await.expect("left task").expect("left redact");
    let right = right.await.expect("right task").expect("right redact");

    let CleanDocument::Text(left) = left else {
        panic!("expected text clean document");
    };
    let CleanDocument::Text(right) = right else {
        panic!("expected text clean document");
    };

    assert_eq!(left, right);
    assert_eq!(
        session.restore_strict(&left).expect("restore"),
        "alice@example.com"
    );
}

#[test]
fn format_preserve_is_deterministic_and_restorable() {
    let session = Session::new(Scope::Conversation("msg-42".to_string())).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::FormatPreserve))
        .build()
        .expect("pipeline");

    let once = pipeline
        .redact(&session, RawDocument::Text("alice@example.com".to_string()))
        .expect("redact once");
    let twice = pipeline
        .redact(&session, RawDocument::Text("alice@example.com".to_string()))
        .expect("redact twice");

    let CleanDocument::Text(once) = once else {
        panic!("expected text document");
    };
    let CleanDocument::Text(twice) = twice else {
        panic!("expected text document");
    };

    assert_eq!(once, twice);
    assert!(once.contains("@example.test"));
    assert_eq!(
        session.restore_strict(&once).expect("restore"),
        "alice@example.com"
    );
}

#[test]
fn generalize_replaces_with_class_token_without_restore_mapping() {
    let session = Session::new(Scope::Conversation("msg-42".to_string())).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::Generalize))
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(&session, RawDocument::Text("alice@example.com".to_string()))
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    assert_eq!(text, "[EMAIL]");
    assert!(session.restore_strict("[EMAIL]").is_err());
}

#[test]
fn custom_pii_class_normalizes_name() {
    assert_eq!(
        PiiClass::custom(" Order-ID ").as_custom_name(),
        Some("order_id")
    );
}

#[test]
fn column_rule_uses_field_name_context() {
    let session = Session::new(Scope::Conversation("msg-42".to_string())).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ColumnRule::new("primary_email", Action::Redact))
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Structured(BTreeMap::from([
                (
                    "primary_email".to_string(),
                    Value::String("alice@example.com".to_string()),
                ),
                (
                    "secondary_email".to_string(),
                    Value::String("alice@example.com".to_string()),
                ),
            ])),
        )
        .expect("redact");

    let CleanDocument::Structured(fields) = clean else {
        panic!("expected structured document");
    };

    assert_eq!(fields["primary_email"], "[REDACTED]");
    assert_eq!(fields["secondary_email"], "Email_1");
}

#[test]
fn pipeline_builds_without_ner_when_model_dir_absent() {
    // Contract: absent/empty [ner] model_dir must not panic and must not
    // poison the rest of the pipeline. A warn is emitted (not asserted here
    // because tracing subscribers aren't wired in unit tests) and regex
    // detectors still work end to end.
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .with_ner_model_dir(None)
        .expect("build with ner=None")
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline");
    let clean = pipeline
        .redact(&session, RawDocument::Text("alice@example.com".into()))
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert_eq!(text, "Email_1");
}

#[test]
fn pipeline_builder_fails_when_ner_model_dir_missing_on_disk() {
    // Explicit model_dir that doesn't exist must propagate NerLoad, not
    // silently drop NER. This is the fail-closed contract for explicit config.
    let result =
        Pipeline::builder().with_ner_model_dir(Some(std::path::Path::new("/nonexistent/gaze/ner/xyz")));
    match result {
        Ok(_) => panic!("expected NerLoad error"),
        Err(err) => {
            let msg = format!("{err}");
            assert!(msg.contains("ner load error"), "unexpected: {msg}");
        }
    }
}

#[test]
fn sqlite_logger_persists_entries() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let logger = SqliteLogger::new(temp.path()).expect("sqlite logger");

    logger
        .log(&RedactionEntry {
            source: "regex".to_string(),
            class: PiiClass::Email,
            action: Action::Tokenize,
            field_name: Some("email".to_string()),
            document_kind: gaze::DocumentKind::Structured,
            conflict_loser: false,
        })
        .expect("log entry");

    let rows = logger.entries().expect("read entries");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, "regex");
    assert_eq!(rows[0].field_name.as_deref(), Some("email"));
}
