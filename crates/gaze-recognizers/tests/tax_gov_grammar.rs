//! Synthetic identifier shapes under explicit cues and benign lookalikes.

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, Pipeline,
    RawDocument, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

fn pipeline() -> Pipeline {
    let mut policy = gaze::Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let context = Context {
        dictionaries: Default::default(),
        class_map: Default::default(),
        fields: Default::default(),
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    };
    let pack = Rulepack::load(RulepackSource::Embedded(embedded("core").unwrap())).unwrap();
    let chain =
        LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(&[LocaleTag::Global]));
    gaze_assembly::build_pipeline(&policy, &context, &[pack], &chain, None).unwrap()
}

fn clean(pipeline: &Pipeline, input: &str) -> String {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(input.into()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .unwrap();
    let CleanDocument::Text(clean) = clean else {
        panic!("text expected")
    };
    assert_eq!(
        pipeline.restore_strict_text(&session, &clean).unwrap(),
        input
    );
    clean
}

#[test]
fn tax_and_government_values_are_whole_tokens_under_direct_and_connector_cues() {
    let pipeline = pipeline();
    for (input, value, class) in [
        (
            "Steueridentifikationsnummer lautet 00-123-456-789.",
            "00-123-456-789",
            "steuer_id",
        ),
        (
            "steuerliche Identifikationsnummer ist hinterlegt als 00 123 456 789",
            "00 123 456 789",
            "steuer_id",
        ),
        ("Steuernummer: 00/123/45678", "00/123/45678", "tax_number"),
        (
            "Steuernummer lautet 000/1234/56789",
            "000/1234/56789",
            "tax_number",
        ),
        ("TIN: AB000000000", "AB000000000", "tax_number"),
        ("tax number: AB 000000000", "AB 000000000", "tax_number"),
        ("TIN: AB000000000CD", "AB000000000CD", "tax_number"),
        (
            "numéro fiscal numéro est AB000123",
            "AB000123",
            "tax_number",
        ),
        ("tax number número est AB000123", "AB000123", "tax_number"),
        (
            "tax identification number is recorded as 001234567",
            "001234567",
            "tax_number",
        ),
        (
            "taxpayer identification number: 00123456789",
            "00123456789",
            "tax_number",
        ),
        ("SSN: 000 12 3456", "000 12 3456", "ssn"),
        (
            "social security number (SSN): 000.12.3456",
            "000.12.3456",
            "ssn",
        ),
        ("AHV number: 000-1234-5678-90", "000-1234-5678-90", "ssn"),
        (
            "social insurance number is 000.1234.5678.90",
            "000.1234.5678.90",
            "ssn",
        ),
        (
            "national identification number: 000123Z4567",
            "000123Z4567",
            "national_id",
        ),
        (
            "national ID number: 00 123 456 7",
            "00 123 456 7",
            "national_id",
        ),
        ("identity card number: 0001234Z", "0001234Z", "national_id"),
    ] {
        let cleaned = clean(&pipeline, input);
        assert!(
            cleaned.contains(&format!(":Custom:{class}_1>")),
            "{input:?} -> {cleaned:?}"
        );
        assert_eq!(
            without_tokens(&cleaned),
            input.replace(value, "\0"),
            "{input:?} -> {cleaned:?}"
        );
    }
}

#[test]
fn nearby_government_cues_do_not_claim_dates_amounts_or_separate_fields() {
    let pipeline = pipeline();
    for input in [
        "Steuer-ID verified; vehicle ID: 00123456789",
        "TIN checked; invoice amount: 001234567 EUR",
        "Tax number: 2026-10-09",
        "TIN: 000000000",
        "Tax rate: 00.1234567%",
        "SKU: ZZ001234567",
        "national ID checked; batch reference: 000123Z4567",
        "VAT ID checked; vehicle ID: DE 000 123 456",
    ] {
        assert_eq!(clean(&pipeline, input), input, "{input:?}");
    }
}

#[test]
fn generic_tax_identification_fields_protect_uncued_repeated_values() {
    let pipeline = pipeline();
    for (key, value) in [
        ("taxIdentificationNumber", "00 000 000 001"),
        ("taxpayer ID", "00 000 000 001"),
        ("taxpayer_id", "00 000 000 001"),
        ("tax payer ID", "00 000 000 001"),
        ("tax ID", "00 000 000 001"),
        ("taxpayer identification number", "00123456789"),
    ] {
        for connector in [": ", " is ", " lautet ", " est "] {
            let input = format!("{key}{connector}{value}; repeat: {value}");
            let cleaned = clean(&pipeline, &input);
            assert_eq!(without_tokens(&cleaned), input.replace(value, "\0"));
            assert_eq!(cleaned.matches(":Custom:tax_number_1>").count(), 2);
        }
    }
}

#[test]
fn compact_german_identifiers_keep_trusted_repeat_ownership() {
    let pipeline = pipeline();
    for input in [
        "Steuer-ID number 00123456789; repeat: 00123456789",
        "steuerliche Identifikationsnummer lautet 00123456789; repeat: 00123456789",
        "Identifikationsnummer is 00123456789; repeat: 00123456789",
    ] {
        let cleaned = clean(&pipeline, input);
        assert_eq!(without_tokens(&cleaned), input.replace("00123456789", "\0"));
        assert_eq!(cleaned.matches(":Custom:national_id_1>").count(), 2);
    }
}

#[test]
fn generic_id_cues_do_not_turn_short_numeric_groups_into_national_ids() {
    let pipeline = pipeline();
    for input in [
        "Order ID number 12 345 678",
        "Order ID number 1 2 3",
        "Customer ID number 2024 10 15",
        "Product ID number 12.345.678 EUR",
        "ID card 1/2/3",
        "Invoice id number 4711/2024/03",
        "ID number 3-4-5 triangle",
    ] {
        assert_eq!(clean(&pipeline, input), input, "{input:?}");
    }
    // The IP recognizer retains ownership instead of a generic ID cue.
    let cleaned = clean(&pipeline, "ID number 10.0.0.1");
    assert!(cleaned.contains(":Custom:ip_address_1>"), "{cleaned:?}");
    assert!(!cleaned.contains(":Custom:national_id_"), "{cleaned:?}");
}
