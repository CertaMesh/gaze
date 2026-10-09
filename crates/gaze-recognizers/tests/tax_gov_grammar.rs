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
        ("My TIN is 00-1234567.", "00-1234567", "tax_number"),
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
            "steuer_id",
        ),
        ("SSN: 000 12 3456", "000 12 3456", "ssn"),
        ("SSN: 00012-3456", "00012-3456", "ssn"),
        ("SSN: 000-123456", "000-123456", "ssn"),
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
        (
            "ID card: A0123456789A0123456789A0123456789A0123456789",
            "A0123456789A0123456789A0123456789A0123456789",
            "national_id",
        ),
        (
            "Umsatzsteuer-ID: DE 000 123 456",
            "DE 000 123 456",
            "vat_id",
        ),
        ("VAT ID: DE000.123.456", "DE000.123.456", "vat_id"),
        ("USt-IdNr.: DE-000-123-456", "DE-000-123-456", "vat_id"),
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
fn an_identifier_suffix_after_an_internal_validated_card_stays_protected() {
    let pipeline = pipeline();
    for input in [
        "ID card: AB00 4111 1111 1111 1111 CD00",
        "ID card: AB00 4111 1111 1111 1111 CD00 EF01",
        "ID card: AB00 4111 1111 1111 1111 CD00; status=synthetic",
    ] {
        let cleaned = clean(&pipeline, input);
        let raw = without_tokens(&cleaned);
        assert!(!raw.contains("CD00"), "{input:?} -> {cleaned:?}");
        assert!(
            !raw.chars().any(|c| c.is_ascii_digit()),
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
        "Tax rate: 00.1234567%",
        "SKU: ZZ001234567",
        "national ID checked; batch reference: 000123Z4567",
        "VAT ID checked; vehicle ID: DE 000 123 456",
    ] {
        assert_eq!(clean(&pipeline, input), input, "{input:?}");
    }
}

#[test]
fn bare_nino_requires_the_constrained_uppercase_shape() {
    let pipeline = pipeline();
    // The zero sequence is synthetic, not an assigned account number.
    for value in ["AB 00 00 00 C", "AB000000C"] {
        let input = format!("The identifier is {value}.");
        let cleaned = clean(&pipeline, &input);
        assert!(cleaned.contains(":Custom:nino_1>"), "{cleaned:?}");
        assert_eq!(without_tokens(&cleaned), input.replace(value, "\0"));
    }
    for input in [
        "ab000000c",
        "BG000000C",
        "GB000000C",
        "KN000000C",
        "NK000000C",
        "NT000000C",
        "TN000000C",
        "ZZ000000C",
        "AB000000E",
        "AB0000000C",
        "XAB000000C",
        "AB000000CX",
        "AB\n00\n00\n00\nC",
    ] {
        assert_eq!(clean(&pipeline, input), input, "{input:?}");
    }
}
