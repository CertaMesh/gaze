use super::*;

const RAW: &str = "password: \"left right\"\nmarker";

#[derive(Clone)]
struct Fixed(Vec<Candidate>);
impl crate::Recognizer for Fixed {
    fn id(&self) -> &str {
        "synthetic.fixed"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        _: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self.0.clone())
    }
}
fn candidate(span: Range<usize>, class: PiiClass, id: &str) -> Candidate {
    Candidate::new(
        span,
        class,
        id,
        0.9,
        0,
        None,
        "counter",
        id,
        ConflictTier::None,
        vec![],
    )
}
fn field() -> PiiClass {
    PiiClass::custom("password").unwrap()
}
fn pair() -> Vec<Candidate> {
    vec![
        candidate(0..15, PiiClass::Name, "synthetic.name"),
        candidate(11..21, field(), "synthetic.field"),
    ]
}
fn pipeline(candidates: Vec<Candidate>, enabled: bool) -> Pipeline {
    let mut p = Pipeline::builder()
        .recognizer(Fixed(candidates))
        .rule(crate::rule::DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    p.residual_coverage = enabled;
    p
}
fn clean(p: &Pipeline, session: &Session, raw: &str) -> Result<CleanText> {
    p.redact_text_with_manifest_uncached(
        &mut ProtectionTarget::Live(session),
        raw,
        None,
        DocumentKind::Text,
        &[crate::LocaleTag::Global],
        &DictionaryBundle::default(),
        None,
    )
}
#[test]
fn pair_residual_preserves_whole_token_and_exact_raw_union() {
    let old_session = Session::new(crate::Scope::Ephemeral).unwrap();
    let new_session = &old_session;
    let old = clean(&pipeline(pair(), false), &old_session, RAW).unwrap();
    let new = clean(&pipeline(pair(), true), new_session, RAW).unwrap();
    assert_eq!(
        old.manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..15]
    );
    assert_eq!(
        new.manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..15, 15..21]
    );
    assert_eq!(
        &new.text[new.manifest[0].clean_span.clone()],
        &old.text[old.manifest[0].clean_span.clone()]
    );
    assert_eq!(&new.text[new.manifest[1].clean_span.end..], &RAW[21..]);
    assert_eq!(new_session.restore_strict_text(&new.text).unwrap(), RAW);
}
