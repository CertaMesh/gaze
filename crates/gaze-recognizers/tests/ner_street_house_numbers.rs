//! House numbers licensed by the real `NerRecognizer`.
//!
//! The house-number stage trusts only candidates carrying the NER recognizer id. Every other
//! test uses a stand-in with that id, so this one drives the real recognizer: renaming its id
//! away from `gaze::NER_RECOGNIZER_ID` would turn the feature off and fail here.

#![cfg(feature = "test-support")]

use gaze::{
    Action, CleanDocument, DefaultRule, DictionaryBundle, LocaleTag, Pipeline, RawDocument,
    SafetyNetPolicy, Scope, Session, StreetNumberOrder, NER_RECOGNIZER_ID,
};
use gaze_recognizers::{NerOptions, NerRecognizer};
use gaze_types::Recognizer;

fn street_ner() -> (tempfile::TempDir, NerRecognizer) {
    let dir = tempfile::tempdir().unwrap();
    let model_dir = dir.path().join("__gaze_test_street_ner");
    std::fs::create_dir(&model_dir).unwrap();
    let recognizer = NerRecognizer::load_with_options(&model_dir, NerOptions::default()).unwrap();
    (dir, recognizer)
}

#[test]
fn the_real_ner_recognizer_licenses_the_house_number() {
    let (_dir, ner) = street_ner();
    assert_eq!(ner.id(), NER_RECOGNIZER_ID);
    let pipeline = Pipeline::builder()
        .recognizer(ner)
        .register_street_lexicon(
            LocaleTag::DeDe,
            StreetNumberOrder::NumberAfter,
            vec!["weg".into()],
        )
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "Bitte an Musterweg 17b senden.";
    let (clean, spans, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(raw.into()),
            &[LocaleTag::DeDe],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    let CleanDocument::Text(text) = clean else {
        panic!("text document")
    };
    let tokenized = spans
        .into_iter()
        .map(|span| raw[span.raw_span].to_string())
        .collect::<Vec<_>>();
    assert_eq!(tokenized, ["Musterweg", "17b"]);
    assert_eq!(session.restore_strict_text(&text).unwrap(), raw);
}
