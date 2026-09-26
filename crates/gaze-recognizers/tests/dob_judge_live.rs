use std::path::PathBuf;

use gaze::{
    Action, Candidate, CleanDocument, ConflictTier, DefaultRule, DetectContext, DictionaryBundle,
    PiiClass, Pipeline, RawDocument, Recognizer, Scope, Session,
};
use gaze_recognizers::DobJudgeRecognizer;

#[test]
fn local_bundle_classifies_synthetic_dates() {
    let Some(path) = std::env::var_os("GAZE_GLINER_DOB_TEST_BUNDLE") else {
        return;
    };
    let recognizer = DobJudgeRecognizer::load(&PathBuf::from(path), 0.5).unwrap();
    let dictionaries = DictionaryBundle::default();
    let prior = [];
    let ctx = DetectContext::new(&[], &dictionaries).with_prior_candidates(&prior);
    for (text, date) in [
        (
            "Helena (14.03.1987) is listed in the patient file.",
            "14.03.1987",
        ),
        ("Biografie von Dr. Schmidt (14.03.1987).", "14.03.1987"),
        ("Fiche de Marie (12 mars 1984).", "12 mars 1984"),
    ] {
        let detections = recognizer.detect(text, &ctx).unwrap();
        assert_eq!(detections.len(), 1, "{text}");
        assert_eq!(&text[detections[0].span.clone()], date);
        assert_eq!(detections[0].source, "dob.gliner");
    }
    for text in [
        "Account opened: 12/08/1993.",
        "Invoice issued on 1990-12-05.",
        "Payment due 14/08/1993.",
        "Order shipped 1990-12-05.",
        "Version 1990-12-05 was released.",
        "Log entry at 1990-12-05.",
    ] {
        let detections = recognizer.detect(text, &ctx).unwrap();
        assert!(detections.is_empty(), "{text}: {detections:?}");
    }

    let cue_text = "DOB: 14.03.1987";
    let cue_date = cue_text.find("14.03.1987").unwrap();
    let claimed = [Candidate::new(
        cue_date..cue_date + "14.03.1987".len(),
        PiiClass::Custom("birth_date".into()),
        "birth_date.cue",
        0.9,
        100,
        None,
        "birth_date",
        "birth_date.cue",
        ConflictTier::None,
        Vec::new(),
    )];
    let claimed_ctx = DetectContext::new(&[], &dictionaries).with_prior_candidates(&claimed);
    assert!(recognizer
        .detect(cue_text, &claimed_ctx)
        .unwrap()
        .is_empty());

    let pipeline = Pipeline::builder()
        .recognizer(recognizer)
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let text = "Helena (14.03.1987) is listed in the patient file.";
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.into()),
            &[],
            &dictionaries,
        )
        .unwrap();
    let CleanDocument::Text(cleaned) = clean else {
        panic!("expected text")
    };
    assert!(!cleaned.contains("14.03.1987"));
    assert!(cleaned.contains(":Custom:birth_date_"));
    assert_eq!(
        pipeline.restore_strict_text(&session, &cleaned).unwrap(),
        text
    );
}
