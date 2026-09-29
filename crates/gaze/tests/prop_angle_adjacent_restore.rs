#![cfg(feature = "bundled-recognizers")]
//! Literal `<` / `>` beside a token is ordinary text for strict restore (todo 4009).
//!
//! Strict restore used to reject a token whose neighbouring byte was `<` or `>` as a
//! "nested wrapper", so `<alice@example.invalid>` cleaned to `<{token}>` and then failed
//! with `UnknownToken`. These properties place angle fragments directly against every
//! emitted token spelling (wrapped, format-preserving email, bracketless name/custom) and
//! against raw values cleaned by a pipeline, and require byte-exact strict restore.

use gaze::{
    Action, CleanDocument, DefaultRule, DictionaryBundle, LocaleTag, PiiClass, Pipeline,
    RawDocument, Scope, Session,
};
use gaze_recognizers::RegexDetector;
use proptest::prelude::*;

fn angle_fragment() -> impl Strategy<Value = &'static str> {
    // No empty fragment: two bracketless tokens touching each other is word
    // adjacency, a different class from the angle neighbours tested here.
    prop_oneof![
        Just("<"),
        Just(">"),
        Just("<<"),
        Just(">>"),
        Just("<x>"),
        Just("</a>"),
        Just("><"),
        Just("->"),
        Just("(<"),
        Just(" "),
    ]
}

/// One raw value and how the session spells its token.
#[derive(Debug, Clone, Copy)]
enum Spelling {
    Wrapped,
    FormatPreserving,
}

fn value() -> impl Strategy<Value = (PiiClass, &'static str, Spelling)> {
    let class_value = prop_oneof![
        Just((PiiClass::Email, "alice@example.invalid")),
        Just((PiiClass::Name, "Dr. Schmidt")),
        Just((PiiClass::Location, "München")),
        Just((
            PiiClass::custom("credit_card").expect("class"),
            "4111111111111111"
        )),
    ];
    let spelling = prop_oneof![Just(Spelling::Wrapped), Just(Spelling::FormatPreserving)];
    (class_value, spelling).prop_map(|((class, raw), spelling)| (class, raw, spelling))
}

fn email_card_pipeline() -> Pipeline {
    let email = RegexDetector::new(r"[a-z]+@example\.invalid", PiiClass::Email).expect("email");
    let card = RegexDetector::new(
        r"4111111111111111",
        PiiClass::custom("credit_card").expect("class"),
    )
    .expect("card");
    Pipeline::builder()
        .recognizer(email)
        .recognizer(card)
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .expect("pipeline")
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

    #[test]
    fn session_tokens_beside_angle_brackets_restore_exactly(
        parts in prop::collection::vec((angle_fragment(), value(), angle_fragment()), 1..5),
    ) {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let mut protected = String::new();
        let mut expected = String::new();
        for (before, (class, raw, spelling), after) in parts {
            let token = match spelling {
                Spelling::Wrapped => session.tokenize(&class, raw),
                Spelling::FormatPreserving => session.format_preserving_fake(&class, raw),
            }
            .expect("token");
            protected.push_str(&format!("{before}{token}{after}"));
            expected.push_str(&format!("{before}{raw}{after}"));
        }
        prop_assert_eq!(
            session.restore_strict_text(&protected).expect("strict restore"),
            expected
        );
    }

    #[test]
    fn pipeline_values_beside_angle_brackets_round_trip(
        parts in prop::collection::vec(
            (
                angle_fragment(),
                prop_oneof![Just("alice@example.invalid"), Just("4111111111111111")],
                angle_fragment(),
            ),
            1..5,
        ),
    ) {
        let text = parts
            .iter()
            .map(|(before, raw, after)| format!("{before}{raw}{after}"))
            .collect::<Vec<_>>()
            .join(" ");
        let pipeline = email_card_pipeline();
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
        prop_assert!(
            !cleaned.contains("alice@example.invalid") && !cleaned.contains("4111111111111111"),
            "value stayed raw: {}",
            cleaned
        );
        prop_assert_eq!(pipeline.restore_strict_text(&session, &cleaned).expect("restore"), text);
    }
}

#[test]
fn unowned_token_inside_angle_brackets_still_fails_closed() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let owned = session
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .expect("token");
    let unowned = format!("<{}:Email_999>", session.session_hex());
    for text in [
        format!("<{unowned}>"),
        format!("{owned}><{unowned}"),
        format!("<<{unowned}>>"),
        "<<Email_1>>".to_owned(),
    ] {
        assert!(
            session.restore_strict_text(&text).is_err(),
            "unowned token restored: {text}"
        );
    }
}
