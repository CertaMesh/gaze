#![cfg(feature = "bundled-recognizers")]

use gaze::{
    Action, CleanDocument, DefaultRule, DictionaryBundle, LocaleTag, PiiClass, Pipeline,
    RawDocument, Scope, Session, Value,
};
use gaze_recognizers::{DictionaryRecognizer, RegexDetector};
use gaze_types::RulepackDict;
use std::collections::BTreeMap;

fn email_pipeline(action: Action) -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .rule(DefaultRule::new(action))
        .enable_prefix_cache()
        .build()
        .unwrap()
}

fn check_context_change(kind: &str, staged: bool) {
    let first = email_pipeline(Action::Preserve);
    let active = match kind {
        "policy" => email_pipeline(Action::Tokenize),
        "locale" => Pipeline::builder()
            .recognizer(
                RegexDetector::with_rulepack_fields(
                    r"alice@example\.invalid",
                    PiiClass::Email,
                    "review.email",
                    vec![LocaleTag::DeDe],
                    1.0,
                    0,
                    "counter",
                    None,
                    vec![],
                    None,
                    None,
                )
                .unwrap(),
            )
            .rule(DefaultRule::new(Action::Tokenize))
            .enable_prefix_cache()
            .build()
            .unwrap(),
        "dictionary" => Pipeline::builder()
            .recognizer(DictionaryRecognizer::new(
                "review.dictionary",
                PiiClass::Email,
                "dict_alpha",
                true,
                "counter",
            ))
            .rule(DefaultRule::new(Action::Tokenize))
            .enable_prefix_cache()
            .build()
            .unwrap(),
        _ => unreachable!(),
    };
    let empty = DictionaryBundle::default();
    let populated = DictionaryBundle::from_rulepack_terms(&[RulepackDict::new(
        "dict_alpha",
        vec!["alice@example.invalid".into()],
        true,
    )]);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let mut redact =
        |pipeline: &Pipeline, text: &str, locale: &[LocaleTag], dict: &DictionaryBundle| {
            let raw = RawDocument::Structured(BTreeMap::from([(
                "same_field".into(),
                Value::String(text.into()),
            )]));
            if staged {
                pipeline.pseudonymize_transaction_with_detect_context(
                    &mut transaction,
                    raw,
                    locale,
                    dict,
                )
            } else {
                pipeline.pseudonymize_with_detect_context(&session, raw, locale, dict)
            }
            .unwrap()
        };
    let priming = if kind == "policy" { &first } else { &active };
    let prime = redact(
        priming,
        "alice@example.invalid x",
        &[LocaleTag::Global],
        &empty,
    );
    let CleanDocument::Structured(prime) = prime else {
        panic!("structured")
    };
    assert_eq!(prime["same_field"], "alice@example.invalid x");
    let dict = if kind == "dictionary" {
        &populated
    } else {
        &empty
    };
    let locale = if kind == "locale" {
        LocaleTag::DeDe
    } else {
        LocaleTag::Global
    };
    let clean = redact(
        &active,
        "alice@example.invalid x extra",
        std::slice::from_ref(&locale),
        dict,
    );
    // A fresh session proves the active decision context protects this exact input.
    let fresh = Session::new(Scope::Ephemeral).unwrap();
    let control = active
        .pseudonymize_with_detect_context(
            &fresh,
            RawDocument::Structured(BTreeMap::from([(
                "same_field".into(),
                Value::String("alice@example.invalid x extra".into()),
            )])),
            std::slice::from_ref(&locale),
            dict,
        )
        .unwrap();
    let CleanDocument::Structured(control) = control else {
        panic!("structured")
    };
    assert!(
        !control["same_field"]
            .as_str()
            .unwrap()
            .contains("alice@example.invalid"),
        "control must protect"
    );
    let CleanDocument::Structured(clean) = clean else {
        panic!("structured")
    };
    assert!(
        !clean["same_field"]
            .as_str()
            .unwrap()
            .contains("alice@example.invalid"),
        "prefix cache replayed stale {kind} context; staged={staged}",
    );
}

#[test]
fn prefix_cache_policy_live() {
    check_context_change("policy", false);
}
#[test]
fn prefix_cache_policy_staged() {
    check_context_change("policy", true);
}
#[test]
fn prefix_cache_locale_live() {
    check_context_change("locale", false);
}
#[test]
fn prefix_cache_locale_staged() {
    check_context_change("locale", true);
}
#[test]
fn prefix_cache_dictionary_live() {
    check_context_change("dictionary", false);
}
#[test]
fn prefix_cache_dictionary_staged() {
    check_context_change("dictionary", true);
}
