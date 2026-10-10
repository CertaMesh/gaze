use gaze::{
    Action, Candidate, CleanDocument, ConflictTier, DefaultRule, DetectContext, DetectError,
    DictionaryBundle, LocaleBasis, LocaleTag, PiiClass, Pipeline, RawDocument, Recognizer,
    RecognizerRegistry, Scope, Session,
};

const INPUT: &str = "alice@example.invalid";

struct ScoredRecognizer {
    score: f32,
    path: Path,
    mixed: bool,
}

#[derive(Clone, Copy, Debug)]
enum Path {
    Format,
    Document,
    ReusedDocument,
    PostCandidate,
}

const PATHS: [Path; 4] = [
    Path::Format,
    Path::Document,
    Path::ReusedDocument,
    Path::PostCandidate,
];

const LOCALES: [LocaleTag; 2] = [LocaleTag::EnUs, LocaleTag::Global];

impl Recognizer for ScoredRecognizer {
    fn id(&self) -> &str {
        // Deliberately user-controlled: neither this id nor candidate metadata may leak
        // into admission errors.
        INPUT
    }

    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Email
    }

    fn token_family(&self) -> &str {
        "Email"
    }

    fn locale_basis(&self) -> LocaleBasis {
        match self.path {
            Path::Format | Path::PostCandidate => LocaleBasis::Format,
            Path::Document | Path::ReusedDocument => LocaleBasis::Document,
        }
    }

    fn locales(&self) -> &[LocaleTag] {
        &LOCALES
    }

    fn detect_is_locale_invariant(&self) -> bool {
        matches!(self.path, Path::ReusedDocument)
    }

    fn requires_prior_candidates(&self) -> bool {
        matches!(self.path, Path::PostCandidate)
    }

    fn detect(&self, input: &str, _: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        let mut candidates = vec![Candidate::new(
            0..input.len(),
            PiiClass::Email,
            self.id(),
            self.score,
            0,
            Some(input.to_string()),
            self.token_family(),
            self.id(),
            ConflictTier::None,
            Vec::new(),
        )];
        if self.mixed {
            let mut valid = candidates[0].clone();
            valid.score = 1.0;
            candidates.insert(0, valid);
        }
        Ok(candidates)
    }
}

#[test]
fn nan_score_refuses_pseudonymization_instead_of_returning_raw_input() {
    let pipeline = Pipeline::builder()
        .recognizer(ScoredRecognizer {
            score: f32::NAN,
            path: Path::Format,
            mixed: false,
        })
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let result = pipeline.pseudonymize_with_detect_context(
        &session,
        RawDocument::Text(INPUT.into()),
        &[LocaleTag::Global],
        &DictionaryBundle::default(),
    );
    match result {
        Err(gaze::Error::RecognizerDetect(error)) => assert_eq!(
            error,
            DetectError::backend("registry.score-admission", "invalid_score")
        ),
        other => panic!("expected typed refusal, got {other:?}"),
    }
}

fn invalid_score_error() -> DetectError {
    DetectError::backend("registry.score-admission", "invalid_score")
}

#[test]
fn every_registry_path_refuses_entire_nonfinite_batches() {
    for path in PATHS {
        for score in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for mixed in [false, true] {
                let registry = RecognizerRegistry::builder()
                    .register(ScoredRecognizer { score, path, mixed })
                    .build();
                let dictionaries = DictionaryBundle::default();
                let ctx = DetectContext::new(&LOCALES, &dictionaries);
                for result in [
                    registry.detect_all(INPUT, &ctx),
                    registry
                        .detect_all_resolved(INPUT, &ctx)
                        .map(|(found, _)| found),
                ] {
                    let error = result.expect_err("invalid batches must never return candidates");
                    assert_eq!(error, invalid_score_error(), "{path:?}, {score}, {mixed}");
                    assert!(!format!("{error:?} {error}").contains(INPUT));
                }
            }
        }
    }
}

#[test]
fn every_pipeline_path_refuses_without_output_or_partial_session_state() {
    for path in PATHS {
        for score in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for mixed in [false, true] {
                let pipeline = Pipeline::builder()
                    .recognizer(ScoredRecognizer { score, path, mixed })
                    .rule(DefaultRule::new(Action::Tokenize))
                    .build()
                    .unwrap();
                let session = Session::new(Scope::Ephemeral).unwrap();
                let error = pipeline
                    .pseudonymize_with_detect_context(
                        &session,
                        RawDocument::Text(INPUT.into()),
                        &LOCALES,
                        &DictionaryBundle::default(),
                    )
                    .expect_err("invalid scores must never return a clean document");
                assert!(
                    matches!(
                        &error,
                        gaze::Error::RecognizerDetect(inner) if *inner == invalid_score_error()
                    ),
                    "{path:?}, {score}, {mixed}: {error:?}"
                );
                assert!(!format!("{error:?} {error}").contains(INPUT));
                assert!(session.snapshot_entries().is_empty());
            }
        }
    }
}

#[test]
fn finite_scores_keep_the_existing_zero_floor_on_every_path() {
    for path in PATHS {
        for (score, accepted) in [
            (-f32::MAX, false),
            (-1.0, false),
            (-f32::from_bits(1), false),
            (-0.0, true),
            (0.0, true),
            (f32::from_bits(1), true),
            (1.0, true),
            (f32::MAX, true),
        ] {
            let registry = RecognizerRegistry::builder()
                .register(ScoredRecognizer {
                    score,
                    path,
                    mixed: false,
                })
                .build();
            let dictionaries = DictionaryBundle::default();
            let ctx = DetectContext::new(&LOCALES, &dictionaries);
            // Unresolved detection historically returns finite negative candidates too.
            assert!(!registry.detect_all(INPUT, &ctx).unwrap().is_empty());
            let (candidates, vetoed) = registry.detect_all_resolved(INPUT, &ctx).unwrap();
            assert!(vetoed.is_empty());
            assert_eq!(candidates.len(), usize::from(accepted), "{path:?}, {score}");
            if accepted {
                assert_eq!(candidates[0].span, 0..INPUT.len());
                assert_eq!(candidates[0].score.to_bits(), score.to_bits());
            }
            let pipeline = Pipeline::builder()
                .recognizer(ScoredRecognizer {
                    score,
                    path,
                    mixed: false,
                })
                .rule(DefaultRule::new(Action::Tokenize))
                .build()
                .unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let clean = pipeline
                .pseudonymize_with_detect_context(
                    &session,
                    RawDocument::Text(INPUT.into()),
                    &LOCALES,
                    &dictionaries,
                )
                .unwrap();
            let CleanDocument::Text(text) = clean else {
                panic!("expected clean text");
            };
            if accepted {
                assert!(!text.contains(INPUT));
                assert!(text.starts_with('<') && text.ends_with(":Email_1>"));
                assert_eq!(session.restore(&text).as_deref(), Some(INPUT));
            } else {
                assert_eq!(text, INPUT);
                assert!(session.snapshot_entries().is_empty());
            }
        }
    }
}
