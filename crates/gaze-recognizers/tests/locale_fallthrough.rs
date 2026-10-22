//! Span-aware locale fall-through through the bundled `core` rulepack.
//!
//! The registry once stopped at the first chain locale that produced any candidate for a class.
//! Under a global-first chain, one international number (`phone.structural`, `global`) therefore
//! switched `phone.national.de` off for the whole document, and a national German number next to
//! it shipped raw. Earlier locales now win per span, not per document.
//!
//! Numbers come from the BNetzA reserved drama ranges for mobile and Berlin landline contacts.

mod support;

use gaze::{Action, LocaleTag, PiiClass, Pipeline, RuleSpec, Scope, Session};

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

fn pipeline_for(locales: &[LocaleTag]) -> Pipeline {
    support::pipeline_for(
        locales,
        &["core"],
        vec![
            RuleSpec::Class {
                class: PiiClass::custom("phone").expect("valid custom class"),
                action: Action::Tokenize,
            },
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
        false,
    )
}

fn clean_in(locales: &[LocaleTag], text: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    support::clean(&pipeline_for(locales), &session, text, locales)
}

const INTERNATIONAL: &str = "+491713920000";
const NATIONAL: &str = "030 23125000";

#[test]
fn national_rule_fires_alone_under_de_de() {
    let cleaned = clean_in(&[LocaleTag::DeDe], &format!("Büro: {NATIONAL}."));
    assert!(!without_tokens(&cleaned).contains("23125000"));
}

#[test]
fn global_first_chain_keeps_national_rule_beside_an_international_number() {
    let text = format!("Mobil {INTERNATIONAL}. Büro: {NATIONAL}.");
    for chain in [
        vec![LocaleTag::Global, LocaleTag::DeDe],
        vec![LocaleTag::Global, LocaleTag::EnUs, LocaleTag::DeDe],
    ] {
        let cleaned = clean_in(&chain, &text);
        assert!(!cleaned.contains(INTERNATIONAL), "{chain:?}: {cleaned:?}");
        assert!(!without_tokens(&cleaned).contains("23125000"), "{chain:?}");
        assert!(cleaned.contains("Büro: "), "{chain:?}: {cleaned:?}");
    }
}
