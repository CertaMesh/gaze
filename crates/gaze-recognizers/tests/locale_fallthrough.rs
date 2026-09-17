//! Span-aware locale fall-through through the bundled `core` rulepack.
//!
//! The registry once stopped at the first chain locale that produced any candidate for a class.
//! Under a global-first chain, one international number (`phone.structural`, `global`) therefore
//! switched `phone.national.de` off for the whole document, and a national German number next to
//! it shipped raw. Earlier locales now win per span, not per document.
//!
//! Numbers are synthetic: `+49 151` and Berlin `030` are valid prefixes, the subscriber digits are
//! not assigned test contacts.

use gaze::Context;
use gaze::{
    Action, CleanDocument, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    RawDocument, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

fn pipeline_for(locales: &[LocaleTag]) -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("phone").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = false;
    let chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(locales));
    let context = Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    };
    gaze_assembly::build_pipeline(&policy, &context, &[rulepack], &chain, None).expect("pipeline")
}

fn clean_in(locales: &[LocaleTag], text: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline_for(locales)
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            locales,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    }
}

const INTERNATIONAL: &str = "+4915199887766";
const NATIONAL: &str = "030 12345678";

#[test]
fn national_rule_fires_alone_under_de_de() {
    let cleaned = clean_in(&[LocaleTag::DeDe], &format!("Büro: {NATIONAL}."));
    assert!(!cleaned.contains("12345678"), "{cleaned:?}");
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
        assert!(!cleaned.contains("12345678"), "{chain:?}: {cleaned:?}");
        assert!(cleaned.contains("Büro: "), "{chain:?}: {cleaned:?}");
    }
}
