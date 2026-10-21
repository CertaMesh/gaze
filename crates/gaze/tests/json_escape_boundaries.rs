use gaze::*;

struct Substring(&'static str);

impl Recognizer for Substring {
    fn id(&self) -> &str {
        "partial_escape_path"
    }
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Rule
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Location
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(input
            .match_indices(self.0)
            .map(|(at, raw)| {
                Candidate::new(
                    at..at + raw.len(),
                    PiiClass::Location,
                    self.id(),
                    0.99,
                    0,
                    None,
                    self.token_family(),
                    self.id(),
                    ConflictTier::None,
                    vec![],
                )
            })
            .collect())
    }
}

#[test]
fn partial_escape_capture_keeps_literal_path() {
    assert_plain_capture(r#"{"path":"\\new"}"#, r"\new");
}

#[test]
fn partial_unicode_and_surrogate_captures_keep_source_bytes() {
    for (input, raw) in [
        (r#"{"a":"x\n\\tail"}"#, r"x\n\"),
        (r#"{"a":"x\n\u0041"}"#, r"x\n\u00"),
        (r#"{"a":"\u0041\n"}"#, r"0041\n"),
        (r#"{"a":"x\n\uD83D\uDE00"}"#, r"x\n\uD83D"),
        (r#"{"a":"\uD83D\uDE00\n"}"#, r"\uDE00\n"),
        (r#"{"a":"\uD83D\uDE00\n"}"#, r"DE00\n"),
    ] {
        assert_plain_capture(input, raw);
    }
}

fn assert_plain_capture(input: &str, raw: &'static str) {
    let pipeline = Pipeline::builder()
        .recognizer(Substring(raw))
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Conversation("escape-boundaries".into())).unwrap();
    let CleanDocument::Text(clean) = pipeline
        .redact(&session, RawDocument::Text(input.into()))
        .unwrap()
    else {
        panic!("text")
    };
    let token = token_shape::find_tokens(&clean).next().unwrap();
    assert_eq!(session.source_spelling(token), Some(SourceSpelling::Plain));
    assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
    assert_eq!(
        session
            .restore_strict_decoded_text_with_provenance(token)
            .unwrap()
            .text,
        raw
    );
}
