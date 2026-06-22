#![cfg(feature = "phone-parser")]

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Pipeline, RawDocument, RawMatch, RecognizerSpec,
    RedactionEntry, RedactionLogError, RedactionLogger, Rulepack, RulepackError, RulepackSource,
    Scope, Session,
};
use gaze_recognizers::{embedded, NormalizerKind, RegexDetector, ValidatorKind};
use gaze_types::{
    DetectContext, DictionaryBundle, LocaleTag, PiiClass, Recognizer, ValidatorOutcome,
};
use std::sync::{Arc, Mutex};

fn core_extended() -> Rulepack {
    let mut rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core-extended").expect("core-extended embedded rulepack"),
    ))
    .expect("core-extended loads");
    rulepack.recognizers.retain(|recognizer| {
        !recognizer.id.starts_with("email.") && !recognizer.id.starts_with("name.")
    });
    rulepack
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
    let locale_chain = [locale, LocaleTag::Global];
    let ctx = DetectContext::new(&locale_chain, &dictionaries);
    let spec = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == recognizer_id)
        .unwrap_or_else(|| panic!("missing recognizer {recognizer_id}"));
    let detector = regex_from_spec(spec);

    let validator_kind = detector.validator_kind();

    Recognizer::detect(&detector, input, &ctx)
        .unwrap()
        .into_iter()
        .filter(|candidate| {
            validator_kind.is_none_or(|kind| {
                matches!(
                    kind.validate(&input[candidate.span.clone()]),
                    ValidatorOutcome::Pass { .. } | ValidatorOutcome::NotApplicable
                )
            })
        })
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
    let locale_chain = [locale, LocaleTag::Global];
    let ctx = DetectContext::new(&locale_chain, &dictionaries);
    let spec = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == recognizer_id)
        .unwrap_or_else(|| panic!("missing recognizer {recognizer_id}"));
    let detector = regex_from_spec(spec);

    let validator_kind = detector.validator_kind();

    Recognizer::detect(&detector, input, &ctx)
        .unwrap()
        .into_iter()
        .filter(|candidate| {
            validator_kind.is_none_or(|kind| {
                matches!(
                    kind.validate(&input[candidate.span.clone()]),
                    ValidatorOutcome::Pass { .. } | ValidatorOutcome::NotApplicable
                )
            })
        })
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
            PiiClass::Custom("eth_address".to_string()),
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
        .rule(ClassRule::new(
            PiiClass::Custom("ssn".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("nino".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("pan".to_string()),
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

fn email_global_spec() -> RecognizerSpec {
    let core = Rulepack::load(RulepackSource::Embedded(embedded("core").unwrap())).unwrap();
    core.recognizers
        .into_iter()
        .find(|recognizer| recognizer.id == "email.global")
        .expect("email.global")
}

fn email_global_pipeline() -> Pipeline {
    let email_spec = email_global_spec();
    Pipeline::builder()
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .recognizer(regex_from_spec(&email_spec))
        .build()
        .expect("pipeline")
}

fn clean_text(pipeline: &Pipeline, session: &Session, input: &str, locale: LocaleTag) -> String {
    let clean = pipeline
        .pseudonymize_with_context(
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

#[test]
fn email_global_structural_tlds_tokenize_and_restore() {
    let email_spec = email_global_spec();
    assert_eq!(
        email_spec
            .validator
            .as_ref()
            .map(|validator| validator.kind.as_str()),
        Some("email_rfc")
    );
    assert!(matches!(
        ValidatorKind::EmailRfc.validate("missingdot@example"),
        ValidatorOutcome::Fail { .. }
    ));
    let pipeline = email_global_pipeline();
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "Contacts ada@datapuente.es legal@medfleet.health k@kettenmacher.at b@bancodelnorte.mx user@northwave-labs.io ops@mail.corp.example.es";

    let clean = clean_text(&pipeline, &session, input, LocaleTag::EnUs);

    for raw in [
        "ada@datapuente.es",
        "legal@medfleet.health",
        "k@kettenmacher.at",
        "b@bancodelnorte.mx",
        "user@northwave-labs.io",
        "ops@mail.corp.example.es",
    ] {
        assert!(!clean.contains(raw), "{raw} leaked in {clean}");
    }
    assert_eq!(clean.matches(":Email_").count(), 6, "{clean}");
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[test]
fn email_global_overlapping_host_rival_keeps_email_span_intact() {
    let email_spec = email_global_spec();
    let host_rival = RegexDetector::with_rulepack_fields(
        r"(?i)\b(?:[a-z0-9](?:[a-z0-9\-]*[a-z0-9])?\.)+[a-z]{2,}\b",
        PiiClass::Custom("host".to_string()),
        "test.host.rival",
        vec![LocaleTag::Global],
        0.99,
        120,
        "counter",
        None,
        Vec::new(),
        None,
        None,
    )
    .expect("host rival");
    let pipeline = Pipeline::builder()
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("host".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve))
        .recognizer(regex_from_spec(&email_spec))
        .recognizer(host_rival)
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "visit acme.es or mail ada@acme.es";

    let clean = clean_text(&pipeline, &session, input, LocaleTag::EnUs);

    assert!(!clean.contains("ada@acme.es"), "{clean}");
    assert!(!clean.contains("ada@"), "{clean}");
    assert!(clean.contains(":Email_1>"), "{clean}");
    assert_custom_token(&clean, "host");
    assert_eq!(restore_tokens(&session, &clean), input);
}

fn assert_custom_token(clean: &str, class: &str) {
    let pattern = format!(r"^.+<[0-9a-f]{{8}}:Custom:{class}_1>.*$");
    assert!(
        regex::Regex::new(&pattern).unwrap().is_match(clean),
        "missing Custom:{class} token in {clean}"
    );
}

#[derive(Clone)]
struct CapturingLogger {
    entries: Arc<Mutex<Vec<RedactionEntry>>>,
}

impl RedactionLogger for CapturingLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.entries.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

#[test]
fn full_pipeline_rejects_identifier_attached_e164_phone_candidates() {
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
    for spec in &extended.recognizers {
        if spec.enabled {
            builder = builder.recognizer(regex_from_spec(spec));
        }
    }

    let pipeline = builder.build().expect("pipeline");

    for input in ["Customer+12025550100", "Order_+12025550100"] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, input, LocaleTag::EnUs);
        assert_eq!(clean, input, "unexpected tokenization for {input}: {clean}");
        assert!(
            !clean.contains(":Custom:phone_"),
            "custom:phone must not fire for {input}: {clean}"
        );
    }

    for input in ["Phone +12025550100", "+12025550100 logged at startup"] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, input, LocaleTag::EnUs);
        assert!(
            clean.contains(":Custom:phone_1>"),
            "missing Custom:phone token in {clean}"
        );
        assert_eq!(restore_tokens(&session, &clean), input);
    }
}

#[test]
fn spaced_international_e164_phones_tokenize_and_round_trip() {
    let rulepack = core_extended();
    let pipeline = pipeline_from_rulepack(&rulepack);

    for (input, locale, raw) in [
        // Source: libphonenumber GB mobile example shape; parser-valid synthetic fixture.
        (
            "UK phone +44 7400 123456",
            LocaleTag::EnGb,
            "+44 7400 123456",
        ),
        // Source: synthetic parser-valid international mobile fixture from dogfooding.
        (
            "FR phone +33 6 12 34 56 78",
            LocaleTag::Other("fr-FR".to_string()),
            "+33 6 12 34 56 78",
        ),
        // Source: synthetic parser-valid international mobile fixture from dogfooding.
        (
            "ES phone +34 612 345 678",
            LocaleTag::Other("es-ES".to_string()),
            "+34 612 345 678",
        ),
    ] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, input, locale);

        assert!(
            !clean.contains(raw),
            "spaced E.164 phone leaked raw bytes {raw} in {clean}"
        );
        assert_eq!(clean.matches(":Custom:phone_").count(), 1, "{clean}");
        assert_eq!(restore_tokens(&session, &clean), input);
    }
}

#[test]
fn spaced_e164_phone_recognizer_accepts_international_spacing_and_vetoes_false_positives() {
    let rulepack = core_extended();

    for (input, locale, expected) in [
        // Source: libphonenumber GB mobile example shape; parser-valid synthetic fixture.
        (
            "UK phone +44 7400 123456",
            LocaleTag::EnGb,
            "+44 7400 123456",
        ),
        // Source: synthetic parser-valid international mobile fixture from dogfooding.
        (
            "FR phone +33 6 12 34 56 78",
            LocaleTag::Other("fr-FR".to_string()),
            "+33 6 12 34 56 78",
        ),
        // Source: synthetic parser-valid international mobile fixture from dogfooding.
        (
            "ES phone +34 612 345 678",
            LocaleTag::Other("es-ES".to_string()),
            "+34 612 345 678",
        ),
    ] {
        assert_eq!(
            detect_recognizer(&rulepack, "phone.e164.spaced", input, locale),
            vec![expected.to_string()],
            "{input}"
        );
    }

    for input in [
        "Noise +12 34",
        "Noise +1234567",
        "Noise +99999999",
        // Source: NANPA 555-LINE Number Reservation; handled by phone.national.us.
        "US fixture +1 555 0100",
        // Source: synthetic-non-reachable; handled by phone.national.de.
        "DE fixture +49 1555 0112233",
        "Customer_+447400123456",
    ] {
        assert!(
            detect_recognizer(&rulepack, "phone.e164.spaced", input, LocaleTag::EnGb).is_empty(),
            "phone.e164.spaced must not fire for {input}"
        );
    }
}

#[test]
fn existing_regional_and_contiguous_phone_fixtures_still_round_trip() {
    let rulepack = core_extended();
    let pipeline = pipeline_from_rulepack(&rulepack);

    for (input, locale, recognizer_id, expected) in [
        // Source: synthetic-non-reachable; documented DE fixture shape.
        (
            "DE phone +49 1555 0112233",
            LocaleTag::DeDe,
            "phone.national.de",
            "+49 1555 0112233",
        ),
        // Source: NANPA 555-LINE Number Reservation.
        // https://nationalnanpa.com/number_resource_info/555_numbers.html
        (
            "US phone +1 555 0100",
            LocaleTag::EnUs,
            "phone.national.us",
            "+1 555 0100",
        ),
        // Source: parser-valid contiguous E.164 fixture from existing tests.
        (
            "US phone +12025550100",
            LocaleTag::EnUs,
            "phone.structural",
            "+12025550100",
        ),
    ] {
        assert_eq!(
            detect_recognizer(&rulepack, recognizer_id, input, locale.clone()),
            vec![expected.to_string()],
            "{input}"
        );

        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, input, locale);
        assert!(!clean.contains(expected), "{expected} leaked in {clean}");
        assert_eq!(clean.matches(":Custom:phone_").count(), 1, "{clean}");
        assert_eq!(restore_tokens(&session, &clean), input);
    }
}

#[test]
fn overlapping_phone_recognizers_pick_single_winners_without_span_loss() {
    let rulepack = core_extended();
    let entries = Arc::new(Mutex::new(Vec::new()));
    let pipeline = pipeline_from_rulepack(&rulepack).with_redaction_logger(CapturingLogger {
        entries: Arc::clone(&entries),
    });
    let session = Session::new(Scope::Ephemeral).expect("session");
    // Source: synthetic-non-reachable per CONTRIBUTING.md phone fixture guidance:
    // DE uses documented parser-valid placeholder mobile; UK uses
    // libphonenumber example mobile shapes.
    let input = "DE +49 1555 0112233 UK +44 7400 123456 INTL +447400123456";

    let clean = clean_text(&pipeline, &session, input, LocaleTag::DeDe);

    for raw in ["+49 1555 0112233", "+44 7400 123456", "+447400123456"] {
        assert!(!clean.contains(raw), "{raw} leaked in {clean}");
    }
    assert_eq!(clean.matches(":Custom:phone_").count(), 3, "{clean}");
    assert_eq!(restore_tokens(&session, &clean), input);

    let entries = entries.lock().unwrap();
    let winner_ids = entries
        .iter()
        .filter(|entry| !entry.conflict_loser)
        .filter(|entry| matches!(&entry.class, PiiClass::Custom(name) if name == "phone"))
        .filter_map(|entry| entry.recognizer_id.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        winner_ids,
        vec![
            "phone.national.de",
            "phone.e164.spaced",
            "phone.structural+phone.e164.spaced"
        ],
        "{entries:?}"
    );
    assert!(
        entries.iter().any(|entry| {
            entry.conflict_loser
                && matches!(&entry.class, PiiClass::Custom(name) if name == "phone")
                && entry.recognizer_id.as_deref() == Some("phone.e164.spaced")
        }),
        "expected phone.e164.spaced to lose at least one same-class overlap: {entries:?}"
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
        // The subscriber runs are zero-padded synthetic placeholders.
        ("Phone 040 0000 0000", "040 0000 0000"),
        ("Phone 089 0000 0000", "089 0000 0000"),
        ("Phone 089 0000 0000", "089 0000 0000"),
        ("Phone 069 0000 0000", "069 0000 0000"),
        ("Phone 030 0000 0000", "030 0000 0000"),
        ("Phone 030 0000 0000", "030 0000 0000"),
        ("Phone +49 89 0000 0000", "+49 89 0000 0000"),
        // Source: synthetic-non-reachable; Germany has no official fictional
        // range equivalent to NANPA 555-01XX, so these literals are
        // parser-valid but intentionally non-routable-looking test shapes.
        ("Phone +49 40 0000 0000", "+49 40 0000 0000"),
        ("Phone +49 89 0000 0000", "+49 89 0000 0000"),
        ("Phone +49 69 0000 0000", "+49 69 0000 0000"),
        ("Phone +49 221 0000 000", "+49 221 0000 000"),
        ("Phone +49 711 0000 000", "+49 711 0000 000"),
        // Source: synthetic-non-reachable per CONTRIBUTING.md:42
        // (zero-exchange-code carve-out). ONKs are real BNetzA-assigned
        // values so the 3-/4-digit alternation branches are exercised;
        // subscriber runs are zero-padded so the full literals are
        // unambiguously non-routable.
        ("Phone 0221 0000 0000", "0221 0000 0000"),
        ("Phone 0711 0000 0000", "0711 0000 0000"),
        ("Phone 0201 0000 0000", "0201 0000 0000"),
        ("Phone 0421 0000 0000", "0421 0000 0000"),
        ("Phone 0202 0000 0000", "0202 0000 0000"),
        ("Phone 0251 0000 0000", "0251 0000 0000"),
        ("Phone 0521 0000 0000", "0521 0000 0000"),
        ("Phone 0211 0000 0000", "0211 0000 0000"),
        ("Phone 0911 0000 0000", "0911 0000 0000"),
        ("Phone +49 221 0000 000", "+49 221 0000 000"),
        ("Phone 0341 0000 0000", "0341 0000 0000"),
        ("Phone 02202 0000 000", "02202 0000 000"),
        ("Phone 0911 1234", "0911 1234"),
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
    // Source: RFC 6943 Section 3.1.1. Parser-backed validation must reject
    // non-decimal and out-of-range IPv4 candidates even if regexes change later.
    assert!(
        detect_recognizer(&rulepack, "ip.v4", "Host 192.000.2.1.", LocaleTag::EnUs).is_empty(),
        "ip.v4 must reject leading-zero octets via Ipv4Parse"
    );
    assert!(
        detect_recognizer(&rulepack, "ip.v4", "Host 256.0.0.1.", LocaleTag::EnUs).is_empty(),
        "ip.v4 must reject out-of-range octets"
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
    let pipeline = pipeline_from_rulepack(&rulepack);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let cleaned = clean_text(
        &pipeline,
        &session,
        "Server ::ffff:192.0.2.1 logged",
        LocaleTag::EnUs,
    );
    assert_eq!(
        restore_tokens(&session, &cleaned),
        "Server ::ffff:192.0.2.1 logged",
        "IPv4-embedded IPv6 wrapper must redact and restore as one unit"
    );
    assert!(
        !cleaned.contains("192.0.2.1"),
        "cleaned text leaked inner IPv4 octets: {cleaned:?}"
    );

    {
        let input = "Server 192.0.2.1 logged";
        let matches = detect_recognizer(&rulepack, "ip.v4", input, LocaleTag::EnUs);
        assert_eq!(matches, vec!["192.0.2.1".to_string()]);
    }
    {
        let input = "Forwarding ::ffff:198.51.100.7 to upstream";
        let matches = detect_recognizer(&rulepack, "ip.v6", input, LocaleTag::EnUs);
        assert_eq!(matches, vec!["::ffff:198.51.100.7".to_string()]);
    }
    // drift-ack: v0.6.5 validator bundle adds eth.address and parser-backed
    // IP fixture coverage to the core/core-extended no-policy drift snapshots.
    // Source: EIP-55 "Test Cases". Mixed-case addresses must pass only when
    // the Keccak-256 case checksum is correct; all-lower legacy form is
    // accepted per the EIP.
    assert_eq!(
        detect_recognizer(
            &rulepack,
            "eth.address",
            "Wallet 0x52908400098527886E0F7030069857D2E4169EE7.",
            LocaleTag::EnUs
        ),
        vec!["0x52908400098527886E0F7030069857D2E4169EE7".to_string()]
    );
    assert!(
        detect_recognizer(
            &rulepack,
            "eth.address",
            "Wallet 0x52908400098527886e0F7030069857D2E4169EE7.",
            LocaleTag::EnUs
        )
        .is_empty(),
        "eth.address must reject mixed-case checksum mutants"
    );
    assert_eq!(
        detect_recognizer(
            &rulepack,
            "eth.address",
            "Wallet 0xde709f2102306220921060314715629080e2fb77.",
            LocaleTag::EnUs
        ),
        vec!["0xde709f2102306220921060314715629080e2fb77".to_string()]
    );
    assert_eq!(
        detect_recognizer(
            &rulepack,
            "eth.address",
            "Pair 0x52908400098527886E0F7030069857D2E4169EE7;0x8617E340B3D01FA5F11F306F4090FD50E238070D",
            LocaleTag::EnUs
        ),
        vec![
            "0x52908400098527886E0F7030069857D2E4169EE7".to_string(),
            "0x8617E340B3D01FA5F11F306F4090FD50E238070D".to_string(),
        ]
    );
    {
        let input = "Wallet 0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359 approved";
        let matches = detect_recognizer(&rulepack, "eth.address", input, LocaleTag::EnUs);
        assert_eq!(
            matches,
            vec!["0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359".to_string()],
            "EthEip55 must preserve original matched bytes verbatim"
        );
    }
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

    for (recognizer_id, locale, input, expected) in [
        ("ssn.us", LocaleTag::EnUs, "SSN 123-45-6789", "123-45-6789"),
        (
            "ssn.us",
            LocaleTag::EnUs,
            "Social Security Number:\n123456789",
            "123456789",
        ),
        (
            "nino.uk",
            LocaleTag::EnGb,
            "National Insurance Number AB123456C",
            "AB123456C",
        ),
        (
            "nino.uk",
            LocaleTag::EnGb,
            "NI Number: AB 12 34 56 C",
            "AB 12 34 56 C",
        ),
        (
            "pan.in",
            LocaleTag::Other("en-IN".to_string()),
            "PAN card ABCPA1234F",
            "ABCPA1234F",
        ),
        (
            "pan.in",
            LocaleTag::Other("hi-IN".to_string()),
            "पैन ABCPA1234F",
            "ABCPA1234F",
        ),
    ] {
        assert_eq!(
            detect_recognizer(&rulepack, recognizer_id, input, locale),
            vec![expected.to_string()],
            "{input}"
        );
    }

    for (recognizer_id, locale, input) in [
        ("ssn.us", LocaleTag::EnUs, "Reference 123-45-6789"),
        ("nino.uk", LocaleTag::EnGb, "Reference AB123456C"),
        ("nino.uk", LocaleTag::EnGb, "NINO GB123456C"),
        (
            "pan.in",
            LocaleTag::Other("en-IN".to_string()),
            "Reference ABCPA1234F",
        ),
        (
            "pan.in",
            LocaleTag::Other("en-IN".to_string()),
            "PAN ABCDA1234F",
        ),
    ] {
        assert!(
            detect_recognizer(&rulepack, recognizer_id, input, locale).is_empty(),
            "{recognizer_id} must not fire for {input}"
        );
    }

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
        "Order 0044 0532 01",
        "Build at 2026-04-30 0040 12345",
        "Customer_4915550112233",
        "Order_4915550112233",
        "Order_15551234567",
        "Steuer-ID 12345678901",
        "Steuer-ID 12 345 678 901",
        "IBAN fragment 3704 0044 0532",
        "Postal code 10115",
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
    assert!(
        !clean.contains(":Custom:phone_"),
        "phone recognizer must not fire inside formatted DE IBAN: {clean}"
    );
    assert_eq!(restore_tokens(&session, &clean), input);
}

#[test]
fn phase2_mod97_failing_iban_shape_can_still_tokenize_phone_tail() {
    let rulepack = core_extended();
    let input = "My IBAN DE12 3456 7890 01555 0112233";

    assert!(
        detect_recognizer(&rulepack, "iban.structural", input, LocaleTag::DeDe).is_empty(),
        "iban.structural must reject mod-97-failing IBAN shapes"
    );
    assert_eq!(
        detect_recognizer(&rulepack, "phone.national.de", input, LocaleTag::DeDe),
        vec!["01555 0112233".to_string()]
    );

    let pipeline = pipeline_from_rulepack(&rulepack);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(&pipeline, &session, input, LocaleTag::DeDe);
    assert_custom_token(&clean, "phone");
    assert!(
        !clean.contains(":Custom:iban_"),
        "invalid IBAN shape must not produce an IBAN token: {clean}"
    );
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
    assert_eq!(iban.cooperates_with, vec!["card.structural"]);
    assert_eq!(card.cooperates_with, vec!["iban.structural"]);
    assert_eq!(
        iban.collision
            .as_ref()
            .map(|collision| collision.family.as_str()),
        Some("payment-card-or-iban")
    );
    assert_eq!(
        card.collision
            .as_ref()
            .map(|collision| collision.family.as_str()),
        Some("payment-card-or-iban")
    );
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
        (
            "Wallet 0x52908400098527886E0F7030069857D2E4169EE7",
            LocaleTag::EnUs,
        ),
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
    let phone_spaced = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "phone.e164.spaced")
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
        vec![
            "phone.e164.spaced",
            "phone.national.de",
            "phone.national.us"
        ]
    );
    assert_eq!(
        phone_spaced.cooperates_with,
        vec!["phone.structural", "phone.national.de", "phone.national.us"]
    );
    assert_eq!(
        phone_de.cooperates_with,
        vec!["phone.structural", "phone.e164.spaced", "phone.national.us"]
    );
    assert_eq!(
        phone_us.cooperates_with,
        vec!["phone.structural", "phone.e164.spaced", "phone.national.de"]
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
