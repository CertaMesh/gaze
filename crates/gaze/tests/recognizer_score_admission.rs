use gaze::{
    Action, Candidate, ConflictTier, DefaultRule, DetectContext, DetectError, DictionaryBundle,
    LocaleBasis, LocaleTag, PiiClass, Pipeline, RawDocument, Recognizer, Scope, Session,
};

const INPUT: &str = "alice@example.invalid";

struct ScoredRecognizer {
    score: f32,
}

impl Recognizer for ScoredRecognizer {
    fn id(&self) -> &str {
        "score.fixture"
    }

    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Email
    }

    fn token_family(&self) -> &str {
        "Email"
    }

    fn locale_basis(&self) -> LocaleBasis {
        LocaleBasis::Format
    }

    fn detect(&self, input: &str, _: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        Ok(vec![Candidate::new(
            0..input.len(),
            PiiClass::Email,
            self.id(),
            self.score,
            0,
            None,
            self.token_family(),
            self.id(),
            ConflictTier::None,
            Vec::new(),
        )])
    }
}

#[test]
fn nan_score_refuses_pseudonymization_instead_of_returning_raw_input() {
    let pipeline = Pipeline::builder()
        .recognizer(ScoredRecognizer { score: f32::NAN })
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
