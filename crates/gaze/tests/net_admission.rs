use gaze::*;
use std::sync::{Arc, Mutex};

const EMAIL: &str = "alice@example.invalid";
struct Net {
    calls: Arc<Mutex<Vec<Manifest>>>,
    residual: bool,
}
impl SafetyNet for Net {
    fn id(&self) -> &str {
        "admission-test"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::EnUs]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        assert_eq!(context.locale_chain, &[LocaleTag::EnUs]);
        assert!(context.dictionaries.unwrap().get("tenant").is_some());
        self.calls.lock().unwrap().push(context.manifest.clone());
        if self.residual {
            return Ok(vec![LeakSuspect::new(
                0..text.len(),
                PiiClass::Email,
                self.id(),
                None,
                LeakKind::Uncovered,
                "synthetic",
                None,
            )]);
        }
        Ok(context
            .manifest
            .spans
            .iter()
            .map(|span| {
                LeakSuspect::new(
                    span.clean_span.clone(),
                    PiiClass::Name,
                    self.id(),
                    None,
                    LeakKind::ClassMismatch {
                        pipeline_class: PiiClass::Email,
                        safety_net_class: PiiClass::Name,
                    },
                    "synthetic",
                    None,
                )
            })
            .collect())
    }
}
fn dictionaries() -> DictionaryBundle {
    DictionaryBundle::from_rulepack_terms(&[RulepackDict::new(
        "tenant",
        vec!["synthetic".into()],
        true,
    )])
}
fn configured(residual: bool) -> (Pipeline, Arc<Mutex<Vec<Manifest>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .register_safety_net(Net {
            calls: calls.clone(),
            residual,
        })
        .enable_capitals_heuristic_gate()
        .build()
        .unwrap();
    (pipeline, calls)
}
#[test]
fn admission_uses_exact_owned_restore_ranges_and_expanded_coordinates() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    let token = tx.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let fake = tx
        .format_preserving_fake(&PiiClass::Email, "bob@example.invalid")
        .unwrap();
    let text = format!("é {token} {fake} {token}");
    let (pipeline, calls) = configured(false);
    pipeline
        .admit_safety_nets_transaction(&mut tx, &text, &[LocaleTag::EnUs], &dictionaries())
        .unwrap();
    assert!(session.tokens().is_empty());
    let manifests = calls.lock().unwrap();
    let manifest = &manifests[0];
    assert_eq!(manifest.spans.len(), 3);
    assert_eq!(manifest.spans[0].raw_span, 3..3 + EMAIL.len());
    for span in &manifest.spans {
        assert!(tx.restore(&text[span.clean_span.clone()]).is_some());
    }
    drop(manifests);
    tx.commit().unwrap();
    pipeline
        .admit_safety_nets(&session, &text, &[LocaleTag::EnUs], &dictionaries())
        .unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(calls[0], calls[1]);
}
#[test]
fn unknown_token_shapes_and_raw_gaps_have_no_coverage() {
    let (pipeline, _) = configured(true);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let foreign = Session::new(Scope::Ephemeral)
        .unwrap()
        .tokenize(&PiiClass::Email, EMAIL)
        .unwrap();
    for text in [foreign.as_str(), "<Email_1>", EMAIL] {
        assert_eq!(
            pipeline.admit_safety_nets(&session, text, &[LocaleTag::EnUs], &dictionaries()),
            Err(ProtectionError::Residual)
        );
    }
    assert!(session.tokens().is_empty());
}
#[test]
fn no_net_and_custom_locale_skip_are_limits_not_strict_coverage_waivers() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    Pipeline::builder()
        .build()
        .unwrap()
        .admit_safety_nets(
            &session,
            EMAIL,
            &[LocaleTag::DeDe],
            &DictionaryBundle::default(),
        )
        .unwrap();
    let (pipeline, calls) = configured(true);
    pipeline
        .admit_safety_nets(
            &session,
            EMAIL,
            &[LocaleTag::DeDe],
            &DictionaryBundle::default(),
        )
        .unwrap();
    assert!(calls.lock().unwrap().is_empty());
    // Strict still validates the primary graph first, and its existing locale tests stay authoritative.
    assert_eq!(
        pipeline.validate_protection_context(ProtectionContext::strict(
            &[LocaleTag::DeDe],
            &DictionaryBundle::default()
        )),
        Err(ProtectionError::EmptyPrimary)
    );
}

#[test]
fn live_admission_keeps_one_snapshot_when_a_net_adds_a_mapping() {
    struct MutatingNet {
        session: Arc<Session>,
        token: String,
    }
    impl SafetyNet for MutatingNet {
        fn id(&self) -> &str {
            "snapshot-test"
        }
        fn supported_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::Global]
        }
        fn check(
            &self,
            text: &str,
            context: SafetyNetContext<'_>,
        ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
            assert!(context.manifest.spans.is_empty());
            // A concurrent writer can add ownership, but cannot change this scan's snapshot.
            assert_eq!(
                self.session
                    .format_preserving_fake(&PiiClass::Custom("later".into()), "synthetic later")
                    .unwrap(),
                self.token
            );
            Ok(vec![LeakSuspect::new(
                0..text.len(),
                PiiClass::Name,
                self.id(),
                None,
                LeakKind::Uncovered,
                "synthetic",
                None,
            )])
        }
    }
    let session = Arc::new(Session::new(Scope::Ephemeral).unwrap());
    let existing = session.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let token = session
        .begin_transaction()
        .format_preserving_fake(&PiiClass::Custom("later".into()), "synthetic later")
        .unwrap();
    let p = Pipeline::builder()
        .register_safety_net(MutatingNet {
            session: session.clone(),
            token: token.clone(),
        })
        .build()
        .unwrap();
    assert_eq!(
        p.admit_safety_nets(
            &session,
            &token,
            &[LocaleTag::Global],
            &DictionaryBundle::default()
        ),
        Err(ProtectionError::Residual)
    );
    assert_eq!(session.restore(&existing).as_deref(), Some(EMAIL));
    assert_eq!(session.restore(&token).as_deref(), Some("synthetic later"));
}
