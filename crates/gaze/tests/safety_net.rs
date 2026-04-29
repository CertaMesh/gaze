use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, DocumentKind,
    EmittedTokenSpan, LeakKind, LeakReportTelemetry, LeakSuspect, PiiClass, Pipeline, RawDocument,
    SafetyNet, SafetyNetContext, SafetyNetError, Scope, Session, Value,
};

#[derive(Clone)]
struct FixedDetector {
    span: Range<usize>,
    class: PiiClass,
}

impl Detector for FixedDetector {
    fn detect(&self, _input: &str) -> Vec<Detection> {
        vec![Detection {
            span: self.span.clone(),
            class: self.class.clone(),
            source: "fixed".to_string(),
        }]
    }
}

#[derive(Clone)]
struct MockNet {
    locales: Vec<gaze::LocaleTag>,
    span: Option<Range<usize>>,
    class: PiiClass,
    raw_label: &'static str,
    field_path: Option<&'static str>,
    error_on_field_path: Option<&'static str>,
    error_on_text: bool,
    seen: Arc<Mutex<Vec<SeenCheck>>>,
}

#[derive(Debug, Clone)]
struct SeenCheck {
    clean_text: String,
    field_path: Option<String>,
    document_kind: DocumentKind,
    manifest: Vec<EmittedTokenSpan>,
}

impl MockNet {
    fn new(span: Option<Range<usize>>, class: PiiClass) -> Self {
        Self {
            locales: vec![gaze::LocaleTag::Global],
            span,
            class,
            raw_label: "private_email",
            field_path: None,
            error_on_field_path: None,
            error_on_text: false,
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_locales(mut self, locales: Vec<gaze::LocaleTag>) -> Self {
        self.locales = locales;
        self
    }

    fn with_field_path(mut self, field_path: &'static str) -> Self {
        self.field_path = Some(field_path);
        self
    }

    fn error_on_field_path(mut self, field_path: &'static str) -> Self {
        self.error_on_field_path = Some(field_path);
        self
    }

    fn error_on_text(mut self) -> Self {
        self.error_on_text = true;
        self
    }
}

impl SafetyNet for MockNet {
    fn id(&self) -> &str {
        "mock"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &self.locales
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.seen.lock().unwrap().push(SeenCheck {
            clean_text: clean_text.to_string(),
            field_path: context.field_path.map(str::to_string),
            document_kind: context.document_kind,
            manifest: context.manifest.spans.clone(),
        });

        if (self.error_on_text && context.field_path.is_none())
            || self
                .error_on_field_path
                .is_some_and(|path| Some(path) == context.field_path)
        {
            return Err(SafetyNetError::Runtime {
                message: "field-level failure".to_string(),
            });
        }

        if self.field_path.is_some() && self.field_path != context.field_path {
            return Ok(Vec::new());
        }

        let Some(span) = self.span.clone() else {
            return Ok(Vec::new());
        };
        let Some(kind) = context.manifest.diff_against(&span, &self.class) else {
            return Ok(Vec::new());
        };

        Ok(vec![LeakSuspect {
            span,
            class: self.class.clone(),
            safety_net_id: self.id().to_string(),
            score: Some(0.99),
            kind,
            raw_label: self.raw_label.to_string(),
            field_path: context.field_path.map(str::to_string),
        }])
    }
}

fn session() -> Session {
    Session::new(Scope::Ephemeral).expect("session")
}

fn text(clean: CleanDocument) -> String {
    match clean {
        CleanDocument::Text(text) => text,
        CleanDocument::Structured(_) => panic!("expected text"),
    }
}

fn pipeline_with_net(net: Option<MockNet>) -> Pipeline {
    let mut builder = Pipeline::builder().rule(DefaultRule::new(Action::Preserve));
    if let Some(net) = net {
        builder = builder.register_safety_net(net);
    }
    builder.build().expect("pipeline")
}

fn tokenizing_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(FixedDetector {
            span: 0.."alice@example.invalid".len(),
            class: PiiClass::Email,
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline")
}

fn tokenizing_pipeline_with_net(net: MockNet) -> Pipeline {
    Pipeline::builder()
        .detector(FixedDetector {
            span: 0.."alice@example.invalid".len(),
            class: PiiClass::Email,
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(net)
        .build()
        .expect("pipeline")
}

#[test]
fn clean_with_safety_net_text_path_returns_manifest_and_report() {
    let net = MockNet::new(Some(0..5), PiiClass::Email);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net(
            &session,
            RawDocument::Text("Reach alice@example.invalid".to_string()),
            &[gaze::LocaleTag::Global],
        )
        .expect("safety net clean");

    assert_eq!(text(clean), "Reach alice@example.invalid");
    assert!(manifest.is_empty());
    assert_eq!(report.stats.suspect_count, 1);
    assert_eq!(report.suspects[0].kind, LeakKind::Uncovered);
}

#[test]
fn byte_equal_invariance_for_leak_kinds_and_locale_skip() {
    let session = session();
    let raw = RawDocument::Text("alice@example.invalid ok".to_string());
    let baseline = text(
        tokenizing_pipeline()
            .redact(&session, raw.clone())
            .expect("baseline"),
    );
    let token_len = baseline.find(" ok").expect("token suffix");
    let clean_len = baseline.len();

    let cases = [
        (
            MockNet::new(Some(0..clean_len), PiiClass::Email),
            Some(LeakKind::PartialBleed {
                uncovered: token_len..clean_len,
            }),
        ),
        (
            MockNet::new(Some(0..token_len), PiiClass::Name),
            Some(LeakKind::ClassMismatch {
                pipeline_class: PiiClass::Email,
                safety_net_class: PiiClass::Name,
            }),
        ),
        (
            MockNet::new(Some(token_len + 1..clean_len), PiiClass::Email),
            Some(LeakKind::Uncovered),
        ),
    ];

    for (net, expected_kind) in cases {
        let pipeline = tokenizing_pipeline_with_net(net);
        let (clean, _, report) = pipeline
            .clean_with_safety_net(&session, raw.clone(), &[gaze::LocaleTag::Global])
            .expect("safety net clean");
        assert_eq!(text(clean), baseline);
        assert_eq!(
            report.suspects.first().map(|suspect| &suspect.kind),
            expected_kind.as_ref()
        );
    }

    let skipped =
        MockNet::new(Some(0..9), PiiClass::Email).with_locales(vec![gaze::LocaleTag::EnUs]);
    let (clean, _, report) = tokenizing_pipeline_with_net(skipped)
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::DeDe])
        .expect("locale-skipped safety net clean");
    assert_eq!(text(clean), baseline);
    assert_eq!(report.stats.locale_skipped_count, 1);
    assert!(report.suspects.is_empty());
}

#[test]
fn safety_net_error_fails_closed_after_observing_byte_equal_clean_text() {
    let session = session();
    let raw = RawDocument::Text("alice@example.invalid ok".to_string());
    let baseline = text(
        tokenizing_pipeline()
            .redact(&session, raw.clone())
            .expect("baseline"),
    );
    let net = MockNet::new(Some(0..9), PiiClass::Email).error_on_text();
    let seen = Arc::clone(&net.seen);

    let err = tokenizing_pipeline_with_net(net)
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::Global])
        .expect_err("safety net errors fail closed");

    assert!(matches!(
        err,
        gaze::Error::SafetyNet(SafetyNetError::Runtime { .. })
    ));
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].clean_text, baseline);
    assert_eq!(
        seen[0].manifest[0].clean_span,
        0..baseline.find(" ok").unwrap()
    );
}

#[test]
fn structured_safety_net_traverses_nested_fields_and_preserves_shape() {
    let net =
        MockNet::new(Some(0..21), PiiClass::Email).with_field_path("$.user.contacts[0].email");
    let pipeline = pipeline_with_net(Some(net.clone()));
    let session = session();

    let mut contact = BTreeMap::new();
    contact.insert(
        "email".to_string(),
        Value::String("alice@example.invalid".to_string()),
    );
    let mut user = BTreeMap::new();
    user.insert(
        "contacts".to_string(),
        Value::Array(vec![Value::Object(contact)]),
    );
    user.insert("empty".to_string(), Value::String(String::new()));
    user.insert("active".to_string(), Value::Bool(true));
    user.insert("count".to_string(), Value::I64(7));

    let raw = RawDocument::Structured(BTreeMap::from([("user".to_string(), Value::Object(user))]));

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::Global])
        .expect("structured safety net clean");

    assert!(manifest.is_empty(), "structured manifests stay field-local");
    assert_eq!(report.stats.suspect_count, 1);
    assert_eq!(
        report.suspects[0].field_path.as_deref(),
        Some("$.user.contacts[0].email")
    );
    assert_eq!(report.suspects[0].kind, LeakKind::Uncovered);

    let CleanDocument::Structured(fields) = clean else {
        panic!("expected structured output");
    };
    let Value::Object(user) = &fields["user"] else {
        panic!("expected object");
    };
    assert_eq!(user["active"], Value::Bool(true));
    assert_eq!(user["count"], Value::I64(7));

    let seen = net.seen.lock().unwrap();
    assert!(seen.iter().any(|entry| {
        entry.field_path.as_deref() == Some("$.user.contacts[0].email")
            && entry.document_kind == DocumentKind::Structured
    }));
    assert!(
        !seen
            .iter()
            .any(|entry| entry.field_path.as_deref() == Some("$.user.empty")),
        "empty string fields are no-op skips"
    );
}

#[test]
fn structured_locale_skip_uses_session_level_locale_chain() {
    let net = MockNet::new(Some(0..21), PiiClass::Email).with_locales(vec![gaze::LocaleTag::EnUs]);
    let pipeline = pipeline_with_net(Some(net));
    let session = session();
    let raw = RawDocument::Structured(BTreeMap::from([(
        "email".to_string(),
        Value::String("alice@example.invalid".to_string()),
    )]));

    let (_, _, report) = pipeline
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::DeDe])
        .expect("structured locale skip");

    // For RawDocument::Structured, locale gating uses session-level locale
    // chain across all fields; fields do not carry per-field locale metadata.
    assert_eq!(report.stats.locale_skipped_count, 1);
    assert!(matches!(
        &report.telemetry[0],
        LeakReportTelemetry::LocaleSkipped {
            field_path: Some(path),
            document_kind: DocumentKind::Structured,
            ..
        } if path == "$.email"
    ));
    assert!(report.suspects.is_empty());
}

#[test]
fn structured_field_error_fails_closed_at_doc_level() {
    let net = MockNet::new(None, PiiClass::Email).error_on_field_path("$.profile.email");
    let pipeline = pipeline_with_net(Some(net));
    let session = session();
    let raw = RawDocument::Structured(BTreeMap::from([(
        "profile".to_string(),
        Value::Object(BTreeMap::from([(
            "email".to_string(),
            Value::String("alice@example.invalid".to_string()),
        )])),
    )]));

    let err = pipeline
        .clean_with_safety_net(&session, raw, &[gaze::LocaleTag::Global])
        .expect_err("field-level safety net errors fail closed");

    assert!(matches!(
        err,
        gaze::Error::SafetyNet(SafetyNetError::Runtime { .. })
    ));
}
