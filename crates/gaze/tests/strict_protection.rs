use gaze::*;
use std::sync::{Arc, Mutex};
const EMAIL: &str = "alice@example.invalid";
#[derive(Clone)]
struct Primary;
impl Detector for Primary {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .match_indices(EMAIL)
            .map(|(start, _)| {
                Detection::new(start..start + EMAIL.len(), PiiClass::Email, "synthetic")
            })
            .collect()
    }
}
fn pipeline(action: Action) -> Pipeline {
    Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, action))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}
fn protect(
    pipeline: &Pipeline,
    transaction: &mut SessionTransaction<'_>,
    input: &str,
) -> std::result::Result<String, ProtectionError> {
    pipeline.protect_text_transaction(
        transaction,
        input,
        ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default()),
    )
}
#[test]
fn primary_floor_and_round_trip_without_publication() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    assert_eq!(
        protect(
            &Pipeline::builder().build().unwrap(),
            &mut transaction,
            "benign"
        ),
        Err(ProtectionError::EmptyPrimary)
    );
    let pipeline = pipeline(Action::Tokenize);
    let token = protect(&pipeline, &mut transaction, EMAIL).unwrap();
    assert!(session.tokens().is_empty());
    assert_eq!(transaction.restore(&token).as_deref(), Some(EMAIL));
    assert_eq!(
        protect(&pipeline, &mut transaction, &format!("{token}{token}")),
        Ok(format!("{token}{token}"))
    );
    let custom = transaction
        .tokenize(&PiiClass::Custom("class_alpha".into()), "synthetic alpha")
        .unwrap();
    assert_eq!(protect(&pipeline, &mut transaction, &custom), Ok(custom));
    let fake = transaction
        .format_preserving_fake(&PiiClass::Email, "bob@example.invalid")
        .unwrap();
    assert_eq!(protect(&pipeline, &mut transaction, &fake), Ok(fake));
    transaction.commit().unwrap();
    let mut transaction = session.begin_transaction();
    assert_eq!(protect(&pipeline, &mut transaction, &token), Ok(token));
}
#[test]
fn literal_collision_and_one_way_replacements_reject_without_publish() {
    for action in [Action::Redact, Action::Generalize] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut transaction = session.begin_transaction();
        assert_eq!(
            protect(&pipeline(action), &mut transaction, EMAIL),
            Err(ProtectionError::Provenance)
        );
        drop(transaction);
        assert!(session.tokens().is_empty());
    }
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut prediction = session.begin_transaction();
    let predicted = prediction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    drop(prediction);
    let mut transaction = session.begin_transaction();
    assert_eq!(
        protect(
            &pipeline(Action::Tokenize),
            &mut transaction,
            &format!("{predicted} {EMAIL}")
        ),
        Err(ProtectionError::Provenance)
    );
    assert!(session.tokens().is_empty());
    assert_eq!(
        protect(
            &pipeline(Action::Tokenize),
            &mut session.begin_transaction(),
            "literal <Unknown_1>"
        ),
        Ok("literal <Unknown_1>".into())
    );
}
#[derive(Clone)]
struct Net {
    seen: Arc<Mutex<Vec<(String, usize)>>>,
    whole: bool,
    locale: LocaleTag,
}
impl SafetyNet for Net {
    fn id(&self) -> &str {
        "spy"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        std::slice::from_ref(&self.locale)
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        assert!(context.dictionaries.is_some());
        self.seen
            .lock()
            .unwrap()
            .push((text.into(), context.manifest.spans.len()));
        let range = if self.whole {
            Some(0..text.len())
        } else {
            text.find("residual").map(|start| start..start + 8)
        };
        Ok(range
            .into_iter()
            .map(|range| {
                LeakSuspect::new(
                    range,
                    PiiClass::Name,
                    "spy",
                    None,
                    LeakKind::Uncovered,
                    "synthetic",
                    None,
                )
            })
            .collect())
    }
}
#[test]
fn mandatory_scans_ignore_both_skip_flags_and_cover_complete_owned_leaf() {
    let seen = Arc::new(Mutex::new(vec![]));
    let p = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .register_safety_net(Net {
            seen: seen.clone(),
            whole: false,
            locale: LocaleTag::Global,
        })
        .build()
        .unwrap()
        .with_pipeline_optimizations(
            PipelineOptimizationConfig::new()
                .with_skip_class_gating(true)
                .with_capitals_heuristic_gate(true),
        );
    let session = Session::new(Scope::Ephemeral).unwrap();
    assert_eq!(
        protect(&p, &mut session.begin_transaction(), "residual"),
        Err(ProtectionError::Residual)
    );
    let mut tx = session.begin_transaction();
    let token = protect(&p, &mut tx, EMAIL).unwrap();
    assert_eq!(protect(&p, &mut tx, &token), Ok(token.clone()));
    let seen = seen.lock().unwrap();
    assert_eq!(
        *seen,
        vec![("residual".into(), 0), (token.clone(), 1), (token, 1)]
    );
}
#[test]
fn coverage_must_not_cross_raw_gap_and_incompatible_custom_locale_fails() {
    let p = Pipeline::builder()
        .detector(Primary)
        .register_safety_net(Net {
            seen: Default::default(),
            whole: true,
            locale: LocaleTag::Global,
        })
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session.tokenize(&PiiClass::Email, EMAIL).unwrap();
    assert_eq!(
        protect(&p, &mut session.begin_transaction(), &token),
        Ok(token.clone())
    );
    assert_eq!(
        protect(
            &p,
            &mut session.begin_transaction(),
            &format!("{token} gap {token}")
        ),
        Err(ProtectionError::Residual)
    );
    let p = Pipeline::builder()
        .detector(Primary)
        .register_safety_net(Net {
            seen: Default::default(),
            whole: false,
            locale: LocaleTag::DeDe,
        })
        .build()
        .unwrap();
    assert_eq!(
        p.protect_text_transaction(
            &mut session.begin_transaction(),
            "benign",
            ProtectionContext::strict(&[LocaleTag::EnUs], &DictionaryBundle::default())
        ),
        Err(ProtectionError::UnsupportedCoverage)
    );
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn mandatory_registry_runs_every_selected_backend_and_rejects_uncovered_locale() {
    use gaze_recognizers::{
        LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
    };
    struct Model {
        id: &'static str,
        seen: Arc<Mutex<Vec<String>>>,
    }
    impl LocaleAwareModel for Model {
        fn name(&self) -> &str {
            self.id
        }
        fn native_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::EnUs]
        }
        fn infer(
            &self,
            _: ModelInput,
            _: ModelHints,
        ) -> std::result::Result<Vec<ModelSpan>, ModelError> {
            self.seen.lock().unwrap().push(self.id.into());
            Ok(vec![])
        }
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let registry = LocaleAwareModelRegistry::from_backends(vec![
        Box::new(Model {
            id: "a",
            seen: seen.clone(),
        }),
        Box::new(Model {
            id: "b",
            seen: seen.clone(),
        }),
    ]);
    let p = pipeline(Action::Tokenize).with_safety_net_registry(registry);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let dictionaries = DictionaryBundle::default();
    p.protect_text_transaction(
        &mut session.begin_transaction(),
        "benign",
        ProtectionContext::strict(&[LocaleTag::EnUs], &dictionaries),
    )
    .unwrap();
    let mut names = seen.lock().unwrap().clone();
    names.sort();
    assert_eq!(names, ["a", "b"]);
    assert!(p
        .protect_text_transaction(
            &mut session.begin_transaction(),
            "benign",
            ProtectionContext::strict(&[LocaleTag::DeDe], &dictionaries)
        )
        .is_err());
}

#[test]
fn format_preserving_emission_must_match_actual_restore_word_boundaries() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    // The detector replaces EMAIL only; the final fake has no left word boundary.
    assert_eq!(
        protect(
            &pipeline(Action::FormatPreserve),
            &mut session.begin_transaction(),
            &format!("x{EMAIL}")
        ),
        Err(ProtectionError::Provenance)
    );
    assert!(session.tokens().is_empty());
}

#[cfg(feature = "bundled-recognizers")]
#[test]
fn malformed_model_spans_reject_before_manifest_filtering() {
    use gaze_recognizers::{
        LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
    };
    struct BadModel(std::ops::Range<usize>);
    impl LocaleAwareModel for BadModel {
        fn name(&self) -> &str {
            "bad-span"
        }
        fn native_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::EnUs]
        }
        fn infer(
            &self,
            _: ModelInput,
            _: ModelHints,
        ) -> std::result::Result<Vec<ModelSpan>, ModelError> {
            Ok(vec![ModelSpan {
                text: "synthetic".into(),
                byte_range: self.0.clone(),
                class: PiiClass::Email,
                confidence: None,
                model_name: "bad-span".into(),
            }])
        }
    }
    for range in [0..0, std::ops::Range { start: 2, end: 1 }, 0..100, 1..2] {
        let p = pipeline(Action::Tokenize).with_safety_net_registry(
            LocaleAwareModelRegistry::from_backends(vec![Box::new(BadModel(range))]),
        );
        let session = Session::new(Scope::Ephemeral).unwrap();
        assert_eq!(
            p.protect_text_transaction(
                &mut session.begin_transaction(),
                "é benign",
                ProtectionContext::strict(&[LocaleTag::EnUs], &DictionaryBundle::default())
            ),
            Err(ProtectionError::Residual)
        );
    }
}

#[test]
fn manifest_coordinates_use_expanded_owner_input_and_exact_final_ranges() {
    struct Coordinates;
    impl SafetyNet for Coordinates {
        fn id(&self) -> &str {
            "coordinates"
        }
        fn supported_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::Global]
        }
        fn check(
            &self,
            text: &str,
            context: SafetyNetContext<'_>,
        ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
            let token = text.split(' ').next().unwrap();
            assert_eq!(
                context.manifest.spans,
                [
                    EmittedTokenSpan::new(0..token.len(), 0..EMAIL.len(), PiiClass::Email),
                    EmittedTokenSpan::new(
                        token.len() + 1..text.len(),
                        EMAIL.len() + 1..2 * EMAIL.len() + 1,
                        PiiClass::Email
                    ),
                ]
            );
            Ok(vec![])
        }
    }
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .register_safety_net(Coordinates)
        .build()
        .unwrap();
    let input = format!("{token} {EMAIL}");
    assert_eq!(
        protect(&pipeline, &mut session.begin_transaction(), &input).unwrap(),
        format!("{token} {token}")
    );
}

#[test]
fn malformed_custom_spans_fail_and_errors_require_discarding_staging() {
    struct BadNet(std::ops::Range<usize>);
    impl SafetyNet for BadNet {
        fn id(&self) -> &str {
            "bad-custom"
        }
        fn supported_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::Global]
        }
        fn check(
            &self,
            _: &str,
            _: SafetyNetContext<'_>,
        ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
            Ok(vec![LeakSuspect::new(
                self.0.clone(),
                PiiClass::Name,
                "bad-custom",
                None,
                LeakKind::Uncovered,
                "synthetic",
                None,
            )])
        }
    }
    for range in [0..0, std::ops::Range { start: 2, end: 1 }, 0..1000, 1..2] {
        let p = Pipeline::builder()
            .detector(Primary)
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .register_safety_net(BadNet(range))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut transaction = session.begin_transaction();
        assert_eq!(
            protect(&p, &mut transaction, &format!("é {EMAIL}")),
            Err(ProtectionError::Residual)
        );
        assert!(
            !transaction.tokens().is_empty(),
            "Err does not automatically roll back caller-owned staging"
        );
        drop(transaction);
        assert!(session.tokens().is_empty());
    }
    assert_eq!(
        Pipeline::builder()
            .build()
            .unwrap()
            .validate_protection_context(ProtectionContext::strict(
                &[LocaleTag::Global],
                &DictionaryBundle::default()
            )),
        Err(ProtectionError::EmptyPrimary)
    );
}
