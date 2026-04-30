#![cfg(feature = "phone-parser")]

use std::cell::Cell;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Pipeline, RawDocument, RawMatch, RecognizerSpec,
    Rulepack, RulepackError, RulepackSource, Scope, Session,
};
use gaze_recognizers::{embedded, NormalizerKind, RegexDetector, ValidatorKind};
use gaze_types::{DetectContext, DictionaryBundle, LocaleTag, PiiClass, Recognizer};

fn core_extended() -> Rulepack {
    Rulepack::load(RulepackSource::Embedded(
        embedded("core-extended").expect("core-extended embedded rulepack"),
    ))
    .expect("core-extended loads")
}

fn regex_from_spec(spec: &RecognizerSpec) -> RegexDetector {
    let RawMatch::Regex {
        pattern,
        pattern_template: None,
        capture_groups,
    } = &spec.matcher
    else {
        panic!("expected plain regex recognizer {}", spec.id);
    };

    RegexDetector::with_rulepack_fields(
        pattern.as_deref().expect("regex pattern"),
        spec.class.clone(),
        &spec.id,
        spec.locales.clone(),
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups.clone(),
        spec.context
            .as_ref()
            .map(|context| context.exclusions.clone())
            .unwrap_or_default(),
        spec.validator
            .as_ref()
            .map(|validator| ValidatorKind::parse(&validator.kind).expect("validator kind")),
        spec.normalizer
            .as_ref()
            .map(|normalizer| NormalizerKind::parse(&normalizer.kind).expect("normalizer kind")),
    )
    .expect("regex detector")
}

fn detect_recognizer(
    rulepack: &Rulepack,
    recognizer_id: &str,
    input: &str,
    locale: LocaleTag,
) -> Vec<String> {
    let dictionaries = DictionaryBundle::default();
    let fields = ();
    let ctx = DetectContext {
        locale_chain: &[locale, LocaleTag::Global],
        dictionaries: &dictionaries,
        fields: &fields,
        degraded: Cell::new(false),
    };
    let spec = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == recognizer_id)
        .unwrap_or_else(|| panic!("missing recognizer {recognizer_id}"));
    let detector = regex_from_spec(spec);

    Recognizer::detect(&detector, input, &ctx)
        .into_iter()
        .map(|candidate| input[candidate.span].to_string())
        .collect()
}

fn detect_recognizer_canonical_forms(
    rulepack: &Rulepack,
    recognizer_id: &str,
    input: &str,
    locale: LocaleTag,
) -> Vec<Option<String>> {
    let dictionaries = DictionaryBundle::default();
    let fields = ();
    let ctx = DetectContext {
        locale_chain: &[locale, LocaleTag::Global],
        dictionaries: &dictionaries,
        fields: &fields,
        degraded: Cell::new(false),
    };
    let spec = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == recognizer_id)
        .unwrap_or_else(|| panic!("missing recognizer {recognizer_id}"));
    let detector = regex_from_spec(spec);

    Recognizer::detect(&detector, input, &ctx)
        .into_iter()
        .map(|candidate| candidate.canonical_form)
        .collect()
}

fn pipeline_from_rulepack(rulepack: &Rulepack) -> Pipeline {
    let mut builder = Pipeline::builder()
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("phone".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("ip_address".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("postal_code".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("iban".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("credit_card".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve));

    for spec in &rulepack.recognizers {
        if spec.enabled {
            builder = builder.recognizer(regex_from_spec(spec));
        }
    }

    builder.build().expect("pipeline")
}

fn clean_text(pipeline: &Pipeline, session: &Session, input: &str, locale: LocaleTag) -> String {
    let clean = pipeline
        .redact_with_context(
            session,
            RawDocument::Text(input.to_string()),
            &[locale, LocaleTag::Global],
        )
        .expect("clean");
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

fn assert_custom_token(clean: &str, class: &str) {
    let pattern = format!(r"^.+<[0-9a-f]{{8}}:Custom:{class}_1>.*$");
    assert!(
        regex::Regex::new(&pattern).unwrap().is_match(clean),
        "missing Custom:{class} token in {clean}"
    );
}

#[test]
fn corpus_accepts_universal_shapes_and_rejects_tenant_like_phone_inputs() {
    let rulepack = core_extended();

    assert_eq!(
        detect_recognizer(
            &rulepack,
            "phone.structural",
            // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
            // literals chosen for parser-valid + non-routable.
            "Phone +4915100000000",
            LocaleTag::DeDe
        ),
        vec!["+4915100000000".to_string()]
    );
    assert_eq!(
        detect_recognizer(
            &rulepack,
            "phone.national.de",
            // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
            // literals chosen for parser-valid + non-routable.
            "Phone +49 30 0000 0000",
            LocaleTag::DeDe
        ),
        vec!["+49 30 0000 0000".to_string()]
    );
    // drift-ack: S4 DE national phone broaden — internal recall per gaze-laravel
    for (input, expected) in [
        // Regression fixtures from #414: common DE metropolitan landline
        // shapes that must not fall below the national recognizer floor.
        ("Phone 040 1234567", "040 1234567"),
        ("Phone 089 1234567", "089 1234567"),
        ("Phone 089 12345678", "089 12345678"),
        ("Phone 069 1234567", "069 1234567"),
        ("Phone 030 1234567", "030 1234567"),
        ("Phone 030 12345678", "030 12345678"),
        ("Phone +49 89 1234567", "+49 89 1234567"),
        // Source: synthetic-non-reachable; Germany has no official fictional
        // range equivalent to NANPA 555-01XX, so these literals are
        // parser-valid but intentionally non-routable-looking test shapes.
        ("Phone +49 40 0000 0000", "+49 40 0000 0000"),
        ("Phone +49 89 0000 0000", "+49 89 0000 0000"),
        ("Phone +49 69 0000 0000", "+49 69 0000 0000"),
        ("Phone +49 221 0000 000", "+49 221 0000 000"),
        ("Phone +49 711 0000 000", "+49 711 0000 000"),
    ] {
        assert_eq!(
            detect_recognizer(&rulepack, "phone.national.de", input, LocaleTag::DeDe),
            vec![expected.to_string()],
            "{input}"
        );
    }
    assert_eq!(
        detect_recognizer(
            &rulepack,
            "phone.national.us",
            // Source: NANPA 555-LINE Number Reservation.
            // https://nationalnanpa.com/number_resource_info/555_numbers.html
            "Phone +1 555 0100",
            LocaleTag::EnUs
        ),
        vec!["+1 555 0100".to_string()]
    );
    for (input, expected) in [
        ("Order 1-202-555-0100", "1-202-555-0100"),
        ("Phone (415) 555-0101", "(415) 555-0101"),
    ] {
        assert_eq!(
            detect_recognizer(&rulepack, "phone.national.us", input, LocaleTag::EnUs),
            vec![expected.to_string()],
            "{input}"
        );
    }
    for input in ["Order_15551234567", "Customer+12025550100"] {
        assert!(
            detect_recognizer(&rulepack, "phone.national.us", input, LocaleTag::EnUs).is_empty(),
            "phone.national.us must not fire for {input}"
        );
    }
    assert_eq!(
        detect_recognizer(&rulepack, "ip.v4", "Host 192.168.1.1.", LocaleTag::EnUs),
        vec!["192.168.1.1".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "ip.v6", "Loopback ::1.", LocaleTag::EnUs),
        vec!["::1".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "ip.v6", "Host 2001:db8::1.", LocaleTag::EnUs),
        vec!["2001:db8::1".to_string()]
    );
    for (input, expected) in [
        ("Host ::ffff:192.0.2.128.", "::ffff:192.0.2.128"),
        ("Host ::ffff:0.0.0.0.", "::ffff:0.0.0.0"),
        ("Host ::192.0.2.1.", "::192.0.2.1"),
        ("Host 2001:db8::192.0.2.1.", "2001:db8::192.0.2.1"),
        ("Host fe80::ffff:1.2.3.4.", "fe80::ffff:1.2.3.4"),
        ("Host 0:0:0:0:0:ffff:192.0.2.1.", "0:0:0:0:0:ffff:192.0.2.1"),
    ] {
        assert_eq!(
            detect_recognizer(&rulepack, "ip.v6", input, LocaleTag::EnUs),
            vec![expected.to_string()],
            "{input}"
        );
    }
    // Invalid IPv4-embedded octets are rejected entirely instead of falling
    // through to a partial hex-only IPv6 prefix match.
    assert!(detect_recognizer(
        &rulepack,
        "ip.v6",
        "Host ::ffff:256.0.0.1.",
        LocaleTag::EnUs
    )
    .is_empty());
    assert_eq!(
        detect_recognizer(
            &rulepack,
            "ip.v6",
            "Build at 1.2.3.4 ::ffff:5.6.7.8 done",
            LocaleTag::EnUs
        ),
        vec!["::ffff:5.6.7.8".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "postal.de", "Berlin 10115", LocaleTag::DeDe),
        vec!["10115".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "postal.us", "ZIP 94103", LocaleTag::EnUs),
        vec!["94103".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "postal.us", "ZIP 94103-1234", LocaleTag::EnUs),
        vec!["94103-1234".to_string()]
    );

    // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists.
    // +49 1555 0112233-style literals follow the documented DE fixture shape;
    // 0171 0000000 covers the separator regression shape without live-looking data.
    for (input, expected) in [
        ("Phone +49 1555 0112233", "+49 1555 0112233"),
        ("Phone +49-1555-0112233", "+49-1555-0112233"),
        ("Phone +49 1555/0112233", "+49 1555/0112233"),
        ("Phone +49.1555.0112233", "+49.1555.0112233"),
        ("Phone +4915550112233", "+4915550112233"),
        ("Phone 01555 0112233", "01555 0112233"),
        ("Phone 01555-0112233", "01555-0112233"),
        ("Phone 01555/0112233", "01555/0112233"),
        ("Phone 01555.0112233", "01555.0112233"),
        ("Phone 0171-0000000", "0171-0000000"),
    ] {
        assert_eq!(
            detect_recognizer(&rulepack, "phone.national.de", input, LocaleTag::DeDe),
            vec![expected.to_string()],
            "{input}"
        );
    }
    for input in [
        "Build finished at 01:55:50.112233",
        "Order 0171-0000000X",
        "Customer_4915550112233",
        "Order_4915550112233",
        "0911 1234",
        "IBAN DE89 3704 0044 0532 0130 00",
    ] {
        assert!(
            detect_recognizer(&rulepack, "phone.national.de", input, LocaleTag::DeDe).is_empty(),
            "phone.national.de must not fire for {input}"
        );
    }

    for input in [
        "1.2.3.4567",
        "1.2.3",
        "version v1.2.3.4",
        "2026-04-25",
        "0815 12345",
        "0123-456789",
        "+99999999",
        "Order_42",
        "Subscriber_12345",
        "This bare inventory count is 12345 in a non-postal sentence",
        "Subscriber_0001234567",
        "Subscriber_4915550112233",
        "Customer_4915550112234",
        "Order_4915550112235",
        "Order_0815",
    ] {
        assert!(
            detect_recognizer(&rulepack, "phone.structural", input, LocaleTag::DeDe).is_empty(),
            "phone.structural must not fire for {input}"
        );
        assert!(
            detect_recognizer(&rulepack, "phone.national.de", input, LocaleTag::DeDe).is_empty(),
            "phone.national.de must not fire for {input}"
        );
    }
}

#[test]
fn phase2_corpus_accepts_validator_passing_iban_and_cards_only() {
    let rulepack = core_extended();

    for (input, recognizer_id, expected) in [
        (
            "IBAN GB82WEST12345698765432",
            "iban.structural",
            "GB82WEST12345698765432",
        ),
        (
            "IBAN DE89370400440532013000",
            "iban.structural",
            "DE89370400440532013000",
        ),
        (
            "IBAN FR1420041010050500013M02606",
            "iban.structural",
            "FR1420041010050500013M02606",
        ),
        (
            "IBAN BE68539007547034",
            "iban.structural",
            "BE68539007547034",
        ),
        (
            "IBAN NL91ABNA0417164300",
            "iban.structural",
            "NL91ABNA0417164300",
        ),
        (
            "Card 4111111111111111",
            "card.structural",
            "4111111111111111",
        ),
        (
            "Card 4012888888881881",
            "card.structural",
            "4012888888881881",
        ),
        ("Card 4222222222222", "card.structural", "4222222222222"),
        (
            "Card 5555555555554444",
            "card.structural",
            "5555555555554444",
        ),
        (
            "Card 5105105105105100",
            "card.structural",
            "5105105105105100",
        ),
        ("Card 378282246310005", "card.structural", "378282246310005"),
    ] {
        assert_eq!(
            detect_recognizer(&rulepack, recognizer_id, input, LocaleTag::EnUs),
            vec![expected.to_string()],
            "{input}"
        );
    }

    for input in [
        "Card 4111111111111112",
        "Card 4012888888881882",
        "Card 4222222222223",
        "Card 5555555555554445",
        "Card 5105105105105101",
        "Card 378282246310006",
        "IBAN GB99WEST12345698765432",
        "IBAN DE99370400440532013000",
        "IBAN FR9920041010050500013M02606",
        "IBAN BE99539007547034",
        "IBAN NL99ABNA0417164300",
        "Subscriber_0001234567",
        "Order_0815",
        "0815 12345",
        "Customer_42",
    ] {
        assert!(
            detect_recognizer(&rulepack, "card.structural", input, LocaleTag::EnUs).is_empty(),
            "card.structural must not fire for {input}"
        );
        assert!(
            detect_recognizer(&rulepack, "iban.structural", input, LocaleTag::EnUs).is_empty(),
            "iban.structural must not fire for {input}"
        );
    }
}

#[test]
fn phase2_validators_drop_failing_candidates_and_iban_canonicalizes() {
    let rulepack = core_extended();

    // ADVERSARIAL: this test relies on S1 ValidatorKind dispatch in
    // RegexDetector::canonical_form. Disabling that dispatch (deleting
    // validator_kind field-read) MUST cause the FAILING tests to start
    // passing. This proves Phase 2 inherits the data-flow gate from S1
    // (drawer architecture_853a0593).
    assert_eq!(
        detect_recognizer(
            &rulepack,
            "card.structural",
            "Card 4111111111111111",
            LocaleTag::EnUs
        ),
        vec!["4111111111111111".to_string()]
    );
    assert!(detect_recognizer(
        &rulepack,
        "card.structural",
        "Card 4111111111111112",
        LocaleTag::EnUs
    )
    .is_empty());
    assert_eq!(
        detect_recognizer_canonical_forms(
            &rulepack,
            "iban.structural",
            "IBAN GB82WEST12345698765432",
            LocaleTag::EnUs
        ),
        vec![Some("GB82WEST12345698765432".to_string())]
    );
    assert!(detect_recognizer(
        &rulepack,
        "iban.structural",
        "IBAN GB99WEST12345698765432",
        LocaleTag::EnUs
    )
    .is_empty());
}

#[test]
fn phase2_formatted_iban_with_spaces_tokenizes_and_round_trips() {
    let rulepack = core_extended();
    let input = "Bank IBAN: DE89 3704 0044 0532 0130 00";

    assert_eq!(
        detect_recognizer(&rulepack, "iban.structural", input, LocaleTag::DeDe),
        vec!["DE89 3704 0044 0532 0130 00".to_string()]
    );
    assert_eq!(
        detect_recognizer_canonical_forms(&rulepack, "iban.structural", input, LocaleTag::DeDe),
        vec![Some("DE89370400440532013000".to_string())]
    );

    let pipeline = pipeline_from_rulepack(&rulepack);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(&pipeline, &session, input, LocaleTag::DeDe);
    assert_custom_token(&clean, "iban");
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[test]
fn phase2_formatted_card_with_spaces_tokenizes_and_round_trips() {
    let rulepack = core_extended();
    let input = "Card: 4111 1111 1111 1111";

    assert_eq!(
        detect_recognizer(&rulepack, "card.structural", input, LocaleTag::EnUs),
        vec!["4111 1111 1111 1111".to_string()]
    );

    let pipeline = pipeline_from_rulepack(&rulepack);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(&pipeline, &session, input, LocaleTag::EnUs);
    assert_custom_token(&clean, "credit_card");
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[test]
fn phase2_formatted_card_with_hyphens_tokenizes_and_round_trips() {
    let rulepack = core_extended();
    let input = "Card: 4111-1111-1111-1111";

    assert_eq!(
        detect_recognizer(&rulepack, "card.structural", input, LocaleTag::EnUs),
        vec!["4111-1111-1111-1111".to_string()]
    );

    let pipeline = pipeline_from_rulepack(&rulepack);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(&pipeline, &session, input, LocaleTag::EnUs);
    assert_custom_token(&clean, "credit_card");
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[test]
fn phase2_formatted_card_failing_luhn_drops() {
    let rulepack = core_extended();
    let input = "Card: 4111 1111 1111 1112";

    assert!(detect_recognizer(&rulepack, "card.structural", input, LocaleTag::EnUs).is_empty());

    let pipeline = pipeline_from_rulepack(&rulepack);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(&pipeline, &session, input, LocaleTag::EnUs);
    assert_eq!(clean, input);
}

#[test]
fn phase2_iban_and_cards_are_universal_and_solo_classes() {
    let rulepack = core_extended();
    let iban = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "iban.structural")
        .expect("iban.structural");
    let card = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "card.structural")
        .expect("card.structural");

    assert_eq!(iban.locales, vec![LocaleTag::Global]);
    assert_eq!(card.locales, vec![LocaleTag::Global]);
    assert!(iban.cooperates_with.is_empty());
    assert!(card.cooperates_with.is_empty());
    assert_eq!(
        rulepack
            .recognizers
            .iter()
            .filter(|recognizer| recognizer.class == PiiClass::Custom("iban".to_string()))
            .count(),
        1
    );
    assert_eq!(
        rulepack
            .recognizers
            .iter()
            .filter(|recognizer| recognizer.class == PiiClass::Custom("credit_card".to_string()))
            .count(),
        1
    );

    for locale in [
        LocaleTag::EnUs,
        LocaleTag::DeDe,
        LocaleTag::Other("fr-FR".to_string()),
    ] {
        assert_eq!(
            detect_recognizer(
                &rulepack,
                "iban.structural",
                "IBAN GB82WEST12345698765432",
                locale.clone()
            ),
            vec!["GB82WEST12345698765432".to_string()],
            "{locale:?}"
        );
        assert_eq!(
            detect_recognizer(
                &rulepack,
                "card.structural",
                "Card 4111111111111111",
                locale.clone()
            ),
            vec!["4111111111111111".to_string()],
            "{locale:?}"
        );
    }
}

#[test]
fn core_and_core_extended_compose_without_counter_collision() {
    let core = Rulepack::load(RulepackSource::Embedded(embedded("core").unwrap())).unwrap();
    let extended = core_extended();
    let mut builder = Pipeline::builder()
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("phone".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve));

    let email_spec = core
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "email.global")
        .expect("email.global");
    builder = builder.recognizer(regex_from_spec(email_spec));
    let phone_spec = extended
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "phone.structural")
        .expect("phone.structural");
    builder = builder.recognizer(regex_from_spec(phone_spec));

    let pipeline = builder.build().expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(
        &pipeline,
        &session,
        // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
        // literals chosen for parser-valid + non-routable.
        "Contact alice@example.invalid or +4915100000000",
        LocaleTag::EnUs,
    );

    let tokens = clean
        .split_whitespace()
        .filter(|part| part.starts_with('<'))
        .collect::<Vec<_>>();
    assert_eq!(tokens.len(), 2);
    assert!(tokens[0].ends_with(":Email_1>"), "{clean}");
    assert!(tokens[1].ends_with(":Custom:phone_1>"), "{clean}");
}

#[test]
fn core_and_core_extended_compose_with_phase2_without_counter_collision() {
    let core = Rulepack::load(RulepackSource::Embedded(embedded("core").unwrap())).unwrap();
    let extended = core_extended();
    let mut builder = Pipeline::builder()
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("phone".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("ip_address".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("postal_code".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("iban".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("credit_card".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve));

    let email_spec = core
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "email.global")
        .expect("email.global");
    builder = builder.recognizer(regex_from_spec(email_spec));
    for spec in &extended.recognizers {
        builder = builder.recognizer(regex_from_spec(spec));
    }

    let pipeline = builder.build().expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
    // literals chosen for parser-valid + non-routable.
    let input = "Email alice@example.invalid phone +4915100000000 host 192.168.1.1 zip 94103 IBAN GB82WEST12345698765432 card 4111111111111111";
    let clean = clean_text(&pipeline, &session, input, LocaleTag::EnUs);

    assert!(clean.contains(":Email_1>"), "{clean}");
    assert_custom_token(&clean, "phone");
    assert_custom_token(&clean, "ip_address");
    assert_custom_token(&clean, "postal_code");
    assert_custom_token(&clean, "iban");
    assert_custom_token(&clean, "credit_card");
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[test]
fn every_phase1_recognizer_round_trips_through_restore() {
    let rulepack = core_extended();
    let pipeline = pipeline_from_rulepack(&rulepack);

    for (input, locale) in [
        // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
        // literals chosen for parser-valid + non-routable.
        ("Call +49 30 0000 0000", LocaleTag::DeDe),
        // Source: NANPA 555-LINE Number Reservation.
        // https://nationalnanpa.com/number_resource_info/555_numbers.html
        ("Call +1 555 0100", LocaleTag::EnUs),
        ("Host 192.168.1.1", LocaleTag::EnUs),
        ("Loopback ::1", LocaleTag::EnUs),
        ("Host 2001:db8::1", LocaleTag::EnUs),
        ("Berlin 10115", LocaleTag::DeDe),
        ("ZIP 94103-1234", LocaleTag::EnUs),
    ] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, input, locale);
        assert_ne!(clean, input, "expected tokenized output for {input}");
        let restored =
            gaze::token_shape::pattern().replace_all(&clean, |captures: &regex::Captures<'_>| {
                session.restore_strict(&captures[0]).expect("known token")
            });
        assert_eq!(restored, input);
    }
}

#[test]
fn every_phase2_recognizer_round_trips_through_restore() {
    let rulepack = core_extended();
    let pipeline = pipeline_from_rulepack(&rulepack);

    for (input, locale) in [
        ("IBAN GB82WEST12345698765432", LocaleTag::EnUs),
        ("IBAN DE89370400440532013000", LocaleTag::DeDe),
        ("Card 4111111111111111", LocaleTag::EnUs),
        ("Card 5555555555554444", LocaleTag::DeDe),
    ] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, input, locale);
        assert_ne!(clean, input, "expected tokenized output for {input}");
        assert_eq!(restore_tokens(&session, &clean), input);
    }
}

#[test]
fn embedded_core_extended_load_smoke_has_at_least_seven_recognizers() {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core-extended").expect("core-extended embedded rulepack"),
    ))
    .expect("core-extended loads");

    assert!(rulepack.recognizers.len() >= 7);
}

#[test]
fn same_class_cooperation_is_data_and_unilateral_failure_behavior() {
    let raw = embedded("core-extended").unwrap();
    let rulepack = Rulepack::load(RulepackSource::Embedded(raw)).unwrap();
    let ip_v4 = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "ip.v4")
        .unwrap();
    let ip_v6 = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "ip.v6")
        .unwrap();
    assert_eq!(ip_v4.cooperates_with, vec!["ip.v6"]);
    assert_eq!(ip_v6.cooperates_with, vec!["ip.v4"]);
    let phone_structural = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "phone.structural")
        .unwrap();
    let phone_de = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "phone.national.de")
        .unwrap();
    let phone_us = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "phone.national.us")
        .unwrap();
    assert_eq!(
        phone_structural.cooperates_with,
        vec!["phone.national.de", "phone.national.us"]
    );
    assert_eq!(
        phone_de.cooperates_with,
        vec!["phone.structural", "phone.national.us"]
    );
    assert_eq!(
        phone_us.cooperates_with,
        vec!["phone.structural", "phone.national.de"]
    );

    let one_side_removed = raw.replace("cooperates_with = [\"ip.v4\"]\n", "");
    Rulepack::parse(&one_side_removed).expect("one-sided cooperation remains valid");

    let both_sides_removed = one_side_removed.replace("cooperates_with = [\"ip.v6\"]\n", "");
    let err = Rulepack::parse(&both_sides_removed)
        .expect_err("both-side cooperation drop must fail closed");
    assert!(matches!(
        err,
        RulepackError::SameClassWithoutCooperation {
            class: PiiClass::Custom(ref name),
            ..
        } if name == "ip_address"
    ));

    let one_postal_side_removed = raw.replace("cooperates_with = [\"postal.de\"]\n", "");
    Rulepack::parse(&one_postal_side_removed).expect("one-sided postal cooperation remains valid");

    let both_postal_sides_removed =
        one_postal_side_removed.replace("cooperates_with = [\"postal.us\"]\n", "");
    let err = Rulepack::parse(&both_postal_sides_removed)
        .expect_err("both-side postal cooperation drop must fail closed");
    assert!(matches!(
        err,
        RulepackError::SameClassWithoutCooperation {
            class: PiiClass::Custom(ref name),
            ..
        } if name == "postal_code"
    ));
}
