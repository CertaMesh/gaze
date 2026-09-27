//! Australian state-anchored postcode fixtures. All example strings are synthetic.

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    RawDocument, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

fn pipeline_for(locales: &[LocaleTag]) -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("postal_code").expect("postal class"),
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

fn clean_and_restore(locale: LocaleTag, original: &str) -> String {
    let pipeline = pipeline_for(std::slice::from_ref(&locale));
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(original.to_string()),
            &[locale],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let CleanDocument::Text(cleaned) = clean else {
        panic!("expected text");
    };
    assert_eq!(
        pipeline
            .restore_strict_text(&session, &cleaned)
            .expect("restore"),
        original
    );
    cleaned
}

#[test]
fn all_eight_states_and_territories_protect_state_and_postcode() {
    for (original, state, code) in [
        ("Sydney NSW 2000.", "NSW", "2000"),
        ("Melbourne VIC 3000.", "VIC", "3000"),
        ("Brisbane QLD 4072.", "QLD", "4072"),
        ("Adelaide SA 5000.", "SA", "5000"),
        ("Perth WA 6000.", "WA", "6000"),
        ("Hobart TAS 7000.", "TAS", "7000"),
        ("Darwin NT 0800.", "NT", "0800"),
        ("Canberra ACT 2601.", "ACT", "2601"),
    ] {
        let cleaned = clean_and_restore(LocaleTag::EnAu, original);
        let visible = without_tokens(&cleaned);
        assert!(!visible.contains(state), "state survived in {cleaned:?}");
        assert!(!visible.contains(code), "postcode survived in {cleaned:?}");
    }
}

#[test]
fn terminal_field_branch_uses_the_same_ranges_for_all_states() {
    for state_and_code in [
        "NSW 2000", "VIC 3000", "QLD 4072", "SA 5000", "WA 6000", "TAS 7000", "NT 0800", "ACT 2601",
    ] {
        let original = format!("{state_and_code}.");
        let cleaned = clean_and_restore(LocaleTag::EnAu, &original);
        assert!(
            !without_tokens(&cleaned).contains(state_and_code),
            "terminal postcode survived in {cleaned:?}"
        );
    }
    for original in ["NSW 6000.", "QLD 3000.", "SA 2000.", "WA 1234."] {
        assert_eq!(clean_and_restore(LocaleTag::EnAu, original), original);
    }
    assert_ne!(
        clean_and_restore(LocaleTag::EnAu, "Canberra ACT 2920."),
        "Canberra ACT 2920."
    );
    assert_eq!(
        clean_and_restore(LocaleTag::EnAu, "Canberra ACT 2921."),
        "Canberra ACT 2921."
    );
    assert_ne!(
        clean_and_restore(LocaleTag::EnAu, "Sydney NSW 2921."),
        "Sydney NSW 2921."
    );
    assert_eq!(
        clean_and_restore(LocaleTag::EnAu, "Sydney NSW 2601."),
        "Sydney NSW 2601."
    );
}

#[test]
fn separators_country_and_table_cells_are_protected() {
    for original in [
        "Canberra ACT\u{00A0}2601.",
        "Canberra ACT\u{202F}2601.",
        "Canberra, ACT, 2601.",
        "Canberra\nACT\n2601.",
        "QLD 4072, Australia",
        "| Brisbane | QLD | 4072 |",
        "45 Cedar Crescent, QLD 4072.",
        "Queensland (QLD 4000).",
    ] {
        let cleaned = clean_and_restore(LocaleTag::EnAu, original);
        assert!(
            !without_tokens(&cleaned).contains("4072")
                && !without_tokens(&cleaned).contains("2601")
                && !without_tokens(&cleaned).contains("4000"),
            "postcode survived in {cleaned:?}"
        );
    }
}

#[test]
fn non_address_numbers_and_hash_prefixed_values_remain_raw() {
    for original in [
        "ACT 2023 report",
        "NSW 1500 staff",
        "SA 2000 units",
        "WA 1234 in non-address text",
        "VIC 3000 people",
        "v1.2.3 ACT 2023 release",
        "#ACT 2601",
        "Canberra ACT #2601.",
        "Canberra ACT 2023 report",
        "ACT 2023.",
        "WA 1234.",
    ] {
        let cleaned = clean_and_restore(LocaleTag::EnAu, original);
        assert_eq!(cleaned, original, "false positive: {original:?}");
    }
}

#[test]
fn australian_rule_requires_en_au_in_explicit_locale_chain() {
    let original = "Brisbane QLD 4072.";
    assert_ne!(clean_and_restore(LocaleTag::EnAu, original), original);
    for locale in [
        LocaleTag::parse("en-NZ").expect("valid locale"),
        LocaleTag::EnUs,
        LocaleTag::Global,
    ] {
        assert_eq!(clean_and_restore(locale, original), original);
    }
}
