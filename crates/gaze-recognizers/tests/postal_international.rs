//! Regression fixtures for the `postal.ca`, `postal.gb`, and `postal.ie` core recognizers.
//!
//! EVERY positive fixture below encodes a shape MEASURED in the Dataiku EN/DE holdout gold, not a
//! format invented alongside the implementation. Before this rule set, `custom:postal_code` was
//! served only by `postal.de` (`locales = ["de-DE"]`) and `postal.us` (`locales = ["en-US"]`), both
//! `locale_basis = "document"`. Those two gates reach 596 of the 1,886 holdout documents, so the
//! other seven document locales had NO postal recognizer at all. Measured gold ZIP recall was
//! 334 of 1,090 entities — the gated population, to within 2 entities of incidental overlap.
//!
//! Measured gold shape distribution for the three locales this file covers
//! (`9` = digit, `A` = uppercase letter, `_` = space):
//!
//! | locale  | entities | gold bytes | shapes                                                     |
//! |---------|---------:|-----------:|------------------------------------------------------------|
//! | `en-CA` |       74 |        512 | `A9A_9A9` 71, `9{5}` 3                                      |
//! | `en-GB` |       85 |        588 | `A{2}9_9A{2}` 58, `A{2}9{2}_9A{2}` 11, `A9{2}_9A{2}` 10,     |
//! |         |          |            | `A{2}9` 3, `A{2}9{2}` 1, `9{5}` 1, `A9_9A{2}` 1             |
//! | `en-IE` |       78 |        569 | `A9{2}_A9A9` 39, `A9{2}_A{2}9{2}` 9, `9{5}` 7,              |
//! |         |          |            | `A9{2}_A9{3}` 5, `A9{2}_A9A{2}` 4, + 14 in a long tail      |
//!
//! Why these three and not the 4-digit locales: bare `\d{4}` (de-AT, de-CH, en-AU, en-NZ) is only
//! 19% precise on the holdout (516 of 2,723 runs are gold ZIP) and fires 1,717 times across 62.5%
//! of the 1,024-document A4 negative corpus. It is not shippable unanchored and is deliberately NOT
//! in this change. The three alphanumeric shapes below match ZERO of those 1,024 negative
//! documents, which is why they can ship without a cue requirement.
//!
//! Fixture values are synthetic and format-valid but are not real deliverable addresses:
//!
//! * Canada: `Z` is not an assigned first letter for Canadian postal codes.
//! * UK: `Z` is not an assigned postcode area letter, and `ZZ99 9ZZ` is the Royal Mail / ONS
//!   pseudo-postcode meaning "address not known".
//! * Ireland: `Y99` is not an assigned Eircode routing key.

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

/// The core bundle assembled through the real activation path with locale-gated auto-activation
/// OFF and only the caller's locales active.
///
/// `auto_activate_locale_gated = false` is deliberate and load-bearing: it is the weakest
/// configuration a default adopter can have. `postal.ca`, `postal.gb`, and `postal.ie` are declared
/// `locale_basis = "format"`, so `gaze_assembly::detector_wiring::recognizer_activates` admits them
/// without consulting the document chain, and `RecognizerRegistry::detect_all_resolved` runs them
/// before the per-locale fallback loop. If any of the three were declared `locale_basis =
/// "document"` instead, the `--locale=global` and cross-locale tests below would fail loudly rather
/// than a default adopter silently getting no postal protection outside de-DE and en-US.
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

/// Cleans under `global` only — the chain an adopter gets from `--locale=global`.
fn clean(text: &str) -> String {
    clean_in(&[LocaleTag::Global], text)
}

/// Asserts the WHOLE postal code is gone and the surrounding prose survives.
///
/// Whole-span coverage is the point. A fragment leaves restorable bytes on the wire and still
/// leaks the rest, which fails axis 1 just as a total miss does.
fn assert_postal_removed(text: &str, code: &str, surviving_context: &[&str]) {
    let cleaned = clean(text);
    assert!(
        !cleaned.contains(code),
        "postal code {code:?} survived tokenization in {cleaned:?}"
    );
    for fragment in surviving_context {
        assert!(
            cleaned.contains(fragment),
            "context {fragment:?} should survive but is missing from {cleaned:?}"
        );
    }
}

/// Asserts the value is still present verbatim after cleaning.
///
/// Deliberately narrower than `assert_eq!(clean(text), text)`: the surrounding prose may contain
/// other classes, and this file only makes claims about the postal recognizers.
fn assert_value_survives(text: &str, value: &str) {
    let cleaned = clean(text);
    assert!(
        cleaned.contains(value),
        "{value:?} must NOT be tokenized, but is missing from {cleaned:?}"
    );
}

// ======================================================= postal.ca — `A9A_9A9` (71 of 74 en-CA gold)

#[test]
fn ca_postal_code_with_space_is_tokenized() {
    assert_postal_removed(
        "Ship the replacement to Z1Z 9Z9 before Friday.",
        "Z1Z 9Z9",
        &["Ship the replacement to ", " before Friday."],
    );
}

#[test]
fn ca_postal_code_with_hyphen_is_tokenized() {
    assert_postal_removed(
        "Mailing code Z1Z-9Z9 is on file.",
        "Z1Z-9Z9",
        &["Mailing code ", " is on file."],
    );
}

#[test]
fn ca_postal_code_without_separator_is_tokenized() {
    assert_postal_removed(
        "Compact form Z1Z9Z9 appears in the export.",
        "Z1Z9Z9",
        &["Compact form ", " appears in the export."],
    );
}

// ======================================================= postal.gb — measured en-GB shape branches

#[test]
fn gb_standard_outward_two_letters_is_tokenized() {
    // `A{2}9_9A{2}` — 58 of 85 en-GB gold entities, the dominant shape.
    assert_postal_removed(
        "The registered office is at ZZ9 9ZZ in the filing.",
        "ZZ9 9ZZ",
        &["The registered office is at ", " in the filing."],
    );
}

#[test]
fn gb_two_letter_two_digit_outward_is_tokenized() {
    // `A{2}9{2}_9A{2}` — 11 of 85. `ZZ99 9ZZ` is the Royal Mail / ONS "address not known" code.
    assert_postal_removed(
        "Returns marked ZZ99 9ZZ are undeliverable.",
        "ZZ99 9ZZ",
        &["Returns marked ", " are undeliverable."],
    );
}

#[test]
fn gb_single_letter_two_digit_outward_is_tokenized() {
    // `A9{2}_9A{2}` — 10 of 85.
    assert_postal_removed(
        "Deliver to Z99 9ZZ on the second attempt.",
        "Z99 9ZZ",
        &["Deliver to ", " on the second attempt."],
    );
}

#[test]
fn gb_single_letter_single_digit_outward_is_tokenized() {
    // `A9_9A{2}` — 1 of 85.
    assert_postal_removed(
        "Archive entry Z9 9ZZ predates the move.",
        "Z9 9ZZ",
        &["Archive entry ", " predates the move."],
    );
}

#[test]
fn gb_alphanumeric_outward_is_tokenized() {
    // `A9A_9AA` — the London-style outward code. Not present in this holdout's gold, but it is one
    // of the six official Royal Mail forms, so the rule accepts it rather than leaking it.
    assert_postal_removed(
        "Central office Z9Z 9ZZ handles the account.",
        "Z9Z 9ZZ",
        &["Central office ", " handles the account."],
    );
}

#[test]
fn gb_outward_code_alone_is_deliberately_not_tokenized() {
    // `A{2}9` (3 entities) and `A{2}9{2}` (1) are outward-code-only golds — 4 of 85. They are NOT
    // covered: an isolated 1-2 letter + 1-2 digit token is indistinguishable from an ordinary
    // reference code, and covering it would trade 4 entities for a large false-positive surface.
    // Documented as a known, accepted gap rather than silently missed.
    assert_value_survives("The outward code ZZ9 is all we hold.", "ZZ9");
}

// ======================================================= postal.ie — Eircode shape branches

#[test]
fn ie_eircode_letter_digit_alternating_is_tokenized() {
    // `A9{2}_A9A9` — 39 of 78 en-IE gold entities, the dominant Eircode shape.
    assert_postal_removed(
        "The Eircode on record is Y99 X4X7 for that site.",
        "Y99 X4X7",
        &["The Eircode on record is ", " for that site."],
    );
}

#[test]
fn ie_eircode_two_letter_two_digit_identifier_is_tokenized() {
    // `A9{2}_A{2}9{2}` — 9 of 78.
    assert_postal_removed(
        "Site code Y99 XX47 was re-issued.",
        "Y99 XX47",
        &["Site code ", " was re-issued."],
    );
}

#[test]
fn ie_eircode_letter_three_digit_identifier_is_tokenized() {
    // `A9{2}_A9{3}` — 5 of 78.
    assert_postal_removed(
        "Correspondence to Y99 X475 was returned.",
        "Y99 X475",
        &["Correspondence to ", " was returned."],
    );
}

#[test]
fn ie_eircode_without_space_is_tokenized() {
    // `A9{2}A9A9` — 1 of 78, written without the conventional space.
    assert_postal_removed(
        "Compact Eircode Y99X4X7 in the CSV column.",
        "Y99X4X7",
        &["Compact Eircode ", " in the CSV column."],
    );
}

#[test]
fn ie_eircode_excluded_alphabet_is_not_matched() {
    // Eircode excludes B G I J L M O Q S U Z from the routing key and the unique identifier. That
    // restriction is what lets `postal.ie` score zero false positives on the A4 negative corpus, so
    // it is pinned: a token in Eircode POSITION but using an excluded letter is not an Eircode.
    assert_value_survives("Batch label B99 X4X7 is internal.", "B99 X4X7");
}

// ======================================================= negative fixtures: must stay untokenized
//
// Drawn from the categories in the committed A4 negative corpus
// (`crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl`): temporal_numeric,
// commerce_identifiers, invalid_identifiers, code_log_syntax.

#[test]
fn four_digit_years_are_not_postal_codes() {
    assert_value_survives("The policy was revised in 2024 and again in 1998.", "2024");
    assert_value_survives("The policy was revised in 2024 and again in 1998.", "1998");
}

#[test]
fn prices_are_not_postal_codes() {
    assert_value_survives("The quoted total came to 4500 GBP for the year.", "4500");
    assert_value_survives("Listed at 1250 CAD including delivery.", "1250");
}

#[test]
fn order_and_invoice_numbers_are_not_postal_codes() {
    assert_value_survives(
        "Order 12345 shipped, invoice 98765 is outstanding.",
        "12345",
    );
    assert_value_survives(
        "Order 12345 shipped, invoice 98765 is outstanding.",
        "98765",
    );
}

#[test]
fn bare_five_digit_numbers_are_not_tokenized_outside_de_de_and_en_us() {
    // `postal.us` stays `locale_basis = "document"`, `locales = ["en-US"]`. Widening its bare
    // 5-digit shape to every English locale was MEASURED and rejected: it buys 13 gold entities
    // (65 bytes) and costs 393 false positives across 254 of the 1,024 A4 negative documents.
    assert_value_survives("Reference 90210 appears in the ledger.", "90210");
}

#[test]
fn alphanumeric_reference_codes_are_not_postal_codes() {
    // Longer letter+digit runs must not be clipped into a postcode-shaped prefix or suffix.
    assert_value_survives("Asset tag ZZ99 9ZZQ is on the chassis.", "ZZ99 9ZZQ");
    assert_value_survives("Serial QZ1Z 9Z9 was superseded.", "QZ1Z 9Z9");
}

#[test]
fn version_and_build_strings_are_not_postal_codes() {
    assert_value_survives("Build 2024 of the agent shipped on time.", "2024");
    assert_value_survives("Rev 1Z9 of the schematic is current.", "1Z9");
}

/// The single MEASURED false positive this rule set introduces across 1,886 holdout documents.
///
/// Shape `A{2}9_9A{2}` — the canonical UK form — occurring in all-lowercase running prose with no
/// gold annotation of any label at that offset. It is accepted knowingly: the same branch carries
/// 58 of the 85 en-GB gold entities, and the surrounding prose context makes it at least as likely
/// to be an UNANNOTATED postcode in the gold as a true error. Axis 1 (never leak) beats one benign
/// reversible token. This test documents the behaviour so a future tightening is a deliberate
/// decision rather than an accident.
#[test]
fn documented_holdout_false_positive_shape_is_tokenized_knowingly() {
    assert_postal_removed(
        "the contact details listed show ZZ9 9ZZ alongside the other notes",
        "ZZ9 9ZZ",
        &[
            "the contact details listed show ",
            " alongside the other notes",
        ],
    );
}

// ======================================================= locale contract: format-basis everywhere

/// The core claim of this change: all three recognizers are `locale_basis = "format"`, so they run
/// for EVERY document locale, including `--locale=global` and including locales that have their own
/// postal rule. `RecognizerRegistry::detect_all_resolved` runs format-basis recognizers
/// unconditionally before the per-locale fallback loop, so they compose ADDITIVELY with the
/// document-basis `postal.de` / `postal.us` instead of racing them in the first-locale-wins loop.
#[test]
fn format_basis_rules_run_at_every_locale() {
    let cases: &[(&str, &str)] = &[("Z1Z 9Z9", "ca"), ("ZZ9 9ZZ", "gb"), ("Y99 X4X7", "ie")];
    let locales = [
        LocaleTag::Global,
        LocaleTag::DeDe,
        LocaleTag::DeAt,
        LocaleTag::DeCh,
        LocaleTag::EnUs,
        LocaleTag::EnGb,
        LocaleTag::EnIe,
        LocaleTag::EnAu,
        LocaleTag::EnCa,
        LocaleTag::parse("en-NZ").expect("en-NZ parses as LocaleTag::Other"),
    ];
    for locale in &locales {
        for (code, rule) in cases {
            let text = format!("Address line holds {code} in the record.");
            let cleaned = clean_in(std::slice::from_ref(locale), &text);
            assert!(
                !cleaned.contains(code),
                "postal.{rule} must fire at locale {} but {code:?} survived in {cleaned:?}",
                locale.as_str()
            );
        }
    }
}

/// The existing document-basis rules keep their gates. `postal.de` is `locales = ["de-DE"]`, so a
/// bare 5-digit number must still be tokenized under `de-DE` and left alone under `global`.
/// Pins that this change did not widen the numeric rules as a side effect.
#[test]
fn document_basis_numeric_rules_keep_their_locale_gates() {
    let de = clean_in(
        &[LocaleTag::DeDe],
        "Die Postleitzahl 10115 steht im Formular.",
    );
    assert!(
        !de.contains("10115"),
        "postal.de must still fire under de-DE, got {de:?}"
    );
    assert_value_survives("Die Postleitzahl 10115 steht im Formular.", "10115");
}

// ======================================================= restore

#[test]
fn international_postal_codes_restore_exactly() {
    let pipeline = pipeline_for(&[LocaleTag::Global]);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let original = "Canadian Z1Z 9Z9, British ZZ99 9ZZ, and Irish Y99 X4X7 on one line.";
    let (clean, _manifest, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(original.to_string()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let clean_text = match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    };
    assert!(!clean_text.contains("Z1Z 9Z9"));
    assert!(!clean_text.contains("ZZ99 9ZZ"));
    assert!(!clean_text.contains("Y99 X4X7"));
    let restored = pipeline
        .restore_strict_text(&session, &clean_text)
        .expect("restore");
    assert_eq!(restored, original, "manifest-first restore must round-trip");
}
