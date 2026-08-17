#![cfg(feature = "phone-parser")]

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Pipeline, RawDocument, RawMatch, RecognizerSpec,
    Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::{embedded, NormalizerKind, RegexDetector, ValidatorKind};
use gaze_types::{LocaleTag, PiiClass};

fn core_extended() -> Rulepack {
    let mut rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core-extended").expect("core-extended embedded rulepack"),
    ))
    .expect("core-extended loads");
    rulepack.recognizers.retain(|recognizer| {
        !recognizer.id.starts_with("email.") && !recognizer.id.starts_with("name.")
    });
    rulepack
}

fn regex_from_spec(spec: &RecognizerSpec) -> RegexDetector {
    let RawMatch::Regex {
        pattern,
        pattern_template: None,
        capture_groups,
    } = &spec.matcher
    else {
        panic!("expected plain regex recognizer {}", spec.id);
    };

    RegexDetector::with_rulepack_fields(
        pattern.as_deref().expect("regex pattern"),
        spec.class.clone(),
        &spec.id,
        spec.locales.clone(),
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups.clone(),
        spec.context
            .as_ref()
            .map(|context| context.exclusions.clone())
            .unwrap_or_default(),
        spec.validator
            .as_ref()
            .map(|validator| ValidatorKind::parse(&validator.kind).expect("validator kind")),
        spec.normalizer
            .as_ref()
            .map(|normalizer| NormalizerKind::parse(&normalizer.kind).expect("normalizer kind")),
    )
    .expect("regex detector")
    .with_locale_basis(spec.locale_basis)
}

fn pipeline_from_rulepack(rulepack: &Rulepack) -> Pipeline {
    let mut builder = Pipeline::builder()
        .rule(ClassRule::new(
            PiiClass::Custom("phone".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("iban".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("credit_card".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve));

    for spec in &rulepack.recognizers {
        if spec.enabled {
            builder = builder.recognizer(regex_from_spec(spec));
        }
    }

    builder.build().expect("pipeline")
}

fn clean_text(pipeline: &Pipeline, session: &Session, input: &str, locale: LocaleTag) -> String {
    let clean = pipeline
        .pseudonymize_with_context(
            session,
            RawDocument::Text(input.to_string()),
            &[locale, LocaleTag::Global],
        )
        .expect("clean");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };
    text
}

fn restore_tokens(session: &Session, clean: &str) -> String {
    gaze::token_shape::pattern()
        .replace_all(clean, |captures: &regex::Captures<'_>| {
            session.restore_strict(&captures[0]).expect("known token")
        })
        .to_string()
}

/// Locks research-855 §Rulepack > Normalization, todo #448, and audit
/// scratchpad 862 §M1: recognizer normalizers may canonicalize validator input,
/// but restore must preserve the original byte span byte-for-byte.
#[test]
fn original_span_preserved_through_normalizer_roundtrip() {
    let rulepack = core_extended();
    let pipeline = pipeline_from_rulepack(&rulepack);

    for (input, locale) in [
        ("IBAN: GB82 WEST 1234 5698 7654 32", LocaleTag::EnUs),
        ("card 4242-4242-4242-4242", LocaleTag::EnUs),
        ("phone +49 30 1234567", LocaleTag::DeDe),
    ] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, input, locale);
        assert_ne!(clean, input, "expected tokenized output for {input}");
        assert_eq!(restore_tokens(&session, &clean), input);
    }
}
