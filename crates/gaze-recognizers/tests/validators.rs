use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, PiiClass, Pipeline, RawDocument, Scope, Session,
};
#[cfg(not(feature = "phone-parser"))]
use gaze_recognizers::RecognizerError;
use gaze_recognizers::{NormalizerKind, RegexDetector, ValidatorKind};

fn validator_pipeline(
    pattern: &str,
    validator: ValidatorKind,
    normalizer: Option<NormalizerKind>,
) -> Pipeline {
    custom_validator_pipeline(
        pattern,
        PiiClass::custom("credit_card_or_iban"),
        validator,
        normalizer,
    )
}

fn custom_validator_pipeline(
    pattern: &str,
    class: PiiClass,
    validator: ValidatorKind,
    normalizer: Option<NormalizerKind>,
) -> Pipeline {
    Pipeline::builder()
        .recognizer(
            RegexDetector::with_rulepack_fields(
                pattern,
                class.clone(),
                "s1.validator.test",
                vec![gaze::LocaleTag::Global],
                0.90,
                50,
                "counter",
                None,
                Vec::new(),
                Some(validator),
                normalizer,
            )
            .expect("regex detector"),
        )
        .rule(ClassRule::new(class, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline")
}

fn clean_text(pipeline: &Pipeline, session: &Session, input: &str) -> String {
    let clean = pipeline
        .redact(session, RawDocument::Text(input.to_string()))
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };
    text
}

fn restore_tokens(session: &Session, clean: &str) -> String {
    gaze::token_shape::pattern()
        .replace_all(clean, |captures: &regex::Captures<'_>| {
            session.restore_strict(&captures[0]).expect("known token")
        })
        .to_string()
}

#[test]
fn luhn_algorithm_accepts_public_test_numbers_and_rejects_mutants() {
    for input in [
        "4111111111111111",
        "4012888888881881",
        "5555555555554444",
        "5105105105105100",
        "378282246310005",
        "371449635398431",
        "4111 1111 1111 1111",
    ] {
        assert!(ValidatorKind::Luhn.validates(input), "{input}");
    }

    for input in ["4111111111111112", "5555555555554445", "378282246310006"] {
        assert!(!ValidatorKind::Luhn.validates(input), "{input}");
    }
}

#[test]
fn iban_mod97_accepts_spec_examples_and_rejects_check_digit_mutants() {
    for input in [
        "BE71 0961 2345 6769",
        "DE89 3704 0044 0532 0130 00",
        "FR14 2004 1010 0505 0001 3M02 606",
        "GB82 WEST 1234 5698 7654 32",
    ] {
        assert!(ValidatorKind::IbanMod97.validates(input), "{input}");
    }

    for input in ["GB99WEST12345698765432", "DE99370400440532013000"] {
        assert!(!ValidatorKind::IbanMod97.validates(input), "{input}");
    }
}

#[test]
fn ipv4_parse_accepts_rfc5737_documentation_addresses_and_rejects_malformed() {
    // Source: RFC 5737 documentation prefixes 192.0.2.0/24,
    // 198.51.100.0/24, and 203.0.113.0/24.
    for input in [
        "192.0.2.1",
        "198.51.100.10",
        "203.0.113.255",
        "0.0.0.0",
        "255.255.255.255",
    ] {
        assert!(ValidatorKind::Ipv4Parse.validates(input), "{input}");
    }
    for input in [
        // Source: RFC 6943 Section 3.1.1 warns against non-decimal IPv4 forms.
        "192.000.2.1",
        "01.2.3.4",
        "0xC0.0x00.0x02.0x01",
        "192.168.1",
        "192.168",
        "256.0.0.1",
        "192.0.2.300",
        "192.0.2.1.",
        " 192.0.2.1",
        "",
    ] {
        assert!(!ValidatorKind::Ipv4Parse.validates(input), "{input}");
    }
}

#[test]
fn ipv4_parse_kind_token_round_trips() {
    let kind = ValidatorKind::parse("ipv4_parse").expect("parse ipv4_parse");
    assert_eq!(kind, ValidatorKind::Ipv4Parse);
}

#[test]
fn ipv6_parse_accepts_rfc3849_documentation_addresses_and_ipv4_embedded() {
    // Source: RFC 3849 documentation prefix 2001:db8::/32 and RFC 4291
    // Section 2.2 textual forms, including IPv4-embedded form 3.
    for input in [
        "2001:db8::1",
        "::1",
        "::",
        "fe80::1",
        "2001:db8:0:0:0:0:0:1",
        "::ffff:192.0.2.128",
        "::192.0.2.1",
        "2001:db8::192.0.2.1",
        "fe80::ffff:1.2.3.4",
        "0:0:0:0:0:ffff:192.0.2.1",
    ] {
        assert!(ValidatorKind::Ipv6Parse.validates(input), "{input}");
    }
    for input in [
        "2001::1::2",
        "[2001:db8::1]",
        "fe80::1%eth0",
        "::ffff:256.0.0.1",
        "",
        " 2001:db8::1",
    ] {
        assert!(!ValidatorKind::Ipv6Parse.validates(input), "{input}");
    }
}

#[test]
fn ipv6_parse_kind_token_round_trips() {
    let kind = ValidatorKind::parse("ipv6_parse").expect("parse ipv6_parse");
    assert_eq!(kind, ValidatorKind::Ipv6Parse);
}

#[cfg(not(feature = "phone-parser"))]
#[test]
fn e164_phone_fails_closed_when_phone_parser_feature_disabled() {
    let result = ValidatorKind::parse("e164_phone");

    assert!(
        matches!(
            result,
            Err(RecognizerError::UnsupportedValidator { ref kind }) if kind == "e164_phone"
        ),
        "feature-disabled build MUST reject e164_phone with UnsupportedValidator (axis-1 fail-closed); got: {result:?}"
    );
}

#[cfg(feature = "phone-parser")]
#[test]
fn e164_phone_accepts_assigned_international_number_and_rejects_unassigned_prefix() {
    assert!(ValidatorKind::E164Phone.validates("+4915550112233"));
    assert!(!ValidatorKind::E164Phone.validates("+99999999"));
    assert!(!ValidatorKind::E164Phone.validates("4915550112233"));
}

#[test]
fn iban_canonical_is_uppercase_whitespace_free_and_idempotent() {
    let canonical = NormalizerKind::IbanCanonical.normalize("gb82 west 1234 5698 7654 32");

    assert_eq!(canonical, "GB82WEST12345698765432");
    assert_eq!(
        NormalizerKind::IbanCanonical.normalize(&canonical),
        canonical
    );
}

#[cfg(feature = "phone-parser")]
#[test]
fn s3a_e164_phone_passing_candidate_emits_detection_and_round_trips() {
    let pipeline = custom_validator_pipeline(
        r"\+\d{6,15}\b",
        PiiClass::custom("phone"),
        ValidatorKind::E164Phone,
        None,
    );
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "Phone: +4915550112233";
    let clean = clean_text(&pipeline, &session, input);

    assert!(clean.starts_with("Phone: <"), "{clean}");
    assert!(clean.ends_with(":Custom:phone_1>"), "{clean}");
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[cfg(feature = "phone-parser")]
#[test]
fn s3a_e164_phone_unassigned_candidate_emits_no_detection() {
    let pipeline = custom_validator_pipeline(
        r"\+\d{6,15}\b",
        PiiClass::custom("phone"),
        ValidatorKind::E164Phone,
        None,
    );
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "Phone: +99999999";
    let clean = clean_text(&pipeline, &session, input);

    assert_eq!(clean, input);
}

#[test]
fn s1_luhn_passing_card_emits_detection() {
    let pipeline = validator_pipeline(r"\b\d{16}\b", ValidatorKind::Luhn, None);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "Card: 4111111111111111";
    let clean = clean_text(&pipeline, &session, input);

    assert!(clean.starts_with("Card: <"), "{clean}");
    assert!(clean.ends_with(":Custom:credit_card_or_iban_1>"), "{clean}");
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[test]
fn s1_luhn_failing_card_emits_no_detection() {
    // ADVERSARIAL: deleting the validator_kind field-read in
    // RegexDetector::canonical_form MUST cause s1_luhn_failing_card_emits_no_detection
    // to FAIL (validator-failing candidate would slip through). This proves the
    // data-flow gate is exercised, not just symbol-present.
    let pipeline = validator_pipeline(r"\b\d{16}\b", ValidatorKind::Luhn, None);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "Card: 4111111111111112";
    let clean = clean_text(&pipeline, &session, input);

    assert_eq!(clean, input);
}

#[test]
fn s1_iban_mod97_passing_iban_emits_detection() {
    let pipeline = validator_pipeline(
        r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b",
        ValidatorKind::IbanMod97,
        Some(NormalizerKind::IbanCanonical),
    );
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "IBAN: GB82WEST12345698765432";
    let clean = clean_text(&pipeline, &session, input);

    assert!(clean.starts_with("IBAN: <"), "{clean}");
    assert!(clean.ends_with(":Custom:credit_card_or_iban_1>"), "{clean}");
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[test]
fn s1_iban_mod97_failing_iban_emits_no_detection() {
    let pipeline = validator_pipeline(
        r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b",
        ValidatorKind::IbanMod97,
        Some(NormalizerKind::IbanCanonical),
    );
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "IBAN: GB99WEST12345698765432";
    let clean = clean_text(&pipeline, &session, input);

    assert_eq!(clean, input);
}
