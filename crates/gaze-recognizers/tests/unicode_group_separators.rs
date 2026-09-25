//! Unicode space separators between identifier groups (solo todo #3819).
//!
//! Text copied from PDFs and banking UIs writes IBANs, cards and tax IDs with NO-BREAK SPACE
//! (U+00A0), NARROW NO-BREAK SPACE (U+202F) or THIN SPACE (U+2009) between groups. Before the fix
//! every one of those shipped raw: the IBAN pattern accepts only `\x20`, Steuer-ID only `[ -]`,
//! and the card pattern's `\s` matched but `luhn` rejected the non-ASCII byte and vetoed the
//! candidate. The fix folds every space separator (Zs) to ASCII space in the detection view
//! (`gaze::normalize`); the raw bytes, the manifest and restore keep the original separator.
//!
//! Fixture values are synthetic, checksum-valid test numbers.

use gaze::Context;
use gaze::{
    Action, CleanDocument, DictionaryBundle, LocaleChain, LocaleTag, Pipeline, RawDocument,
    RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

const SEPARATORS: [(&str, char); 4] = [
    ("NBSP", '\u{00A0}'),
    ("NARROW NBSP", '\u{202F}'),
    ("THIN SPACE", '\u{2009}'),
    ("FIGURE SPACE", '\u{2007}'),
];

fn chain() -> Vec<LocaleTag> {
    [
        "de-DE", "en-US", "en-GB", "nl-NL", "pt-BR", "fr-FR", "en-IN",
    ]
    .iter()
    .map(|tag| LocaleTag::parse(tag).expect("tag"))
    .chain(std::iter::once(LocaleTag::Global))
    .collect()
}

fn pipeline() -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    policy.rulepacks.bundled = vec!["core".to_string()];
    let context = Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    };
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(&chain()));
    gaze_assembly::build_pipeline(&policy, &context, &[rulepack], &locale_chain, None)
        .expect("pipeline")
}

/// Cleans `text`, then proves restore gives back the ORIGINAL bytes, separators included.
fn clean_and_restore(text: &str) -> String {
    let pipeline = pipeline();
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            &chain(),
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let CleanDocument::Text(cleaned) = clean else {
        panic!("expected text");
    };
    let restored = pipeline
        .restore_strict_text(&session, &cleaned)
        .expect("restore");
    assert_eq!(restored, text, "restore must be byte-exact to the original");
    cleaned
}

/// `{sep}` in `template` is replaced by each separator in turn. Every digit run of the value must be
/// gone outside tokens and a token must carry `class`. The core pack alone has no `iban` anchor
/// cue, so an IBAN resolves to its fail-closed `payment-card-or-iban` family token; a `iban_`
/// suffix accepts both.
fn assert_grouped_value_tokenized(template: &str, value_template: &str, class: &str) {
    for (name, sep) in SEPARATORS {
        let text = template.replace("{sep}", &sep.to_string());
        let value = value_template.replace("{sep}", &sep.to_string());
        let cleaned = clean_and_restore(&text);
        let residue = without_tokens(&cleaned);
        for group in value.split(sep) {
            assert!(
                !residue.contains(group),
                "{name}: group {group:?} of {value:?} survived: {cleaned:?}"
            );
        }
        assert!(
            cleaned.contains(&format!("{class}_")),
            "{name}: expected a {class} token in {cleaned:?}"
        );
    }
}

fn assert_untouched(template: &str) {
    for (name, sep) in SEPARATORS {
        let text = template.replace("{sep}", &sep.to_string());
        let cleaned = clean_and_restore(&text);
        assert_eq!(cleaned, text, "{name}: benign text must pass unchanged");
    }
}

#[test]
fn iban_with_unicode_group_separators_is_tokenized_in_every_length_class() {
    // DE (22, 4n+2), GB (22, letters in the BBAN), NL (18), AT (20, a multiple of four), FR (27).
    for value in [
        "DE89{sep}3704{sep}0044{sep}0532{sep}0130{sep}00",
        "GB29{sep}NWBK{sep}6016{sep}1331{sep}9268{sep}19",
        "NL91{sep}ABNA{sep}0417{sep}1643{sep}00",
        "AT61{sep}1904{sep}3002{sep}3457{sep}3201",
        "FR14{sep}2004{sep}1010{sep}0505{sep}0001{sep}3M02{sep}606",
    ] {
        assert_grouped_value_tokenized(&format!("IBAN {value} bitte."), value, "iban");
    }
}

#[test]
fn card_with_unicode_group_separators_passes_luhn_and_is_tokenized() {
    // The card pattern's `\s` already matched NBSP; the leak was `luhn` vetoing the non-ASCII byte.
    assert_grouped_value_tokenized(
        "Card 4111{sep}1111{sep}1111{sep}1111 on file.",
        "4111{sep}1111{sep}1111{sep}1111",
        "credit_card",
    );
}

#[test]
fn steuer_id_with_unicode_group_separators_wins_whole_with_its_own_class() {
    // Before: `86` raw plus a `Custom:phone` token over the tail (wrong class, two digits leaked).
    let value = "86{sep}095{sep}742{sep}719";
    assert_grouped_value_tokenized(&format!("Steuer-ID: {value}"), value, "steuer_id");
    for (_, sep) in SEPARATORS {
        let text = format!("Steuer-ID: {}", value.replace("{sep}", &sep.to_string()));
        assert!(!clean_and_restore(&text).contains(":Custom:phone_"));
    }
}

#[test]
fn national_ids_with_unicode_group_separators_are_tokenized() {
    assert_grouped_value_tokenized(
        "NHS number: 943{sep}476{sep}5919",
        "943{sep}476{sep}5919",
        "nhs_number",
    );
    assert_grouped_value_tokenized(
        "Aadhaar: 2341{sep}2341{sep}2346",
        "2341{sep}2341{sep}2346",
        "aadhaar",
    );
    assert_grouped_value_tokenized(
        "NINO: AB{sep}12{sep}34{sep}56{sep}C",
        "AB{sep}12{sep}34{sep}56{sep}C",
        "nino",
    );
}

#[test]
fn checksum_invalid_values_with_unicode_separators_stay_vetoed() {
    // Validator-veto contract: a failing checksum is logged as a loser and the text is untouched.
    // The tail `3704…0130 09` also fails Luhn, so no card rule can claim it either.
    assert_untouched("IBAN DE89{sep}3704{sep}0044{sep}0532{sep}0130{sep}09 bitte.");
    assert_untouched("Card 4111{sep}1111{sep}1111{sep}1112 on file.");
}

#[test]
fn benign_space_separated_numbers_stay_untouched() {
    assert_untouched("version=1{sep}2{sep}3 build");
    assert_untouched("{\"color\":\"#D3D3D3\",\"gap\":\"12{sep}px\"}");
    assert_untouched("Seite 3{sep}von{sep}12");
}

#[test]
fn a_unicode_separator_cleans_exactly_like_an_ascii_space() {
    // Parity is the contract: whatever the pipeline does with ASCII-space groups, including the
    // pre-existing card claim on a mod-97-invalid IBAN whose digit tail passes Luhn, it must do
    // with every other space separator. Compared with tokens masked, separators folded back.
    for template in [
        "IBAN DE89{sep}3704{sep}0044{sep}0532{sep}0130{sep}00 bitte.",
        "IBAN DE89{sep}3704{sep}0044{sep}0532{sep}0130{sep}01 bitte.",
        "Card 4111{sep}1111{sep}1111{sep}1111 and ref 12{sep}34.",
        "Steuer-ID: 86{sep}095{sep}742{sep}719, Tel. +49{sep}30{sep}1234567",
        // Checksum-invalid, so vetoed; the phone rule then claims `095 742 718` exactly as it
        // does with ASCII spaces on the base release.
        "Steuer-ID: 86{sep}095{sep}742{sep}718",
        "PLZ 10115{sep}Berlin, version 1{sep}2{sep}3",
    ] {
        let ascii = without_tokens(&clean_and_restore(&template.replace("{sep}", " ")));
        for (name, sep) in SEPARATORS {
            let unicode = clean_and_restore(&template.replace("{sep}", &sep.to_string()));
            assert_eq!(
                without_tokens(&unicode).replace(sep, " "),
                ascii,
                "{name}: {template:?}"
            );
        }
    }
}
