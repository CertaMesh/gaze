#![cfg(feature = "bundled-recognizers")]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use gaze::{
    Action, ClassRule, CleanDocument, ColumnRule, DefaultRule, ExecPolicy, PiiClass, Pipeline,
    RawDocument, RedactionEntry, RedactionLogError, RedactionLogger, Sandbox, SandboxError,
    SandboxPlan, Scope, Session, UntrustedExecRequest, ValidatedExecRequest, Value,
};
use gaze_audit::SqliteLogger;
use gaze_recognizers::NormalizerKind;
use gaze_recognizers::RegexDetector;
use gaze_recognizers::ValidatorKind;
use gaze_types::{AmbiguityReason, CollisionMembership, ValidatorFailReason};
use rusqlite::Connection;

#[test]
fn pipeline_is_clone_send_and_sync() {
    fn assert_traits<T: Clone + Send + Sync>() {}
    assert_traits::<Pipeline>();
}

#[test]
fn sandbox_trait_is_object_safe_send_and_sync() {
    fn assert_traits<T: Send + Sync>() {}
    assert_traits::<Box<dyn Sandbox>>();
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
        (
            "a".to_string(),
            Value::String("alice@example.invalid".to_string()),
        ),
        (
            "b".to_string(),
            Value::String("alice@example.invalid".to_string()),
        ),
    ]));

    let clean = pipeline.redact(&session, raw).expect("redact");
    let CleanDocument::Structured(fields) = clean else {
        panic!("expected structured document");
    };

    assert_eq!(fields["a"], fields["b"]);
}

#[test]
fn distinct_conversation_sessions_produce_distinct_pseudonyms_for_identical_raw_values() {
    let pipeline = email_token_pipeline();
    let a = Session::new(Scope::Conversation("a".to_string())).expect("session a");
    let b = Session::new(Scope::Conversation("b".to_string())).expect("session b");

    let clean_a = redact_text(&pipeline, &a, "x@y.invalid");
    let clean_b = redact_text(&pipeline, &b, "x@y.invalid");

    assert_ne!(
        clean_a, clean_b,
        "two Sessions must never produce the same pseudonym for identical input"
    );
}

#[test]
fn two_ephemeral_sessions_are_independent_namespaces() {
    let pipeline = email_token_pipeline();
    let a = Session::new(Scope::Ephemeral).expect("session a");
    let b = Session::new(Scope::Ephemeral).expect("session b");

    let clean_a = redact_text(&pipeline, &a, "x@y.invalid");
    let clean_b = redact_text(&pipeline, &b, "x@y.invalid");

    assert_ne!(
        clean_a, clean_b,
        "two Ephemeral Sessions must not share counters or value-keyed lookups"
    );
}

fn email_token_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline")
}

fn redact_text(pipeline: &Pipeline, session: &Session, text: &str) -> String {
    let clean = pipeline
        .redact(session, RawDocument::Text(text.to_string()))
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };
    text
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

    let raw = RawDocument::Text("a\u{200D}lice＠example.invalid".to_string());
    let clean = pipeline.redact(&session, raw).expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    assert!(text.starts_with('<') && text.ends_with(":Email_1>"));
    assert_eq!(
        session.restore_strict(&text).expect("restore"),
        "a\u{200D}lice＠example.invalid"
    );
}

#[test]
fn email_tokenization_handles_non_ascii_boundary_suffix() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = email_token_pipeline();

    let text = redact_text(&pipeline, &session, "a@example.invalidø");

    assert!(
        !text.contains("a@example.invalid"),
        "email substring leaked through clean text: {text}"
    );
    assert!(text.starts_with('<') && text.ends_with(":Email_1>ø"));

    let token = text.trim_end_matches('ø');
    assert_eq!(
        session.restore_strict(token).expect("restore token"),
        "a@example.invalid"
    );
}

#[test]
fn longest_span_wins_for_overlaps() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = Pipeline::builder()
        .detector(
            RegexDetector::new("alice@example.invalid", PiiClass::Name).expect("name detector"),
        )
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Name, Action::Redact))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("reach alice@example.invalid".to_string()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    assert!(text.starts_with("reach <"));
    assert!(text.ends_with(":Email_1>"));
}

#[test]
fn first_detector_wins_exact_length_tie() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = Pipeline::builder()
        .detector(
            RegexDetector::new("alice@example.invalid", PiiClass::Name).expect("name detector"),
        )
        .detector(
            RegexDetector::new("alice@example.invalid", PiiClass::Email).expect("email detector"),
        )
        .rule(ClassRule::new(PiiClass::Name, Action::Redact))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("reach alice@example.invalid".to_string()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    assert!(text.starts_with("reach <"));
    assert!(text.ends_with(":Email_1>"));
}

#[test]
fn overlap_conflict_logs_losing_detection_without_raw_pii() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let pipeline = Pipeline::builder()
        .detector(
            RegexDetector::with_source("alice@example.invalid", PiiClass::Name, "name-detector")
                .expect("name detector"),
        )
        .detector(
            RegexDetector::with_source("example.invalid", PiiClass::Email, "email-detector")
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
            RawDocument::Text("reach alice@example.invalid".to_string()),
        )
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };
    assert!(text.starts_with("reach alice@<"));
    assert!(text.ends_with(":Email_1>"));

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
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.entries
            .lock()
            .expect("entries lock")
            .push(entry.clone());
        Ok(())
    }
}

#[test]
fn invalid_luhn_candidate_is_vetoed_and_audited() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let pipeline = Pipeline::builder()
        .recognizer(
            RegexDetector::with_rulepack_fields(
                r"\b(?:\d[ -]?){13,19}\b",
                PiiClass::Custom("credit_card".to_string()),
                "card.structural",
                vec![gaze::LocaleTag::Global],
                0.90,
                50,
                "credit_card",
                None,
                Vec::new(),
                Some(ValidatorKind::Luhn),
                None,
            )
            .expect("card detector"),
        )
        .rule(ClassRule::new(
            PiiClass::Custom("credit_card".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("Card 4111-1111-1111-1112".to_string()),
        )
        .expect("redact");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text document");
    };
    assert_eq!(clean, "Card 4111-1111-1111-1112");

    let entries = logger.entries();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].conflict_loser);
    assert_eq!(entries[0].decided_by, gaze::ConflictTier::ValidatorVeto);
    assert_eq!(
        entries[0].validator_fail_reason,
        Some(ValidatorFailReason::LuhnFailed)
    );
}

#[test]
fn pan_iban_overlap_iban_wins_via_collision_policy() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let pipeline = payment_collision_pipeline(logger.clone());

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("IBAN DE70 8807 9565 3194 9631 87".to_string()),
        )
        .expect("redact");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text document");
    };

    assert!(clean.starts_with("IBAN <"));
    assert!(clean.ends_with(":Custom:iban_1>"));
    assert_eq!(
        session
            .restore_strict(clean.trim_start_matches("IBAN "))
            .unwrap(),
        "DE70 8807 9565 3194 9631 87"
    );

    let entries = logger.entries();
    let winner = entries
        .iter()
        .find(|entry| !entry.conflict_loser)
        .expect("winner");
    let loser = entries
        .iter()
        .find(|entry| entry.conflict_loser)
        .expect("loser");
    assert_eq!(winner.class, PiiClass::Custom("iban".to_string()));
    assert_eq!(winner.decided_by, gaze::ConflictTier::CollisionPolicy);
    assert_eq!(loser.decided_by, gaze::ConflictTier::CollisionPolicy);
    assert_eq!(
        loser.collision_family.as_deref(),
        Some("payment-card-or-iban")
    );
    assert_eq!(loser.collision_variant.as_deref(), Some("pan"));
}

#[test]
fn pan_fails_luhn_never_reaches_family_policy() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let pipeline = payment_collision_pipeline(logger.clone());

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("IBAN DE96 7616 4780 7903 5097 45".to_string()),
        )
        .expect("redact");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text document");
    };

    assert!(clean.starts_with("IBAN <"));
    let entries = logger.entries();
    assert!(entries
        .iter()
        .any(|entry| entry.decided_by == gaze::ConflictTier::ValidatorVeto));
    assert!(entries
        .iter()
        .all(|entry| entry.decided_by != gaze::ConflictTier::CollisionPolicy));
}

#[test]
fn phone_family_no_change_when_imei_absent() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let phone_class = PiiClass::Custom("phone".to_string());
    let pipeline = Pipeline::builder()
        .recognizer(
            RegexDetector::with_source(r"\+1-555-0100", phone_class.clone(), "phone.structural")
                .expect("structural phone"),
        )
        .register_collision(
            "phone.structural",
            CollisionMembership::new("phone-or-imei", "phone", 10, None),
        )
        .recognizer(
            RegexDetector::with_source(r"\+1-555-0100", phone_class.clone(), "phone.national.us")
                .expect("us phone"),
        )
        .register_collision(
            "phone.national.us",
            CollisionMembership::new("phone-or-imei", "phone", 10, None),
        )
        .recognizer(
            RegexDetector::with_source(r"\+1-555-0100", phone_class.clone(), "phone.national.de")
                .expect("de phone"),
        )
        .register_collision(
            "phone.national.de",
            CollisionMembership::new("phone-or-imei", "phone", 10, None),
        )
        .rule(ClassRule::new(phone_class, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(&session, RawDocument::Text("call +1-555-0100".to_string()))
        .expect("redact");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text document");
    };
    assert!(clean.starts_with("call <"));

    let entries = logger.entries();
    assert_eq!(entries.len(), 3);
    assert!(entries
        .iter()
        .all(|entry| entry.decided_by != gaze::ConflictTier::CollisionPolicy));
}

#[test]
fn precedence_tie_emits_family_level_token_and_ambiguity_record() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let alpha_class = PiiClass::Custom("alpha_doc".to_string());
    let beta_class = PiiClass::Custom("beta_doc".to_string());
    let family_class = PiiClass::Custom("family:tenant-document".to_string());
    let pipeline = Pipeline::builder()
        .recognizer(
            RegexDetector::with_source("DOC-[0-9]+", alpha_class.clone(), "doc.alpha")
                .expect("alpha detector"),
        )
        .register_collision(
            "doc.alpha",
            CollisionMembership::new("tenant-document", "alpha", 10, None),
        )
        .recognizer(
            RegexDetector::with_source("DOC-[0-9]+", beta_class.clone(), "doc.beta")
                .expect("beta detector"),
        )
        .register_collision(
            "doc.beta",
            CollisionMembership::new("tenant-document", "beta", 10, None),
        )
        .rule(ClassRule::new(family_class.clone(), Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(&session, RawDocument::Text("case DOC-12345".to_string()))
        .expect("redact");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text document");
    };

    let token = clean.trim_start_matches("case ");
    assert!(token.starts_with('<') && token.contains(":Custom:family:tenant-document_"));
    assert_eq!(session.restore_strict(token).unwrap(), "DOC-12345");

    let entries = logger.entries();
    let winner = entries
        .iter()
        .find(|entry| !entry.conflict_loser)
        .expect("winner");
    assert_eq!(winner.class, family_class);
    assert_eq!(winner.collision_family.as_deref(), Some("tenant-document"));
    assert_eq!(winner.collision_variant, None);
    let ambiguity = winner.ambiguity_record.as_ref().expect("ambiguity record");
    assert_eq!(ambiguity.reason, AmbiguityReason::PrecedenceTie);
    assert_eq!(ambiguity.ambiguity_class, family_class);
    assert_eq!(ambiguity.losing_candidates.len(), 2);
}

#[test]
fn mandatory_anchor_missing_emits_family_token_and_no_anchor_ambiguity() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let pipeline = mandatory_anchor_iban_pipeline(logger.clone());

    let clean = pipeline
        .pseudonymize_with_context(
            &session,
            RawDocument::Text("DE89 3704 0044 0532 0130 00".to_string()),
            &[gaze::LocaleTag::DeDe],
        )
        .expect("redact");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text document");
    };

    assert!(
        clean.contains(":Custom:family:payment-card-or-iban_"),
        "{clean}"
    );
    assert_eq!(
        session.restore_strict(clean.as_str()).unwrap(),
        "DE89 3704 0044 0532 0130 00"
    );

    let entries = logger.entries();
    let winner = entries
        .iter()
        .find(|entry| !entry.conflict_loser)
        .expect("winner");
    assert_eq!(
        winner.class,
        PiiClass::Custom("family:payment-card-or-iban".to_string())
    );
    assert_eq!(winner.decided_by, gaze::ConflictTier::AnchoredContext);
    assert_eq!(
        winner.collision_family.as_deref(),
        Some("payment-card-or-iban")
    );
    let ambiguity = winner.ambiguity_record.as_ref().expect("ambiguity");
    assert_eq!(ambiguity.reason, AmbiguityReason::NoAnchor);
    assert_eq!(
        ambiguity.ambiguity_class,
        PiiClass::Custom("family:payment-card-or-iban".to_string())
    );
    assert_eq!(ambiguity.losing_candidates.len(), 1);
    assert_eq!(
        ambiguity.losing_candidates[0].recognizer_id,
        "iban.structural"
    );
}

#[test]
fn mandatory_anchor_present_keeps_variant_token_without_ambiguity() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let pipeline = mandatory_anchor_iban_pipeline(logger.clone());

    let clean = pipeline
        .pseudonymize_with_context(
            &session,
            RawDocument::Text("IBAN: DE89 3704 0044 0532 0130 00".to_string()),
            &[gaze::LocaleTag::DeDe],
        )
        .expect("redact");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text document");
    };

    assert!(clean.contains(":Custom:iban_"), "{clean}");
    assert!(!clean.contains(":Custom:family:payment-card-or-iban_"));

    let entries = logger.entries();
    let winner = entries
        .iter()
        .find(|entry| !entry.conflict_loser)
        .expect("winner");
    assert_eq!(winner.class, PiiClass::Custom("iban".to_string()));
    assert_eq!(winner.decided_by, gaze::ConflictTier::None);
    assert!(winner.ambiguity_record.is_none());
}

fn mandatory_anchor_iban_pipeline(logger: MemoryLogger) -> Pipeline {
    Pipeline::builder()
        .recognizer(
            RegexDetector::with_rulepack_fields(
                r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7} ?[A-Z0-9]{1,4}\b",
                PiiClass::Custom("iban".to_string()),
                "iban.structural",
                vec![gaze::LocaleTag::Global],
                0.70,
                80,
                "counter",
                None,
                Vec::new(),
                Some(ValidatorKind::IbanMod97),
                Some(NormalizerKind::IbanCanonical),
            )
            .expect("iban detector"),
        )
        .register_collision(
            "iban.structural",
            CollisionMembership::new("payment-card-or-iban", "iban", 10, Some("iban".to_string())),
        )
        .register_anchor_cue_bundle(
            gaze::LocaleTag::DeDe,
            "iban",
            vec!["IBAN:".to_string(), "IBAN".to_string()],
            Some(64),
        )
        .rule(ClassRule::new(
            PiiClass::Custom("iban".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("family:payment-card-or-iban".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(logger)
        .build()
        .expect("pipeline")
}

fn payment_collision_pipeline(logger: MemoryLogger) -> Pipeline {
    Pipeline::builder()
        .recognizer(
            RegexDetector::with_rulepack_fields(
                r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7} ?[A-Z0-9]{1,4}\b",
                PiiClass::Custom("iban".to_string()),
                "iban.structural",
                vec![gaze::LocaleTag::Global],
                0.70,
                80,
                "counter",
                None,
                Vec::new(),
                Some(ValidatorKind::IbanMod97),
                Some(NormalizerKind::IbanCanonical),
            )
            .expect("iban detector"),
        )
        .register_collision(
            "iban.structural",
            CollisionMembership::new("payment-card-or-iban", "iban", 10, None),
        )
        .recognizer(
            RegexDetector::with_rulepack_fields(
                r"\b\d(?:[\s-]?\d){12,18}\b",
                PiiClass::Custom("credit_card".to_string()),
                "card.structural",
                vec![gaze::LocaleTag::Global],
                0.70,
                80,
                "counter",
                None,
                Vec::new(),
                Some(ValidatorKind::Luhn),
                None,
            )
            .expect("card detector"),
        )
        .register_collision(
            "card.structural",
            CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
        )
        .rule(ClassRule::new(
            PiiClass::Custom("iban".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("credit_card".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(logger)
        .build()
        .expect("pipeline")
}

#[test]
fn recognizer_without_validator_flows_without_veto_audit_row() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let logger = MemoryLogger::default();
    let pipeline = Pipeline::builder()
        .recognizer(
            RegexDetector::with_source(
                r"\bDr\. Schmidt\b",
                PiiClass::Custom("name".to_string()),
                "name.custom",
            )
            .expect("name detector"),
        )
        .rule(ClassRule::new(
            PiiClass::Custom("name".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(&session, RawDocument::Text("Owner Dr. Schmidt".to_string()))
        .expect("redact");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text document");
    };
    assert!(clean.starts_with("Owner <"));
    assert!(clean.ends_with(":Custom:name_1>"));

    let entries = logger.entries();
    assert_eq!(entries.len(), 1);
    assert!(entries.iter().all(|entry| {
        entry.decided_by != gaze::ConflictTier::ValidatorVeto && !entry.conflict_loser
    }));
}

struct PrefixSandbox;

impl Sandbox for PrefixSandbox {
    fn prepare(&self, request: &ValidatedExecRequest) -> Result<SandboxPlan, SandboxError> {
        let mut plan = SandboxPlan::passthrough(request);
        plan.args.insert(0, "--sandboxed".to_string());
        Ok(plan)
    }
}

#[test]
fn persistent_session_snapshot_roundtrips() {
    let session = Session::new(Scope::Persistent {
        ttl: Duration::from_secs(300),
    })
    .expect("session");
    let token = session
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .expect("tokenize");

    let snapshot = session.export().expect("export snapshot");
    let imported = Session::import(snapshot).expect("import snapshot");

    assert_eq!(
        imported.restore_strict(&token).expect("restore"),
        "alice@example.invalid"
    );
}

#[test]
fn snapshot_import_rejects_tampering() {
    let session = Session::new(Scope::Conversation("msg-42".to_string())).expect("session");
    session
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .expect("tokenize");

    let mut bytes = session.export().expect("export snapshot").into_bytes();
    let last = bytes.last_mut().expect("snapshot bytes");
    *last ^= 0x01;

    assert!(Session::import(bytes.into()).is_err());
}

#[test]
fn snapshot_import_rejects_emitted_version_tampering() {
    let session = Session::new(Scope::Conversation("msg-42".to_string())).expect("session");
    let token = session
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .expect("tokenize");

    let bytes = session.export().expect("export snapshot").into_bytes();
    let imported = Session::import(bytes.clone().into()).expect("import snapshot");
    assert_eq!(
        imported.restore_strict(&token).expect("restore"),
        "alice@example.invalid"
    );

    let mut tampered = bytes;
    tampered[0] = 3;

    assert!(matches!(
        Session::import(tampered.into()),
        Err(gaze::Error::InvalidSnapshotSignature)
    ));
}

#[test]
fn ephemeral_session_cannot_export() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    assert!(session.export().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_redact_reuses_same_token_across_tasks() {
    let session =
        Arc::new(Session::new(Scope::Conversation("msg-42".to_string())).expect("session"));
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
            RawDocument::Text("alice@example.invalid".to_string()),
        )
    });

    let right_pipeline = pipeline.clone();
    let right_session = Arc::clone(&session);
    let right = tokio::spawn(async move {
        right_pipeline.redact(
            &right_session,
            RawDocument::Text("alice@example.invalid".to_string()),
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
        "alice@example.invalid"
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
        .redact(
            &session,
            RawDocument::Text("alice@example.invalid".to_string()),
        )
        .expect("redact once");
    let twice = pipeline
        .redact(
            &session,
            RawDocument::Text("alice@example.invalid".to_string()),
        )
        .expect("redact twice");

    let CleanDocument::Text(once) = once else {
        panic!("expected text document");
    };
    let CleanDocument::Text(twice) = twice else {
        panic!("expected text document");
    };

    assert_eq!(once, twice);
    assert!(once.contains("@gaze-fake.invalid"));
    assert_eq!(
        session.restore_strict(&once).expect("restore"),
        "alice@example.invalid"
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
        .redact(
            &session,
            RawDocument::Text("alice@example.invalid".to_string()),
        )
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    assert_eq!(text, "[EMAIL]");
    assert!(session.restore_strict("[EMAIL]").is_err());
}

#[test]
fn tokenize_assigns_indices_left_to_right() {
    let session = Session::new(Scope::Conversation("msg-42".to_string())).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .build()
        .expect("pipeline");

    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("first@example.invalid then second@example.invalid".to_string()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };

    let parts = text.split(" then ").collect::<Vec<_>>();
    assert_eq!(parts.len(), 2);
    assert!(parts[0].starts_with('<') && parts[0].ends_with(":Email_1>"));
    assert!(parts[1].starts_with('<') && parts[1].ends_with(":Email_2>"));
    assert_eq!(
        session.restore_strict(parts[0]).expect("restore first"),
        "first@example.invalid"
    );
    assert_eq!(
        session.restore_strict(parts[1]).expect("restore second"),
        "second@example.invalid"
    );
}

#[test]
fn custom_pii_class_normalizes_name() {
    assert_eq!(
        PiiClass::custom(" Class-Alpha ").as_custom_name(),
        Some("class_alpha")
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
                    Value::String("alice@example.invalid".to_string()),
                ),
                (
                    "secondary_email".to_string(),
                    Value::String("alice@example.invalid".to_string()),
                ),
            ])),
        )
        .expect("redact");

    let CleanDocument::Structured(fields) = clean else {
        panic!("expected structured document");
    };

    assert_eq!(fields["primary_email"], "[REDACTED]");
    assert!(fields["secondary_email"]
        .as_str()
        .is_some_and(|value| value.starts_with('<') && value.ends_with(":Email_1>")));
}

#[test]
fn exec_policy_validates_untrusted_input_before_sandbox_prepare() {
    let policy = ExecPolicy::new()
        .allow_program("/usr/local/bin/gaze-hook")
        .allow_env("MAIL_FROM");
    let request = UntrustedExecRequest {
        program: "/usr/local/bin/gaze-hook".into(),
        args: vec![
            "send-email".to_string(),
            format!("<{}>", ["Email", "1"].join("_")),
        ],
        env: BTreeMap::from([("MAIL_FROM".to_string(), "bot@example.invalid".to_string())]),
        cwd: Some("/tmp".into()),
    };

    let validated = request.validate(&policy).expect("validated");
    let plan = PrefixSandbox.prepare(&validated).expect("sandbox plan");

    assert_eq!(
        plan.program,
        std::path::Path::new("/usr/local/bin/gaze-hook")
    );
    assert_eq!(plan.args[0], "--sandboxed");
    assert_eq!(plan.args[1], "send-email");
}

#[test]
fn exec_policy_rejects_non_allowlisted_env_keys() {
    let policy = ExecPolicy::new().allow_program("/usr/local/bin/gaze-hook");
    let request = UntrustedExecRequest {
        program: "/usr/local/bin/gaze-hook".into(),
        args: vec!["send-email".to_string()],
        env: BTreeMap::from([("LD_PRELOAD".to_string(), "/tmp/hook.dylib".to_string())]),
        cwd: None,
    };

    assert_eq!(
        request.validate(&policy),
        Err(SandboxError::EnvNotAllowed("LD_PRELOAD".to_string()))
    );
}

#[test]
fn exec_policy_rejects_shell_metacharacters_in_argv() {
    let policy = ExecPolicy::new().allow_program("/usr/local/bin/gaze-hook");
    let request = UntrustedExecRequest {
        program: "/usr/local/bin/gaze-hook".into(),
        args: vec!["send-email;cat".to_string()],
        env: BTreeMap::new(),
        cwd: None,
    };

    assert_eq!(request.validate(&policy), Err(SandboxError::UnsafeArgv));
}

#[test]
fn sqlite_logger_persists_entries() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let logger = SqliteLogger::new(temp.path()).expect("sqlite logger");

    logger
        .log(&RedactionEntry::new(
            "regex",
            PiiClass::Email,
            Action::Tokenize,
            Some("email".to_string()),
            gaze::DocumentKind::Structured,
            false,
            gaze::ConflictTier::None,
            0,
            None,
        ))
        .expect("log entry");

    let rows = logger.entries().expect("read entries");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, "regex");
    assert_eq!(rows[0].field_name.as_deref(), Some("email"));
}

#[test]
fn sqlite_logger_migrates_legacy_tables_and_purges_by_created_at() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    {
        let conn = Connection::open(temp.path()).expect("legacy sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE redaction_log (
                source TEXT NOT NULL,
                class TEXT NOT NULL,
                action TEXT NOT NULL,
                field_name TEXT NULL,
                document_kind TEXT NOT NULL,
                conflict_loser INTEGER NOT NULL,
                decided_by TEXT NOT NULL DEFAULT 'none'
            );
            "#,
        )
        .expect("legacy schema");
    }

    let logger = SqliteLogger::new(temp.path()).expect("migrated sqlite logger");
    logger
        .log(&RedactionEntry::new(
            "regex",
            PiiClass::Email,
            Action::Tokenize,
            None,
            gaze::DocumentKind::Text,
            false,
            gaze::ConflictTier::None,
            1_767_225_600_000,
            None,
        ))
        .expect("log entry");

    assert_eq!(logger.count_before(4_102_444_800_000).unwrap(), 1);
    assert_eq!(logger.purge_before(4_102_444_800_000).unwrap(), 1);
    assert_eq!(logger.entries().unwrap().len(), 0);
}
