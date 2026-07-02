#![cfg(feature = "bundled-recognizers")]

//! Defaults to 64 cases for local speed. Set `PROPTEST_CASES=<n>` to broaden
//! the run in CI or during investigation.

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, PiiClass, Pipeline, RawDocument, Scope, Session,
};
use gaze_recognizers::RegexDetector;
use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;

#[derive(Debug, Clone)]
struct DocumentCase {
    text: String,
    detected_pii: Vec<String>,
}

#[derive(Debug, Clone)]
enum Segment {
    Filler(String),
    DetectedPii(String),
}

fn prop_config() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(64),
        ..Default::default()
    }
}

fn deterministic_pipeline() -> Pipeline {
    let phone = PiiClass::custom("phone");

    Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .detector(RegexDetector::new(r"\+1-555-01[0-9]{2}", phone.clone()).expect("us phone"))
        .detector(RegexDetector::new(r"\+44-7700-900[0-9]{3}", phone.clone()).expect("uk phone"))
        .detector(RegexDetector::new(r"\+49 1555 011[0-9]{4}", phone.clone()).expect("de phone"))
        .detector(RegexDetector::new(r"Dr\. Schmidt", PiiClass::Name).expect("name detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(ClassRule::new(phone, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline")
}

fn filler_segment() -> impl Strategy<Value = Segment> {
    proptest::string::string_regex(r"[\p{L}\p{N}\p{P}\p{Z}\p{M}\x{1F600}-\x{1F64F}]{0,40}")
        .expect("valid filler regex")
        .prop_filter(
            "filler does not contain tracked PII or restore-token delimiters",
            |text| {
                !text.contains('@')
                    && !text.contains('<')
                    && !text.contains('>')
                    && !text.contains("+1-555")
                    && !text.contains("+44-7700")
                    && !text.contains("+49 1555")
                    && !text.contains("Dr. Schmidt")
            },
        )
        .prop_map(Segment::Filler)
}

fn detected_pii_segment() -> impl Strategy<Value = Segment> {
    prop_oneof![
        "[a-z0-9]{1,20}".prop_map(|local| Segment::DetectedPii(format!("{local}@example.invalid"))),
        "[0-9]{2}".prop_map(|suffix| Segment::DetectedPii(format!("+1-555-01{suffix}"))),
        "[0-9]{3}".prop_map(|suffix| Segment::DetectedPii(format!("+44-7700-900{suffix}"))),
        "[0-9]{4}".prop_map(|suffix| Segment::DetectedPii(format!("+49 1555 011{suffix}"))),
        Just(Segment::DetectedPii("Dr. Schmidt".to_string())),
    ]
}

fn document_case() -> impl Strategy<Value = DocumentCase> {
    prop::collection::vec(
        prop_oneof![5 => filler_segment(), 2 => detected_pii_segment()],
        1..24,
    )
    .prop_map(|segments| {
        let mut text = String::new();
        let mut detected_pii = Vec::new();

        for (idx, segment) in segments.into_iter().enumerate() {
            if idx > 0 {
                text.push(' ');
            }
            match segment {
                Segment::Filler(value) => text.push_str(&value),
                Segment::DetectedPii(value) => {
                    text.push_str(&value);
                    detected_pii.push(value);
                }
            }
        }

        DocumentCase { text, detected_pii }
    })
}

fn clean_text(pipeline: &Pipeline, session: &Session, text: &str) -> String {
    let (clean, _, _) = pipeline
        .clean_with_safety_net(session, RawDocument::Text(text.to_string()), &[])
        .expect("clean");
    let CleanDocument::Text(clean_text) = clean else {
        panic!("expected text document");
    };
    clean_text
}

proptest! {
    #![proptest_config(prop_config())]

    #[test]
    fn prop_roundtrip_restore_identity(case in document_case()) {
        let pipeline = deterministic_pipeline();
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, &case.text);

        let restored = pipeline
            .restore_strict_text(&session, &clean)
            .expect("strict restore");

        prop_assert_eq!(restored, case.text);
    }

    #[test]
    fn prop_roundtrip_clean_text_contains_no_detected_pii(case in document_case()) {
        let pipeline = deterministic_pipeline();
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, &case.text);

        for value in &case.detected_pii {
            prop_assert!(
                !clean.contains(value),
                "clean text leaked synthetic PII value {value:?} in {clean:?}"
            );
        }
    }
}
