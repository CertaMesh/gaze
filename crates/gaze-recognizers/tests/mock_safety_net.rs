#![cfg(feature = "test-support")]

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, LeakKind, LeakReport,
    LeakSuspect, PiiClass, Pipeline, RawDocument, Scope, Session, Value,
};
use gaze_recognizers::safety_net::test_support::{MockLeakSuspect, MockSafetyNet};
use gaze_types::{LocaleTag, SafetyNetError};

#[derive(Clone)]
struct FixedDetector {
    span: Range<usize>,
    class: PiiClass,
}

impl Detector for FixedDetector {
    fn detect(&self, _input: &str) -> Vec<Detection> {
        vec![Detection::new(
            self.span.clone(),
            self.class.clone(),
            "fixed",
        )]
    }
}

fn session() -> Session {
    Session::new(Scope::Ephemeral).expect("session")
}

fn text(clean: CleanDocument) -> String {
    match clean {
        CleanDocument::Text(text) => text,
        CleanDocument::Structured(_) => panic!("expected text"),
        _ => panic!("expected text"),
    }
}

fn preserve_pipeline(net: MockSafetyNet) -> Pipeline {
    Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(net)
        .build()
        .expect("pipeline")
}

fn tokenizing_pipeline(net: Option<MockSafetyNet>) -> Pipeline {
    let mut builder = Pipeline::builder()
        .detector(FixedDetector {
            span: 0.."alice@example.invalid".len(),
            class: PiiClass::Email,
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve));
    if let Some(net) = net {
        builder = builder.register_safety_net(net);
    }
    builder.build().expect("pipeline")
}

/// Structured documents accept only an observer policy; the policy-less `clean_with_safety_net`
/// defaults to `Resolve`, which the structured entry point refuses. See
/// `Error::UnsupportedSafetyNetModeForStructured`.
fn clean_structured_observing(
    pipeline: &Pipeline,
    session: &gaze::Session,
    raw: RawDocument,
    locales: &[LocaleTag],
) -> gaze::Result<(CleanDocument, Vec<gaze::EmittedTokenSpan>, LeakReport)> {
    pipeline.clean_with_safety_net_policy_detect_context(
        session,
        raw,
        locales,
        &gaze::DictionaryBundle::default(),
        gaze::SafetyNetPolicy::new(gaze::SafetyNetMode::Strict, gaze::SafetyNetFallback::Redact),
    )
}

fn precomputed_email_report(span: Range<usize>) -> LeakReport {
    LeakReport::from_parts(
        vec![LeakSuspect::new(
            span,
            PiiClass::Email,
            "mock-safety-net",
            Some(0.99),
            LeakKind::Uncovered,
            "private_email",
            None,
        )],
        Vec::new(),
    )
}

#[test]
fn precomputed_report_drives_byte_equal_text_invariance() {
    let session = session();
    let raw = RawDocument::Text("alice@example.invalid ok".to_string());
    let baseline = text(
        tokenizing_pipeline(None)
            .redact(&session, raw.clone())
            .expect("baseline"),
    );
    let report = precomputed_email_report(0..5);
    let pipeline = tokenizing_pipeline(Some(MockSafetyNet::new(report.clone())));

    let (clean, _, actual_report) = pipeline
        .clean_with_safety_net(&session, raw, &[LocaleTag::Global])
        .expect("safety-net clean");

    assert_eq!(text(clean), baseline);
    assert_eq!(actual_report.stats, report.stats);
    assert_eq!(actual_report.suspects, report.suspects);
}

#[test]
fn field_path_reports_drive_structured_tests() {
    let mut reports = HashMap::new();
    reports.insert(
        Some("$.user.email".to_string()),
        precomputed_email_report(0.."alice@example.invalid".len()),
    );
    let pipeline = preserve_pipeline(MockSafetyNet::with_reports(reports));
    let session = session();
    let raw = RawDocument::Structured(BTreeMap::from([(
        "user".to_string(),
        Value::Object(BTreeMap::from([
            (
                "email".to_string(),
                Value::String("alice@example.invalid".to_string()),
            ),
            ("name".to_string(), Value::String("Dr. Schmidt".to_string())),
        ])),
    )]));

    let (clean, manifest, report) =
        clean_structured_observing(&pipeline, &session, raw, &[LocaleTag::Global])
            .expect("structured safety-net clean");

    assert!(manifest.is_empty());
    assert_eq!(report.stats.suspect_count, 1);
    assert_eq!(
        report.suspects[0].field_path.as_deref(),
        Some("$.user.email")
    );
    assert!(matches!(clean, CleanDocument::Structured(_)));
}

#[test]
fn raw_suspects_are_correlated_with_manifest_deterministically() {
    let session = session();
    let raw = RawDocument::Text("alice@example.invalid ok".to_string());
    let baseline = text(
        tokenizing_pipeline(None)
            .redact(&session, raw.clone())
            .expect("baseline"),
    );
    let token_end = baseline.find(" ok").expect("token suffix");
    let raw_suspects = HashMap::from([(
        None,
        vec![
            MockLeakSuspect::new(0..token_end, PiiClass::Email, "private_email").with_score(0.91),
            MockLeakSuspect::new(0..token_end, PiiClass::Name, "private_person").with_score(0.82),
        ],
    )]);
    let pipeline = tokenizing_pipeline(Some(MockSafetyNet::with_raw_suspects(raw_suspects)));

    let (clean, _, first) = pipeline
        .clean_with_safety_net(&session, raw.clone(), &[LocaleTag::Global])
        .expect("first safety-net clean");
    let (_, _, second) = pipeline
        .clean_with_safety_net(&session, raw, &[LocaleTag::Global])
        .expect("second safety-net clean");

    assert_eq!(text(clean), baseline);
    assert_eq!(first.suspects, second.suspects);
    assert_eq!(first.stats, second.stats);
    assert_eq!(first.stats.suspect_count, 1);
    assert!(matches!(
        first.suspects[0].kind,
        LeakKind::ClassMismatch { .. }
    ));
}

#[test]
fn locale_skip_still_uses_session_level_locale_chain_for_structured_docs() {
    let mut reports = HashMap::new();
    reports.insert(
        Some("$.email".to_string()),
        precomputed_email_report(0.."alice@example.invalid".len()),
    );
    let net = MockSafetyNet::with_reports(reports).with_locales(vec![LocaleTag::EnUs]);
    let pipeline = preserve_pipeline(net);
    let session = session();
    let raw = RawDocument::Structured(BTreeMap::from([(
        "email".to_string(),
        Value::String("alice@example.invalid".to_string()),
    )]));

    let (_, _, report) = clean_structured_observing(&pipeline, &session, raw, &[LocaleTag::DeDe])
        .expect("structured locale skip");

    assert_eq!(report.stats.locale_skipped_count, 1);
    assert!(report.suspects.is_empty());
}

#[test]
fn typed_mock_errors_fail_closed() {
    let net = MockSafetyNet::new(LeakReport::default()).with_error(SafetyNetError::Runtime {
        message: "mock backend unavailable".to_string(),
    });
    let pipeline = preserve_pipeline(net);
    let err = pipeline
        .clean_with_safety_net(
            &session(),
            RawDocument::Text("hello".to_string()),
            &[LocaleTag::Global],
        )
        .expect_err("mock errors fail closed");

    assert!(matches!(
        err,
        gaze::Error::SafetyNet(SafetyNetError::Runtime { .. })
    ));
}
