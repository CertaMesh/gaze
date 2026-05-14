use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, PiiClass, Pipeline, RawDocument, Scope, Session,
};
#[cfg(not(feature = "phone-parser"))]
use gaze_recognizers::RecognizerError;
use gaze_recognizers::{NormalizerKind, RegexDetector, ValidatorKind};
use sha3::{Digest, Sha3_256};

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

#[test]
fn eth_eip55_accepts_spec_vectors_and_rejects_mutants() {
    // Source: EIP-55 "Test Cases".
    for input in [
        "0x52908400098527886E0F7030069857D2E4169EE7",
        "0x8617E340B3D01FA5F11F306F4090FD50E238070D",
        "0xde709f2102306220921060314715629080e2fb77",
        "0x27b1fdb04752bbc536007a920d24acb045561c26",
        "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
        "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
        "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
        "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
    ] {
        assert!(ValidatorKind::EthEip55.validates(input), "{input}");
    }

    for input in [
        "0x52908400098527886e0F7030069857D2E4169EE7",
        "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d358",
        "52908400098527886E0F7030069857D2E4169EE7",
        "0x52908400098527886E0F7030069857D2E4169EE",
        "0x52908400098527886E0F7030069857D2E4169EE7Z",
        "",
    ] {
        assert!(!ValidatorKind::EthEip55.validates(input), "{input}");
    }
}

#[test]
fn eth_eip55_kind_token_round_trips() {
    let kind = ValidatorKind::parse("eth_eip55").expect("parse eth_eip55");
    assert_eq!(kind, ValidatorKind::EthEip55);
}

#[test]
fn eth_eip55_spec_vectors_fail_under_nist_sha3_256() {
    // Source: EIP-55 "Test Cases"; NIST SHA3-256 uses different padding than
    // Keccak-256 and must not be substituted for the checksum hash.
    for input in [
        "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
        "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
        "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
        "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
    ] {
        assert!(!eth_eip55_check_with_nist_sha3_256(input), "{input}");
    }
}

fn eth_eip55_check_with_nist_sha3_256(input: &str) -> bool {
    let Some(address) = input.strip_prefix("0x") else {
        return false;
    };
    if address.len() != 40 || !address.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    let lowercase = address.to_ascii_lowercase();
    let hash = Sha3_256::digest(lowercase.as_bytes());
    for (index, byte) in address.bytes().enumerate() {
        if byte.is_ascii_digit() {
            continue;
        }
        let hash_nibble = if index % 2 == 0 {
            hash[index / 2] >> 4
        } else {
            hash[index / 2] & 0x0f
        };
        if (hash_nibble > 7) != byte.is_ascii_uppercase() {
            return false;
        }
    }
    true
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

#[test]
fn aadhaar_verhoeff_accepts_generated_values_and_rejects_mutants() {
    assert!(ValidatorKind::AadhaarVerhoeff.validates("2345 6789 0124"));

    let values = (20..30).map(gen_aadhaar).collect::<Vec<_>>();
    for value in &values {
        assert!(ValidatorKind::AadhaarVerhoeff.validates(value), "{value}");
        assert!(gaze_recognizers::validators::verhoeff::validate_aadhaar(
            value
        ));
        assert!(!ValidatorKind::AadhaarVerhoeff.validates(&flip_last_digit(value)));
    }

    assert_validator_pipeline_round_trip(
        r"\b\d{4} \d{4} \d{4}\b",
        ValidatorKind::AadhaarVerhoeff,
        "Aadhaar 6741 8529 6303",
        "aadhaar",
    );
}

#[test]
fn fr_nir_mod97_accepts_generated_values_and_rejects_mutants() {
    for value in ["151024610204325", "1 84 12 76 451 089 46"] {
        assert!(ValidatorKind::FrNirMod97.validates(value), "{value}");
    }

    let values = (0..10).map(gen_fr_nir).collect::<Vec<_>>();
    for value in &values {
        assert!(ValidatorKind::FrNirMod97.validates(value), "{value}");
        assert!(gaze_recognizers::validators::mod97_nir::validate_fr_nir(
            value
        ));
        assert!(!ValidatorKind::FrNirMod97.validates(&flip_last_digit(value)));
    }

    assert_validator_pipeline_round_trip(
        r"\b[12]\d(?: ?\d){13}\b",
        ValidatorKind::FrNirMod97,
        "NIR 190010100100058",
        "nir",
    );
}

#[test]
fn de_steuer_id_mod1110_accepts_generated_values_and_rejects_mutants() {
    for value in ["48954371207", "55492670836", "65929970489"] {
        assert!(ValidatorKind::DeSteuerIdMod1110.validates(value), "{value}");
    }
    assert!(!ValidatorKind::DeSteuerIdMod1110.validates("12345678903"));

    let values = (0..10).map(gen_steuer_id).collect::<Vec<_>>();
    for value in &values {
        assert!(ValidatorKind::DeSteuerIdMod1110.validates(value), "{value}");
        assert!(gaze_recognizers::validators::mod11::validate_de_steuer_id(
            value
        ));
        assert!(!ValidatorKind::DeSteuerIdMod1110.validates(&flip_last_digit(value)));
    }

    assert_validator_pipeline_round_trip(
        r"\b\d{2} \d{3} \d{3} \d{3}\b",
        ValidatorKind::DeSteuerIdMod1110,
        "Steuer-ID 48 954 371 207",
        "steuer_id",
    );
}

#[test]
fn bsn_mod11_accepts_generated_values_and_rejects_mutants() {
    assert!(ValidatorKind::BsnMod11.validates("123456782"));

    let values = (0..10).map(gen_bsn).collect::<Vec<_>>();
    for value in &values {
        assert!(ValidatorKind::BsnMod11.validates(value), "{value}");
        assert!(gaze_recognizers::validators::mod11::validate_bsn(value));
        assert!(!ValidatorKind::BsnMod11.validates(&flip_last_digit(value)));
    }

    assert_validator_pipeline_round_trip(
        r"\b\d{9}\b",
        ValidatorKind::BsnMod11,
        "BSN 111222333",
        "bsn",
    );
}

#[test]
fn cpf_mod11_accepts_generated_values_and_rejects_mutants() {
    let values = (0..10).map(gen_cpf).collect::<Vec<_>>();
    for value in &values {
        assert!(ValidatorKind::CpfMod11.validates(value), "{value}");
        assert!(gaze_recognizers::validators::mod11::validate_cpf(value));
        assert!(!ValidatorKind::CpfMod11.validates(&flip_last_digit(value)));
    }

    assert_validator_pipeline_round_trip(
        r"\b\d{3}\.\d{3}\.\d{3}-\d{2}\b",
        ValidatorKind::CpfMod11,
        "CPF 529.982.247-25",
        "cpf",
    );
}

#[test]
fn cnpj_mod11_accepts_generated_values_and_rejects_mutants() {
    let values = (0..10).map(gen_cnpj).collect::<Vec<_>>();
    for value in &values {
        assert!(ValidatorKind::CnpjMod11.validates(value), "{value}");
        assert!(gaze_recognizers::validators::mod11::validate_cnpj(value));
        assert!(!ValidatorKind::CnpjMod11.validates(&flip_last_digit(value)));
    }

    assert_validator_pipeline_round_trip(
        r"\b\d{2}\.\d{3}\.\d{3}/\d{4}-\d{2}\b",
        ValidatorKind::CnpjMod11,
        "CNPJ 04.252.011/0001-10",
        "cnpj",
    );
}

#[test]
fn uk_nhs_mod11_accepts_generated_values_and_rejects_mutants() {
    let values = (0..10).map(gen_nhs).collect::<Vec<_>>();
    for value in &values {
        assert!(ValidatorKind::UkNhsMod11.validates(value), "{value}");
        assert!(gaze_recognizers::validators::mod11::validate_uk_nhs(value));
        assert!(!ValidatorKind::UkNhsMod11.validates(&flip_last_digit(value)));
    }

    assert_validator_pipeline_round_trip(
        r"\b\d{3} \d{3} \d{4}\b",
        ValidatorKind::UkNhsMod11,
        "NHS number 943 476 5919",
        "nhs_number",
    );
}

fn assert_validator_pipeline_round_trip(
    pattern: &str,
    validator: ValidatorKind,
    input: &str,
    class: &str,
) {
    let pipeline = custom_validator_pipeline(pattern, PiiClass::custom(class), validator, None);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(&pipeline, &session, input);

    assert!(clean.contains(&format!(":Custom:{class}_1>")), "{clean}");
    assert_eq!(restore_tokens(&session, &clean), input);
}

fn flip_last_digit(value: &str) -> String {
    let mut bytes = value.as_bytes().to_vec();
    let index = bytes
        .iter()
        .rposition(u8::is_ascii_digit)
        .expect("digit fixture");
    bytes[index] = if bytes[index] == b'9' {
        b'0'
    } else {
        bytes[index] + 1
    };
    String::from_utf8(bytes).expect("ascii fixture")
}

fn gen_aadhaar(seed: u64) -> String {
    let mut digits = [0u8; 12];
    digits[0] = 2 + (seed as u8 % 8);
    for (index, digit) in digits[..11].iter_mut().enumerate().skip(1) {
        *digit = ((seed + index as u64 * 7) % 10) as u8;
    }
    digits[11] = verhoeff_check_digit(&digits[..11]);
    digits_to_string(&digits)
}

fn verhoeff_check_digit(digits: &[u8]) -> u8 {
    const D: [[u8; 10]; 10] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
        [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
        [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
        [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
        [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
        [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
        [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
        [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
        [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    ];
    const P: [[u8; 10]; 8] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
        [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
        [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
        [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
        [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
        [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
        [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
    ];
    const INV: [u8; 10] = [0, 4, 3, 2, 1, 5, 6, 7, 8, 9];
    let mut checksum = 0u8;
    for (index, digit) in digits.iter().rev().enumerate() {
        checksum = D[checksum as usize][P[(index + 1) % 8][*digit as usize] as usize];
    }
    INV[checksum as usize]
}

fn gen_fr_nir(seed: u64) -> String {
    let base = format!("1{:02}0101001{:03}", 90 + seed % 10, seed % 999);
    let number = base.parse::<u64>().expect("nir base");
    format!("{base}{:02}", 97 - (number % 97))
}

fn gen_steuer_id(seed: u64) -> String {
    let mut digits = [0u8; 11];
    for (index, digit) in digits[..10].iter_mut().enumerate() {
        *digit = ((seed + index as u64) % 10) as u8;
    }
    digits[0] = 1 + (digits[0] % 9);
    digits[10] = steuer_id_check_digit(&digits[..10]);
    digits_to_string(&digits)
}

fn steuer_id_check_digit(digits: &[u8]) -> u8 {
    let mut product = 10u8;
    for digit in digits {
        let mut sum = (*digit + product) % 10;
        if sum == 0 {
            sum = 10;
        }
        product = (2 * sum) % 11;
    }
    (11 - product) % 10
}

fn gen_bsn(seed: u64) -> String {
    for candidate in seed * 100.. {
        let mut digits = first_digits::<8>(candidate);
        digits[0] = 1 + (digits[0] % 9);
        let sum: u32 = digits
            .iter()
            .enumerate()
            .map(|(index, digit)| u32::from(*digit) * (9 - index as u32))
            .sum();
        let check = sum % 11;
        if check < 10 {
            let mut full = [0u8; 9];
            full[..8].copy_from_slice(&digits);
            full[8] = check as u8;
            return digits_to_string(&full);
        }
    }
    unreachable!("unbounded BSN generator finds a check digit")
}

fn gen_cpf(seed: u64) -> String {
    let mut digits = [0u8; 11];
    digits[..9].copy_from_slice(&first_digits::<9>(seed + 529_982_247));
    digits[9] = cpf_check_digit(&digits[..9], 10);
    digits[10] = cpf_check_digit(&digits[..10], 11);
    digits_to_string(&digits)
}

fn gen_cnpj(seed: u64) -> String {
    let mut digits = [0u8; 14];
    digits[..12].copy_from_slice(&first_digits::<12>(seed + 42_520_110_001));
    digits[12] = weighted_check_digit(&digits[..12], &[5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2]);
    digits[13] = weighted_check_digit(&digits[..13], &[6, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2]);
    digits_to_string(&digits)
}

fn gen_nhs(seed: u64) -> String {
    for candidate in seed * 100.. {
        let digits = first_digits::<9>(candidate + 943_476_591);
        let sum: u32 = digits
            .iter()
            .enumerate()
            .map(|(index, digit)| u32::from(*digit) * (10 - index as u32))
            .sum();
        let check = 11 - (sum % 11);
        let check = if check == 11 { 0 } else { check };
        if check != 10 {
            let mut full = [0u8; 10];
            full[..9].copy_from_slice(&digits);
            full[9] = check as u8;
            return digits_to_string(&full);
        }
    }
    unreachable!("unbounded NHS generator finds a check digit")
}

fn first_digits<const N: usize>(mut value: u64) -> [u8; N] {
    let mut digits = [0u8; N];
    for digit in digits.iter_mut().rev() {
        *digit = (value % 10) as u8;
        value /= 10;
    }
    digits
}

fn cpf_check_digit(digits: &[u8], start_weight: u8) -> u8 {
    let sum: u32 = digits
        .iter()
        .zip((2..=start_weight).rev())
        .map(|(digit, weight)| u32::from(*digit) * u32::from(weight))
        .sum();
    let remainder = sum % 11;
    if remainder < 2 {
        0
    } else {
        (11 - remainder) as u8
    }
}

fn weighted_check_digit(digits: &[u8], weights: &[u8]) -> u8 {
    let sum: u32 = digits
        .iter()
        .zip(weights)
        .map(|(digit, weight)| u32::from(*digit) * u32::from(*weight))
        .sum();
    let remainder = sum % 11;
    if remainder < 2 {
        0
    } else {
        (11 - remainder) as u8
    }
}

fn digits_to_string(digits: &[u8]) -> String {
    digits
        .iter()
        .map(|digit| char::from(b'0' + *digit))
        .collect()
}
