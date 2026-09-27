//! Australian state-anchored postcode fixtures. All example strings are synthetic.

use gaze::RawMatch;
use gaze::{
    Action, CleanDocument, ConflictTier, Context, DictionaryBundle, LocaleChain, LocaleTag,
    PiiClass, Pipeline, RawDocument, RedactionEntry, RedactionLogError, RedactionLogger, RuleSpec,
    Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;
use std::sync::{Arc, Mutex};

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

fn clean_and_restore_with_chain(locales: &[LocaleTag], original: &str) -> String {
    let pipeline = pipeline_for(locales);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(original.to_string()),
            locales,
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

fn clean_and_restore(locale: LocaleTag, original: &str) -> String {
    clean_and_restore_with_chain(&[locale], original)
}

struct CapturingLogger(Arc<Mutex<Vec<RedactionEntry>>>);

impl RedactionLogger for CapturingLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.0.lock().expect("log lock").push(entry.clone());
        Ok(())
    }
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
    assert_ne!(
        clean_and_restore(LocaleTag::EnAu, "Jerrabomberra NSW 2619."),
        "Jerrabomberra NSW 2619."
    );
    assert_ne!(
        clean_and_restore(LocaleTag::EnAu, "Canberra ACT 2618."),
        "Canberra ACT 2618."
    );
    assert_eq!(
        clean_and_restore(LocaleTag::EnAu, "Canberra ACT 2619."),
        "Canberra ACT 2619."
    );
}

#[test]
fn au_state_and_postcode_win_under_shipped_locale_orders() {
    let setup_chain = [
        LocaleTag::EnUs,
        LocaleTag::DeDe,
        LocaleTag::DeAt,
        LocaleTag::DeCh,
        LocaleTag::EnGb,
        LocaleTag::EnIe,
        LocaleTag::EnAu,
    ];
    let core_extended_chain = [
        LocaleTag::Global,
        LocaleTag::EnUs,
        LocaleTag::DeDe,
        LocaleTag::DeAt,
        LocaleTag::DeCh,
        LocaleTag::EnAu,
    ];
    for locales in [&setup_chain[..], &core_extended_chain[..]] {
        for original in [
            "Brisbane QLD 4072, Australia",
            "Canberra ACT\u{202F}2601 Australia",
            "Adelaide SA 5000 Australia",
        ] {
            let cleaned = clean_and_restore_with_chain(locales, original);
            let visible = without_tokens(&cleaned);
            assert!(
                !visible.contains("QLD") && !visible.contains("ACT") && !visible.contains("SA"),
                "state survived under {locales:?}: {cleaned:?}"
            );
        }
        let invalid_act = clean_and_restore_with_chain(locales, "Canberra ACT 2619 Australia");
        assert!(
            !without_tokens(&invalid_act).contains("2619"),
            "postal.at_ch must still protect an out-of-range AU code under {locales:?}: {invalid_act:?}"
        );
    }
}

#[test]
fn shipped_chain_audits_the_enclosed_postal_loser() {
    let locales = [LocaleTag::DeAt, LocaleTag::EnAu];
    let entries = Arc::new(Mutex::new(Vec::new()));
    let pipeline =
        pipeline_for(&locales).with_redaction_logger(CapturingLogger(Arc::clone(&entries)));
    let session = Session::new(Scope::Ephemeral).expect("session");
    let original = "Brisbane QLD 4072, Australia";
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(original.to_string()),
            &locales,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let CleanDocument::Text(cleaned) = clean else {
        panic!("expected text");
    };
    assert!(!without_tokens(&cleaned).contains("QLD 4072"));
    assert_eq!(
        pipeline
            .restore_strict_text(&session, &cleaned)
            .expect("restore"),
        original
    );
    let entries = entries.lock().expect("log lock");
    assert!(entries
        .iter()
        .any(|entry| !entry.conflict_loser && entry.recognizer_id.as_deref() == Some("postal.au")));
    let loser = entries
        .iter()
        .find(|entry| {
            entry.conflict_loser && entry.recognizer_id.as_deref() == Some("postal.at_ch")
        })
        .expect("audit row for enclosed postcode");
    assert_eq!(loser.decided_by, ConflictTier::SameClassContainment);
}

#[test]
fn earlier_numeric_postal_rule_keeps_codes_the_au_rule_rejects() {
    let setup_chain = [
        LocaleTag::EnUs,
        LocaleTag::DeDe,
        LocaleTag::DeAt,
        LocaleTag::DeCh,
        LocaleTag::EnGb,
        LocaleTag::EnIe,
        LocaleTag::EnAu,
    ];
    for original in [
        "Barooga NSW 3644 Australia",
        "Barooga NSW 3644, Australia",
        "Kalka SA 0872 Australia",
        "Jervis Bay ACT 2540 Australia",
        "The NSW 2000 Sydney office",
        "Canberra ACT 2619 Australia",
    ] {
        let cleaned = clean_and_restore_with_chain(&setup_chain, original);
        let code = original
            .split_whitespace()
            .find(|word| {
                word.trim_end_matches(',').len() == 4
                    && word
                        .trim_end_matches(',')
                        .chars()
                        .all(|c| c.is_ascii_digit())
            })
            .expect("four-digit code")
            .trim_end_matches(',');
        assert!(
            !without_tokens(&cleaned).contains(code),
            "earlier numeric recognizer stopped protecting {code}: {cleaned:?}"
        );
    }
    let de_at_only = clean_and_restore(LocaleTag::DeAt, "Brisbane QLD 4072, Australia");
    assert!(
        !without_tokens(&de_at_only).contains("4072"),
        "postal.at_ch must work without en-AU: {de_at_only:?}"
    );
}

#[test]
fn function_words_do_not_anchor_prose() {
    for original in [
        "The NSW 2023 election result.",
        "Our NSW 2025 plan.",
        "Der WA 6100 Bericht.",
        "Die SA 5000 Stellen.",
    ] {
        assert_eq!(clean_and_restore(LocaleTag::EnAu, original), original);
    }
    assert_ne!(
        clean_and_restore(LocaleTag::EnAu, "The Rocks NSW 2000."),
        "The Rocks NSW 2000."
    );
}

#[test]
fn locality_and_terminal_branches_accept_identical_state_ranges() {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let spec = rulepack
        .recognizers
        .iter()
        .find(|spec| spec.id == "postal.au")
        .expect("postal.au");
    let RawMatch::Regex { pattern, .. } = &spec.matcher else {
        panic!("postal.au must be a regex");
    };
    let regex = regex::Regex::new(pattern.as_deref().expect("postal.au pattern"))
        .expect("valid postal.au regex");
    for state in ["NSW", "VIC", "QLD", "SA", "WA", "TAS", "NT", "ACT"] {
        for code in 0..10_000 {
            let pair = format!("{state} {code:04}");
            let locality = format!("Sample {pair} people");
            let terminal = format!("{pair}.");
            let first = regex
                .captures(&locality)
                .and_then(|captures| captures.get(1))
                .is_some();
            let second = regex
                .captures(&terminal)
                .and_then(|captures| captures.get(2))
                .is_some();
            assert_eq!(first, second, "branch range drift for {pair}");
        }
    }
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
