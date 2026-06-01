#![cfg(feature = "test-support")]

use std::path::Path;

use gaze::{
    DictionaryBundle, DictionaryEntry, DictionarySource, Error, LocaleTag, PiiClass, Pipeline,
    RawDocument, Scope, Session,
};
use gaze_recognizers::{DictionaryRecognizer, NerOptions, NerRecognizer};
use gaze_types::{DetectContext, Recognizer};

#[test]
fn ner_backend_error_fails_closed_pipeline() {
    let recognizer =
        NerRecognizer::load_with_options(Path::new("__gaze_test_error_ner"), NerOptions::default())
            .expect("test support recognizer");
    let pipeline = Pipeline::builder()
        .recognizer(recognizer)
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");

    let result = pipeline.redact(
        &session,
        RawDocument::Text("Alice Example visited Berlin".to_string()),
    );

    let err = result.expect_err("backend error must fail closed before clean output");
    match err {
        Error::RecognizerDetect(gaze_types::DetectError::Backend {
            recognizer_id,
            message,
            ..
        }) => {
            assert_eq!(recognizer_id, "ner");
            assert!(message.contains("forced test-support backend failure"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn recognizer_infallible_detect_ok() {
    let recognizer = DictionaryRecognizer::new(
        "dict.artist",
        PiiClass::Custom("artist".to_string()),
        "artists",
        true,
        "counter",
    );
    let entry = DictionaryEntry::new(
        "artists",
        vec!["Synthetic Artist".to_string()],
        true,
        DictionarySource::Cli,
    )
    .expect("dictionary entry");
    let dictionaries = DictionaryBundle::from_entries([("artists".to_string(), entry)]);
    let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);

    let candidates = recognizer
        .detect("Play Synthetic Artist", &ctx)
        .expect("infallible recognizer should use default Ok path");

    assert_eq!(candidates.len(), 1);
    assert_eq!(
        &"Play Synthetic Artist"[candidates[0].span.clone()],
        "Synthetic Artist"
    );
}
