//! Regression fixtures for the `postal.at_ch` core recognizer: anchored four-digit postal codes
//! for Austrian (`de-AT`) and Swiss (`de-CH`) documents.
//!
//! Measured gold shape distribution in the Dataiku EN/DE holdout (`9` = digit, `A` = uppercase
//! letter):
//!
//! | locale  | entities | gold bytes | shapes                     |
//! |---------|---------:|-----------:|----------------------------|
//! | `de-AT` |      184 |        736 | `9{4}` 184                 |
//! | `de-CH` |      182 |        734 | `9{4}` 180, `A{2}-9{4}` 2  |
//!
//! A bare four-digit string carries no structural signal. Unanchored `\b\d{4}\b` is 19% precise
//! on the holdout and fires across 62.5% of the A4 negative corpus, so ALL precision here comes
//! from one of two anchors: a postal cue directly before the code (`PLZ`, `Postleitzahl`,
//! `Postcode`, `ZIP`), or a city-shaped token directly after it (`4020 Musterstadt`), which is
//! how German-language addresses in both countries are written.
//!
//! Fixture values are synthetic. Every town name is invented, so no fixture is a deliverable
//! address even where the four digits are an assigned code.

use gaze::Context;
use gaze::{
    Action, CleanDocument, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    RawDocument, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

fn postal_class() -> PiiClass {
    PiiClass::custom("postal_code").expect("valid custom class")
}

/// The core bundle through the real activation path, locale-gated auto-activation OFF and only
/// the caller's chain active, so a rule runs only where its declared locales intersect the chain.
fn pipeline_for(locales: &[LocaleTag]) -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: postal_class(),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = false;
    let chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(locales));
    gaze_assembly::build_pipeline(&policy, &empty_context(), &[rulepack], &chain, None)
        .expect("pipeline")
}

fn clean_in(locales: &[LocaleTag], text: &str) -> String {
    let pipeline = pipeline_for(locales);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
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

const AT_CH: [LocaleTag; 2] = [LocaleTag::DeAt, LocaleTag::DeCh];

/// Asserts the WHOLE code is gone and the named context survives, under BOTH de-AT and de-CH.
fn assert_removed(text: &str, code: &str, surviving_context: &[&str]) {
    for locale in &AT_CH {
        let cleaned = clean_in(std::slice::from_ref(locale), text);
        assert!(
            !cleaned.contains(code),
            "{}: postal code {code:?} survived in {cleaned:?}",
            locale.as_str()
        );
        for fragment in surviving_context {
            assert!(
                cleaned.contains(fragment),
                "{}: context {fragment:?} should survive but is missing from {cleaned:?}",
                locale.as_str()
            );
        }
    }
}

/// Asserts the value survives verbatim under BOTH de-AT and de-CH.
fn assert_survives(text: &str, value: &str) {
    for locale in &AT_CH {
        let cleaned = clean_in(std::slice::from_ref(locale), text);
        assert!(
            cleaned.contains(value),
            "{}: {value:?} must NOT be tokenized, but is missing from {cleaned:?}",
            locale.as_str()
        );
    }
}

// ======================================================= city anchor: code, then a city-shaped token

#[test]
fn code_before_city_after_a_street_line_is_tokenized() {
    assert_removed(
        "Bitte liefern an Musterweg 3, 4020 Musterstadt.",
        "4020",
        &["Bitte liefern an Musterweg 3, ", " Musterstadt."],
    );
}

#[test]
fn code_before_city_without_a_comma_after_the_house_number_is_tokenized() {
    // Single-line addresses often drop the comma. The code is preceded by a digit group here, so
    // a guard refusing "digit, space, code" would leak this shape; the rule deliberately has none.
    assert_removed(
        "Adresse: Musterweg 12 4020 Musterstadt",
        "4020",
        &["Musterweg 12 ", " Musterstadt"],
    );
}

#[test]
fn code_at_the_start_of_the_input_and_of_a_line_is_tokenized() {
    assert_removed(
        "8999 Musterdorf ist der Lieferort.",
        "8999",
        &[" Musterdorf"],
    );
    assert_removed(
        "Max Beispiel\nMusterweg 3\n8999 Musterdorf",
        "8999",
        &["Musterweg 3\n", " Musterdorf"],
    );
}

#[test]
fn code_then_comma_then_city_is_tokenized() {
    assert_removed("Ort: 4020, Musterstadt", "4020", &[", Musterstadt"]);
}

#[test]
fn nbsp_narrow_nbsp_and_repeated_spaces_before_the_city_are_tokenized() {
    // The PR #598 / #450 lesson: text pasted out of PDFs and web pages carries NBSP separators.
    for sep in ["\u{00A0}", "\u{202F}", "  ", "   "] {
        let text = format!("Lieferung nach 4020{sep}Musterstadt heute.");
        assert_removed(&text, "4020", &["Musterstadt"]);
    }
}

#[test]
fn all_caps_umlaut_and_accented_city_names_are_tokenized() {
    assert_removed("4020 MUSTERSTADT", "4020", &["MUSTERSTADT"]);
    assert_removed("Sitz in 5999 Überbeispiel", "5999", &["Überbeispiel"]);
    assert_removed("Sitz in 1999 Émonville VS", "1999", &["Émonville"]);
}

#[test]
fn sankt_abbreviation_city_is_tokenized() {
    // `St.` is two letters and a dot, shorter than the three-letter city token, so it has its own
    // branch. `St. Pölten`- and `St. Gallen`-shaped addresses leaked without it.
    assert_removed("9999 St. Musterkirchen", "9999", &[" St. Musterkirchen"]);
    assert_removed("9999 St.Musterkirchen", "9999", &[" St.Musterkirchen"]);
    assert_removed("9999 St Musterkirchen", "9999", &[" St Musterkirchen"]);
}

#[test]
fn a_bare_st_abbreviation_is_not_a_city() {
    // `St.` alone is also the German abbreviation for "Stück" (pieces): `1500 St. geliefert` is an
    // invoice quantity. The `St` branch therefore requires a capitalised name right after it.
    assert_survives("Es wurden 1500 St. geliefert.", "1500");
    assert_survives("Menge: 1500 St. à 3 CHF", "1500");
}

#[test]
fn country_prefixed_codes_are_tokenized_whole() {
    // `A{2}-9{4}` is 2 of the 182 de-CH gold entities; the prefix is part of the gold span.
    // The prefix itself must go too: asserting only that `CH-8999` is absent passes when just the
    // digits are tokenized and `CH-` is left on the wire.
    for (text, prefix, code) in [
        ("CH-8999 Musterdorf", "CH-", "8999"),
        ("A-4020 Musterstadt", "A-", "4020"),
        ("FL-9499 Musterdorf", "FL-", "9499"),
    ] {
        assert_removed(text, prefix, &[" Muster"]);
        assert_removed(text, code, &[" Muster"]);
    }
}

#[test]
fn year_shaped_codes_are_not_excluded() {
    // 1900-2099 are assigned postal ranges in both countries (Lower Austria 2xxx, Valais and
    // Neuchâtel 19xx/20xx). Excluding them was measured: it removes ONE holdout false positive and
    // would leak every real code in those ranges, so axis 1 keeps them.
    assert_removed("2000 Musterau an der Donau", "2000", &[" Musterau"]);
}

// ======================================================= cue anchor: postal cue, then the code

#[test]
fn cue_before_the_code_is_tokenized_without_a_city() {
    for text in [
        "PLZ: 4020 bitte prüfen",
        "Postleitzahl 4020 wurde bestätigt",
        "postleitzahl: 4020.",
        "Postcode 4020 ist korrekt",
        "ZIP 4020",
        "Zip code: 4020",
        "PLZ\u{00A0}4020 stimmt",
    ] {
        assert_removed(text, "4020", &[]);
    }
}

#[test]
fn cue_before_a_country_prefixed_code_is_tokenized_whole() {
    assert_removed(
        "PLZ: CH-8999 laut Formular",
        "CH-",
        &["PLZ: ", " laut Formular"],
    );
    assert_removed(
        "PLZ: CH-8999 laut Formular",
        "8999",
        &["PLZ: ", " laut Formular"],
    );
}

// ======================================================= precision: must stay untokenized

#[test]
fn a_four_digit_number_with_no_anchor_survives() {
    // RED if the anchor is dropped.
    assert_survives("Rechnung 4711 ist noch offen.", "4711");
    assert_survives("Im Jahr 2024 stieg der Umsatz deutlich.", "2024");
    assert_survives("Es wurden 1500 bestellt und 1200 geliefert.", "1500");
}

#[test]
fn digit_runs_longer_than_four_are_never_split() {
    // RED if the code is widened to five digits (`10115` would then match whole) or if `\b` is
    // dropped (a four-digit tail would be carved out of the longer run).
    assert_survives("10115 Musterberg", "10115");
    assert_survives("Auftrag 123456 Musterstadt", "123456");
    assert_survives("PLZ 10115", "10115");
}

#[test]
fn digit_runs_shorter_than_four_are_never_codes() {
    // RED if either branch is widened to `\d{3,4}`: a three-digit number before a capitalised
    // word (amounts, page numbers) would then join the four-digit false-positive class unseen.
    // City branch:
    assert_survives("Lieferung nach 123 Wien", "123");
    assert_survives("Lieferung nach A-123 Wien", "A-123");
    assert_survives("Es kostet 100 Euro netto.", "100");
    assert_survives("Siehe Seite 250 Kapitel drei.", "250");
    // Cue branch:
    assert_survives("PLZ 123 fehlt noch", "PLZ 123");
    assert_survives("PLZ: 123", "123");
}

#[test]
fn spaced_iban_and_phone_digit_groups_survive() {
    // Each group is followed by another digit group, never by a city-shaped token.
    let iban = "IBAN AT61 1904 3002 3457 3201 bitte verwenden.";
    for group in ["1904", "3002", "3457", "3201"] {
        assert_survives(iban, group);
    }
    let phone = "Telefon +43 1 2345 6789 erreichbar.";
    assert_survives(phone, "2345");
    assert_survives(phone, "6789");
}

#[test]
fn issue_and_ticket_references_are_not_postal_codes() {
    // RED if the `#` guard on the city branch is dropped: a capitalised German noun after an issue
    // number is otherwise indistinguishable from a town.
    assert_survives("Commit behebt #4711 Fehler im Export.", "#4711");
    assert_survives("Siehe Ticket #2024 Änderungen.", "#2024");
}

#[test]
fn a_cue_separated_from_the_code_by_other_words_does_not_anchor() {
    assert_survives("Die PLZ steht in Zeile 4711 unten.", "4711");
}

#[test]
fn known_cost_capitalized_nouns_after_a_four_digit_number_are_tokenized() {
    // Documented, accepted cost. German capitalises every noun, so `1500 Euro` and
    // `3000 Mitarbeiter` are indistinguishable from `1500 Musterstadt` to a city-shape anchor.
    // The `regex` crate has no negative lookahead, so a stop-list cannot be expressed in the
    // pattern. Measured on the holdout this class is part of the 28 false positives disclosed in
    // the CHANGELOG. Every such token restores losslessly. Pinned so a future tightening is a
    // deliberate, measured change rather than an accident.
    let cleaned = clean_in(&[LocaleTag::DeAt], "Kosten 1500 Euro pro Jahr.");
    assert!(!cleaned.contains("1500"), "{cleaned:?}");
}

// ======================================================= locale contract

#[test]
fn the_rule_is_gated_to_de_at_and_de_ch_documents() {
    // RED if the locale gate is dropped or widened. de-DE matters most: German documents must not
    // gain four-digit tokens.
    let text = "Musterweg 3, 4020 Musterstadt, PLZ 8999";
    for locale in [
        LocaleTag::DeDe,
        LocaleTag::Global,
        LocaleTag::EnUs,
        LocaleTag::EnGb,
        LocaleTag::EnAu,
        LocaleTag::parse("en-NZ").expect("en-NZ parses"),
    ] {
        let cleaned = clean_in(std::slice::from_ref(&locale), text);
        assert_eq!(
            cleaned,
            text,
            "{}: no four-digit postal token outside de-AT/de-CH",
            locale.as_str()
        );
    }
}

/// A locale chain paired with the later-locale `(text, code)` fragments it must still tokenize.
type ChainCase<'a> = (&'a [LocaleTag], &'a [(&'a str, &'a str)]);

/// `custom:postal_code` document-basis rules resolve per span across the locale chain
/// (`RecognizerRegistry::detect_candidate_pool`): an earlier locale wins only where its candidates
/// overlap a later locale's. A four-digit match at `de-AT` / `de-CH`, true or false, must not
/// switch off `postal.de` / `postal.us` for the rest of the document. Each trigger is asserted
/// tokenized first, so the later-locale assertion cannot pass vacuously.
#[test]
fn mixed_country_document_tokenizes_four_and_five_digit_codes_under_every_chain() {
    let triggers = [
        ("1500 Euro", "1500"),
        ("4020 Musterstadt", "4020"),
        ("CH-8001 Musterstadt", "CH-8001"),
        ("PLZ 1010", "1010"),
    ];
    let german = [
        ("10115 Musterberg", "10115"),
        ("D-10115", "10115"),
        ("PLZ 80331", "80331"),
    ];
    let us = [("Springfield, IL 90210", "90210")];
    let chains: [ChainCase; 3] = [
        (&[LocaleTag::DeAt, LocaleTag::DeDe], &german),
        (&[LocaleTag::DeCh, LocaleTag::DeDe], &german),
        (&[LocaleTag::DeAt, LocaleTag::EnUs], &us),
    ];
    for (chain, later) in chains {
        let names: Vec<&str> = chain.iter().map(LocaleTag::as_str).collect();
        for (trigger, trigger_code) in triggers {
            for (later_text, later_code) in later {
                let doc = format!("Kosten laut Anlage: {trigger}. Lieferadresse: {later_text}.");
                let cleaned = clean_in(chain, &doc);
                assert!(
                    !cleaned.contains(trigger_code),
                    "{names:?} {doc:?}: four-digit trigger must be tokenized: {cleaned:?}"
                );
                assert!(
                    !cleaned.contains(later_code),
                    "{names:?} {doc:?}: later-locale code must still be tokenized: {cleaned:?}"
                );
            }
        }
    }
}

/// Single-locale chains keep each rule to its own shape, and the reversed chain behaves the same
/// as the forward one.
#[test]
fn locale_chain_order_does_not_change_the_postal_token_set() {
    let mixed = "Wien: 4020 Musterstadt. Berlin: 10115 Musterberg.";

    // de-AT only: the German five-digit code is not claimed by this rule and is never split
    // into a four-digit fragment.
    let at = clean_in(&[LocaleTag::DeAt], mixed);
    assert!(!at.contains("4020"), "{at:?}");
    assert!(at.contains("10115"), "{at:?}");

    // de-DE only: exactly the pre-existing behaviour.
    let de = clean_in(&[LocaleTag::DeDe], mixed);
    assert!(de.contains("4020"), "{de:?}");
    assert!(!de.contains("10115"), "{de:?}");

    for chain in [
        [LocaleTag::DeAt, LocaleTag::DeDe],
        [LocaleTag::DeDe, LocaleTag::DeAt],
    ] {
        let both = clean_in(&chain, mixed);
        assert!(!both.contains("4020"), "{chain:?} {both:?}");
        assert!(!both.contains("10115"), "{chain:?} {both:?}");
    }
}

// ======================================================= restore

#[test]
fn anchored_four_digit_codes_restore_exactly() {
    for locale in AT_CH {
        let pipeline = pipeline_for(std::slice::from_ref(&locale));
        let session = Session::new(Scope::Ephemeral).expect("session");
        let original = "PLZ: CH-8999, sonst Musterweg 3, 4020\u{00A0}Musterstadt.";
        let (clean, _manifest, _) = pipeline
            .clean_with_safety_net_detect_context(
                &session,
                RawDocument::Text(original.to_string()),
                std::slice::from_ref(&locale),
                &DictionaryBundle::default(),
            )
            .expect("clean");
        let clean_text = match clean {
            CleanDocument::Text(text) => text,
            _ => panic!("expected text"),
        };
        assert!(!clean_text.contains("8999"), "{clean_text:?}");
        assert!(!clean_text.contains("4020"), "{clean_text:?}");
        let restored = pipeline
            .restore_strict_text(&session, &clean_text)
            .expect("restore");
        assert_eq!(restored, original, "manifest-first restore must round-trip");
    }
}
