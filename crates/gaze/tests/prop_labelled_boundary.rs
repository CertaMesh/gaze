#![cfg(feature = "bundled-recognizers")]

use gaze::{
    Action, CleanDocument, DefaultRule, DictionaryBundle, LocaleTag, PiiClass, Pipeline,
    RawDocument, Scope, Session,
};
use gaze_recognizers::RegexDetector;
use proptest::prelude::*;

fn labelled_pipeline() -> Pipeline {
    let marker = RegexDetector::new(r"(?i)(?:X9Y8Z7|alice@example\.invalid)", PiiClass::Email)
        .expect("marker recognizer");
    let labelled = RegexDetector::with_rulepack_fields(
        r"(?i)\btax[ ]+number[ \t]*[:=|\t-][ \t]*([A-Z]{2}\d{6})",
        PiiClass::custom("tax_number").expect("class"),
        "tax_number.labelled",
        vec![LocaleTag::Global],
        0.84,
        84,
        "tax_number",
        Some(vec![1]),
        Vec::new(),
        None,
        None,
    )
    .expect("labelled recognizer")
    .with_complete_labelled_value(true);
    Pipeline::builder()
        .recognizer(marker)
        .recognizer(labelled)
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .expect("pipeline")
}

fn unicode_fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("é"),
        Just("e\u{0301}"),
        Just("中文"),
        Just("😀"),
        Just("\u{200B}"),
        Just("\u{2060}"),
        Just("\u{FEFF}"),
        Just("\u{00A0}"),
        Just("\u{202F}"),
        Just("<"),
        Just(">"),
        Just("AB12"),
    ]
    .prop_map(str::to_owned)
}

fn plain_fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("é"),
        Just("e\u{0301}"),
        Just("\u{0301}"),
        Just("中文"),
        Just("😀"),
        Just("\u{200B}"),
        Just("\u{2060}"),
        Just("\u{FEFF}"),
        Just("\u{00A0}"),
        Just("\u{202F}"),
        Just("AB12"),
        Just(" "),
        Just("."),
        Just("/"),
        Just("_"),
        Just(":"),
        Just("-"),
        Just("ß"),
        Just("CD"),
        Just("7"),
        Just("DRIVER LICENSE: "),
        Just("SSN "),
    ]
    .prop_map(str::to_owned)
}

fn document_case() -> impl Strategy<Value = String> {
    let angle_case = (
        prop_oneof![Just("X9Y8Z7"), Just("alice@example.invalid")],
        prop::collection::vec(unicode_fragment(), 0..4),
        prop_oneof![Just(":"), Just("="), Just("|"), Just("\t"), Just("-")],
        prop::collection::vec(unicode_fragment(), 0..4),
    )
        .prop_map(|(marker, before, connector, after)| {
            // Angle fragments touch the marker token and the labelled value directly.
            format!(
                "{marker}{} Tax number{connector} AB123456{}",
                before.concat(),
                after.concat()
            )
        });
    let adjacent_case = (
        prop_oneof![Just("X9Y8Z7"), Just("alice@example.invalid")],
        prop::collection::vec(plain_fragment(), 1..4),
        prop_oneof![Just(":"), Just("="), Just("|"), Just("\t"), Just("-")],
        prop::collection::vec(plain_fragment(), 1..4),
    )
        .prop_map(|(marker, before, connector, after)| {
            format!(
                "{marker}{} Tax number{connector} AB123456{}",
                before.concat(),
                after.concat()
            )
        });
    prop_oneof![angle_case, adjacent_case]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, .. ProptestConfig::default() })]

    #[test]
    fn unicode_around_labelled_values_never_panics_and_restores(text in document_case()) {
        let pipeline = labelled_pipeline();
        let session = Session::new(Scope::Ephemeral).expect("session");
        let (clean, _, _) = pipeline
            .clean_with_safety_net_detect_context(
                &session,
                RawDocument::Text(text.clone()),
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
            )
            .expect("clean");
        let CleanDocument::Text(cleaned) = clean else {
            panic!("expected text")
        };
        prop_assert!(!cleaned.contains("AB123456"), "labelled value remained raw");
        prop_assert_eq!(pipeline.restore_strict_text(&session, &cleaned).expect("restore"), text);
    }
}
