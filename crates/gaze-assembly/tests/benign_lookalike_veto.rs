//! Benign lookalike veto on the bundled `core` + locale rulepacks, end to end.
//!
//! Direction 1: a weak, cue-less postal or phone shape inside an order number,
//! an amount, a long SKU or after a room label, and a loopback address, leaves
//! raw, and each veto writes a loser row with its typed reason.
//! Direction 2: the same shapes as real PII, or with a cue, still tokenize.
use std::sync::{Arc, Mutex};

use gaze::{
    CleanDocument, ConflictTier, Context, DictionaryBundle, LocaleChain, Policy, RawDocument,
    RedactionEntry, RedactionLogError, RedactionLogger, Rulepack, RulepackSource, SafetyNetPolicy,
    Scope, Session,
};
use gaze_assembly::build_pipeline_builder;
use gaze_recognizers::ValidatorFailReason;

#[derive(Clone, Default)]
struct MemoryLogger(Arc<Mutex<Vec<RedactionEntry>>>);
impl RedactionLogger for MemoryLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.0.lock().expect("entries").push(entry.clone());
        Ok(())
    }
}

fn policy(locale: &str) -> Policy {
    let text = format!(
        "schema_version = \"0.1.0\"\n\n[session]\nscope = \"persistent\"\nttl_secs = 86400\n\n\
         [policy.rulepacks]\nbundled = [\"core\", \"locale-de\", \"locale-en\"]\n\n\
         [locale]\nactive = [\"{locale}\"]\n\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n"
    );
    let path = std::env::temp_dir().join(format!(
        "gaze-benign-lookalike-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, text).expect("write policy");
    let policy = Policy::load(&path).expect("policy");
    let _ = std::fs::remove_file(&path);
    policy
}

fn rulepacks() -> &'static [Rulepack; 3] {
    static PACKS: std::sync::OnceLock<[Rulepack; 3]> = std::sync::OnceLock::new();
    PACKS.get_or_init(|| {
        ["core", "locale-de", "locale-en"].map(|name| {
            Rulepack::load(RulepackSource::Embedded(
                gaze_recognizers::embedded(name).expect("embedded rulepack"),
            ))
            .expect("rulepack")
        })
    })
}

/// Every benign-lookalike or loopback veto row, in log order: (recognizer, reason). No raw
/// value is recorded; exact multiplicity is kept.
type Vetoes = Vec<(String, ValidatorFailReason)>;

fn clean_document(locale: &str, document: RawDocument) -> (CleanDocument, Vetoes) {
    let policy = policy(locale);
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let logger = MemoryLogger::default();
    let pipeline = build_pipeline_builder(&policy, &context, rulepacks(), &active, None)
        .expect("builder")
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    // Structured documents take the plain pseudonymize path: the safety-net resolve mode
    // refuses them.
    let clean = match document {
        RawDocument::Structured(_) => pipeline
            .pseudonymize_with_detect_context(
                &session,
                document,
                active.as_slice(),
                &DictionaryBundle::default(),
            )
            .expect("clean"),
        document => {
            pipeline
                .clean_with_safety_net_policy_detect_context(
                    &session,
                    document,
                    active.as_slice(),
                    &DictionaryBundle::default(),
                    SafetyNetPolicy::default(),
                )
                .expect("clean")
                .0
        }
    };
    let vetoes = logger
        .0
        .lock()
        .expect("entries")
        .iter()
        .filter(|entry| {
            matches!(
                entry.validator_fail_reason,
                Some(
                    ValidatorFailReason::BenignDigitRunFragment
                        | ValidatorFailReason::Ipv4LoopbackRange
                        | ValidatorFailReason::Ipv6LoopbackRange
                )
            )
        })
        .map(|entry| {
            assert_eq!(entry.decided_by, ConflictTier::ValidatorVeto);
            assert!(entry.conflict_loser, "a veto row is a loser row");
            (
                entry
                    .recognizer_id
                    .clone()
                    .expect("veto rows name their recognizer"),
                entry.validator_fail_reason.expect("filtered on a reason"),
            )
        })
        .collect();
    (clean, vetoes)
}

/// Clean `input` under one document locale; returns the clean text with token session
/// prefixes removed and every veto row.
fn clean(locale: &str, input: &str) -> (String, Vetoes) {
    let (clean, vetoes) = clean_document(locale, RawDocument::Text(input.to_string()));
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    let text = regex::Regex::new(r"<[0-9a-f]{8}:")
        .expect("regex")
        .replace_all(&text, "<")
        .into_owned();
    (text, vetoes)
}

/// Benign text leaves raw and writes exactly the expected veto rows, one per vetoed
/// candidate, in any order.
fn assert_raw(locale: &str, input: &str, expected: &[(&str, ValidatorFailReason)]) {
    let (text, mut vetoes) = clean(locale, input);
    assert_eq!(text, input, "{locale}: benign text must leave raw");
    vetoes.sort_by_key(|row| format!("{row:?}"));
    let mut expected: Vetoes = expected
        .iter()
        .map(|(source, reason)| (source.to_string(), *reason))
        .collect();
    expected.sort_by_key(|row| format!("{row:?}"));
    assert_eq!(vetoes, expected, "{locale}: {input}");
}

#[test]
fn value_proven_lookalikes_leave_raw_with_one_row_per_veto() {
    use ValidatorFailReason::*;
    // A strict part of a 16-digit same-separator run cannot be a phone number.
    assert_raw(
        "de-DE",
        "level=info svc=orders itemCode=0593-9506-3395-7573 status=done",
        &[("phone.national.de", BenignDigitRunFragment)],
    );
    // Loopback addresses never leave the host.
    assert_raw(
        "en-US",
        "values: 127.0.0.8 ::ffff:127.0.0.5 ::1 done",
        &[
            ("ip.v4", Ipv4LoopbackRange),
            ("ip.v4", Ipv4LoopbackRange),
            ("ip.v6", Ipv6LoopbackRange),
            ("ip.v6", Ipv6LoopbackRange),
        ],
    );
}

/// The documented cost of the reduced contract: order-number tails and amounts are no longer
/// vetoed, because only a missing label could call them benign. They tokenize like any other
/// postcode- or phone-shaped value, and write no benign row.
#[test]
fn reference_numbers_and_amounts_are_tokenized() {
    for (locale, input, value) in [
        (
            "en-US",
            "Lagerartikel SKU-DEMO-73821 und Charge BATCH-SAMPLE-92163; fertig.",
            "73821",
        ),
        (
            "en-US",
            "Lagerartikel SKU-DEMO-73821 und Charge BATCH-SAMPLE-92163; fertig.",
            "92163",
        ),
        ("de-DE", "invoice INVOICE-TEST-03687455, done", "03687455"),
        (
            "en-US",
            "level=info svc=orders grandTotal=EUR 22186,12 status=done",
            "22186",
        ),
        (
            "en-US",
            "Rechnung RECHNUNG-2026-80331 an 80331 Boston",
            "80331",
        ),
    ] {
        assert_protected(locale, input, value);
    }
    let (leaves, vetoes) = clean_structured(
        "en-US",
        &[(
            "order",
            object(&[
                ("ref", string("ORDER-90210")),
                ("total", string("EUR 22186,12")),
            ]),
        )],
    );
    assert!(
        leaves
            .iter()
            .all(|leaf| !leaf.contains("90210") && !leaf.contains("22186")),
        "{leaves:?}"
    );
    assert!(vetoes.is_empty(), "{vetoes:?}");
}

#[test]
fn a_trailing_cue_a_field_name_or_a_city_anchor_keeps_the_value_protected() {
    // Review 10815 probes: each leaked raw in an early round.
    assert_protected("en-US", "ORDER-90210 (ZIP)", "90210");
    assert_protected("en-US", "ORDER-212-555-0187 (phone)", "555-0187");
    assert_protected("de-AT", "Room 1010 Wien", "1010");
    assert_protected("en-US", r#"{"postal_code":"ORDER-90210"}"#, "90210");
    assert_protected("en-US", "ZIP:\nORDER-90210", "90210");
    assert_protected("de-DE", "Telefon:\nORDER-0301234567", "0301234567");
    // Structured fields, labelled or not: reference tails are no longer vetoed at all.
    for field in [
        "postal_code",
        "zipCode",
        "phone",
        "orderRef",
        "shippingAddress",
        "billing_address",
        "delivery-address",
        "address",
        "Anschrift",
        "adresse",
        "street",
        "city",
    ] {
        let (leaves, vetoes) = clean_structured("en-US", &[(field, string("ORDER-90210"))]);
        assert!(
            leaves.iter().all(|leaf| !leaf.contains("90210")),
            "{field}: {leaves:?}"
        );
        assert!(vetoes.is_empty(), "{field}: {vetoes:?}");
    }
    // A non-loopback IPv4-mapped address stays protected.
    assert_protected("en-US", "peer ::ffff:84.12.3.4 up", "84.12.3.4");
}

fn assert_protected(locale: &str, input: &str, value: &str) {
    let (text, vetoes) = clean(locale, input);
    assert!(
        !text.contains(value),
        "{locale}: {value} must not leave raw in {input:?}, got {text:?}"
    );
    assert!(
        vetoes.is_empty(),
        "{locale}: no benign veto expected, got {vetoes:?}"
    );
}

#[test]
fn real_postcodes_phones_and_addresses_still_tokenize() {
    // Plain and country-prefixed postcodes; a postcode next to an unrelated amount.
    assert_protected("en-US", "Beverly Hills, CA 90210", "90210");
    assert_protected("en-US", "D-80331 München", "80331");
    assert_protected("en-US", "Rechnung an 80331 München, EUR 50", "80331");
    assert_protected("de-AT", "Mariahilfer Straße 1, 1060 Wien", "1060");
    // A security token is not a reference number: its digits stay covered as before.
    assert_protected("en-US", "token ASDFG-98765-ZXCVB here", "98765");
    // A cue word before the value always wins over a benign structure.
    assert_protected("en-US", "ZIP: EUR 90210", "90210");
    assert_protected("de-DE", "Telefon: SKU-0301234567", "0301234567");
    assert_protected("de-AT", "PLZ Raum 1010 Wien", "1010");
    // Phones with direct-dial extensions stay far below a 16-digit run.
    assert_protected("de-DE", "Bitte rufen Sie 089/12345-0 an", "12345");
    assert_protected("en-US", "Call (212) 555-0187 USD", "555-0187");
    // Routable, private and link-local addresses stay protected; only loopback is benign.
    assert_protected("en-US", "host 192.168.1.20 up", "192.168.1.20");
    assert_protected("en-US", "host 10.0.0.7 up", "10.0.0.7");
    assert_protected(
        "en-US",
        "peer fe80::21a:2bff:fe3c:4d5e up",
        "fe80::21a:2bff:fe3c:4d5e",
    );
}

/// Real PII right next to a benign lookalike: the lookalike may leave raw, the
/// PII never does.
fn assert_value_protected(locale: &str, input: &str, value: &str) {
    let (text, _) = clean(locale, input);
    assert!(
        !text.contains(value),
        "{locale}: {value} must not leave raw in {input:?}, got {text:?}"
    );
}

#[test]
fn cued_real_pii_next_to_lookalikes_still_tokenizes() {
    // A cued phone beside an order number and a long SKU.
    assert_value_protected(
        "de-DE",
        "Tel: 030 1234567, Bestellung ORDER-2026-145684",
        "1234567",
    );
    assert_value_protected(
        "de-DE",
        "Telefon 0301234567 itemCode=0593-9506-3395-7573",
        "0301234567",
    );
    // A phone cue in front of a digit run keeps even the fragment protected.
    assert_value_protected("de-DE", "Phone: 0593-9506-3395-7573", "9506-3395");
    // Financial identifiers beside amounts: checksum rules never opt in.
    assert_value_protected(
        "en-US",
        "IBAN DE89 3704 0044 0532 0130 00 EUR 500,00",
        "3704 0044 0532",
    );
    assert_value_protected(
        "en-US",
        "Card 4539 1488 0343 6467 USD 12.00",
        "4539 1488 0343 6467",
    );
    assert_value_protected(
        "en-US",
        "paid $12.00 by card 4539148803436467",
        "4539148803436467",
    );
    // Cued and plain postcodes beside references, amounts and rooms.
    assert_value_protected("en-US", "ZIP 90210, SKU-DEMO-73821", "90210");
    assert_value_protected(
        "en-US",
        "SKU-DEMO-73821 ships to Beverly Hills, CA 90210",
        "90210",
    );
    assert_value_protected("de-AT", "PLZ: 1010 Wien, Room 4833", "1010");
    assert_value_protected("en-US", "EUR 12,00 to 10115 Berlin", "10115");
    // The same five digits: vetoed as an order tail, still protected as a postcode.
    // The same five digits as an order tail and as a postcode: both tokenized.
    assert_value_protected(
        "de-DE",
        "Rechnung RECHNUNG-2026-80331 an 80331 München",
        "80331",
    );
    // An email and a routable IP next to loopback and a reference stay protected.
    assert_value_protected(
        "en-US",
        "SKU-DEMO-73821 owner anna@example.org",
        "anna@example.org",
    );
    assert_value_protected(
        "en-US",
        "proxy 127.0.0.1 forwarded client 84.12.3.4",
        "84.12.3.4",
    );
    assert_value_protected(
        "en-US",
        "::1 and 2a00:1450:4001:82a::200e",
        "2a00:1450:4001:82a::200e",
    );
}

/// The bundled opt-in set is exactly the two national phone rules with the one structure a value
/// can prove by itself. Adding a rule or a structure here needs a leak-direction review first.
#[test]
fn only_the_audited_bundled_rules_declare_benign_lookalikes() {
    let declared: Vec<(String, Vec<String>)> = rulepacks()
        .iter()
        .flat_map(|pack| pack.recognizers.iter())
        .filter_map(|recognizer| {
            let context = recognizer.context.as_ref()?;
            (!context.benign_lookalikes.is_empty())
                .then(|| (recognizer.id.clone(), context.benign_lookalikes.clone()))
        })
        .collect();
    let pair = |id: &str| (id.to_string(), vec!["digit_run_fragment".to_string()]);
    assert_eq!(
        declared,
        vec![pair("phone.national.de"), pair("phone.national.us")]
    );
}

fn load_pack(recognizer: &str) -> Result<Rulepack, gaze::RulepackError> {
    let text = format!(
        "schema_version = \"0.1.0\"\nrulepack_id = \"probe\"\nrulepack_version = \"0.1.0\"\n\
         default_locales = [\"global\"]\n\n[[recognizers]]\n{recognizer}"
    );
    let path = std::env::temp_dir().join(format!(
        "gaze-benign-pack-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, text).expect("write pack");
    let pack = Rulepack::load(RulepackSource::Path(path.clone()));
    let _ = std::fs::remove_file(&path);
    pack
}

fn bundled_pack(recognizer: &str) -> Result<Rulepack, gaze::RulepackError> {
    Rulepack::parse_bundled(&format!(
        "schema_version = \"0.1.0\"\nrulepack_id = \"probe\"\nrulepack_version = \"0.1.0\"\n\
         default_locales = [\"global\"]\n\n[[recognizers]]\n{recognizer}"
    ))
}

fn refused(pack: Result<Rulepack, gaze::RulepackError>) -> bool {
    matches!(
        pack,
        Err(gaze::RulepackError::IneligibleBenignLookalike { .. })
    )
}

fn phone_rule(id: &str, pattern: &str, extra: &str) -> String {
    format!(
        "id = \"{id}\"\nclass = \"custom:phone\"\nlocales = [\"en-US\"]\n\
         locale_basis = \"format\"\n[recognizers.match]\nkind = \"regex\"\n\
         pattern = '''{pattern}'''\n{extra}\
         [recognizers.context]\nbenign_lookalikes = [\"digit_run_fragment\"]\n"
    )
}

#[test]
fn the_loader_refuses_benign_lookalikes_outside_the_audited_bundled_rules() {
    let plain = phone_rule("phone.national.us", r"\b\d{10}\b", "");
    assert!(
        bundled_pack(&plain).is_ok(),
        "an audited id in a bundled pack passes the loader; the grant decides later"
    );
    // A custom id, from a file or handed to the bundled parser; an audited id from a file.
    let custom = phone_rule(
        "custom.order_phone",
        r"ORDER-(\d{10})\s+Beverly",
        "capture_groups = [1]\n",
    );
    assert!(refused(load_pack(&custom)));
    assert!(refused(bundled_pack(&custom)));
    assert!(refused(load_pack(&plain)));
    // The retired postal ids are no longer eligible anywhere.
    assert!(refused(bundled_pack(
        &plain.replace("phone.national.us", "postal.us")
    )));
    // Defence in depth on an audited id: a mandatory anchor or a non-regex matcher.
    assert!(refused(bundled_pack(&phone_rule(
        "phone.national.us",
        r"\b\d{10}\b",
        "[recognizers.collision]\nfamily = \"probe-family\"\nvariant = \"a\"\nprecedence = 10\n\
         mandatory_anchor = \"iban\"\n"
    ))));
    assert!(refused(bundled_pack(
        "id = \"phone.national.us\"\nclass = \"custom:phone\"\nlocales = [\"en-US\"]\n\
         locale_basis = \"format\"\n[recognizers.match]\nkind = \"dictionary\"\n\
         terms = [\"2125550187\"]\n[recognizers.context]\nbenign_lookalikes = [\"digit_run_fragment\"]\n"
    )));
}

/// The exact bundled `phone.national.us` rule, rebuilt from the embedded core spec the same way
/// assembly wires it, so these probes borrow a genuine grant.
fn bundled_phone_us() -> gaze_recognizers::RegexDetector {
    let spec = rulepacks()[0]
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "phone.national.us")
        .expect("bundled phone.national.us");
    let gaze::RawMatch::Regex {
        pattern: Some(pattern),
        capture_groups,
        ..
    } = &spec.matcher
    else {
        panic!("phone.national.us is a plain regex rule");
    };
    let validator = spec.validator.as_ref().map(|validator| {
        gaze_recognizers::ValidatorKind::parse(&validator.kind).expect("bundled validator")
    });
    gaze_recognizers::RegexDetector::with_rulepack_fields(
        pattern,
        spec.class.clone(),
        &spec.id,
        spec.locales.clone(),
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups.clone(),
        Vec::new(),
        validator,
        None,
    )
    .expect("detector")
    .with_locale_basis(spec.locale_basis)
    .with_benign_lookalikes(vec![gaze_recognizers::BenignLookalike::DigitRunFragment])
    .expect("the exact bundled phone.national.us rule is granted")
}

#[test]
fn the_regex_builder_refuses_rules_that_are_not_exactly_audited() {
    let custom = gaze_recognizers::RegexDetector::with_rulepack_fields(
        r"ORDER-(\d{10})\s+Beverly",
        gaze::PiiClass::custom("phone").expect("class"),
        "custom.order_phone",
        vec![gaze::LocaleTag::EnUs],
        0.7,
        0,
        "counter",
        Some(vec![1]),
        Vec::new(),
        None,
        None,
    )
    .expect("detector");
    assert!(matches!(
        custom.with_benign_lookalikes(vec![gaze_recognizers::BenignLookalike::DigitRunFragment]),
        Err(gaze_recognizers::RecognizerError::UnsupportedBenignLookalike { .. })
    ));
    let _ = bundled_phone_us();
}

/// Spoof 1: a rule that borrows the audited id `phone.national.us` with another pattern, handed
/// to the bundled parser, parses but mints no grant, so building the pipeline fails closed.
#[test]
fn a_spoofed_bundled_rule_gets_no_grant() {
    let spoof = phone_rule(
        "phone.national.us",
        r"ORDER-(\d{10})\s+Beverly",
        "capture_groups = [1]\n",
    );
    let text: &'static str = Box::leak(
        format!(
            "schema_version = \"0.1.0\"\nrulepack_id = \"probe\"\nrulepack_version = \"0.1.0\"\n\
             default_locales = [\"global\"]\n\n[[recognizers]]\n{spoof}"
        )
        .into_boxed_str(),
    );
    let pack = Rulepack::load(RulepackSource::Embedded(text)).expect("the id is audited");
    let policy = policy("en-US");
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let error = build_pipeline_builder(&policy, &context, &[pack], &active, None)
        .err()
        .expect("a spoofed audited id must not build");
    assert!(
        format!("{error:?}").contains("UnsupportedBenignLookalike"),
        "{error:?}"
    );
}

/// A custom recognizer that presents a real grant it borrowed, and emits `span` for the phone
/// class. `global` locales stand for "gates differently from the audited rule".
struct Borrowed {
    real: gaze_recognizers::RegexDetector,
    span_of: &'static str,
    global: bool,
}

impl gaze::Recognizer for Borrowed {
    fn id(&self) -> &str {
        gaze::Recognizer::id(&self.real)
    }
    fn supported_class(&self) -> &gaze::PiiClass {
        gaze::Recognizer::supported_class(&self.real)
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn locales(&self) -> &[gaze::LocaleTag] {
        if self.global {
            &[gaze::LocaleTag::Global]
        } else {
            gaze::Recognizer::locales(&self.real)
        }
    }
    fn locale_basis(&self) -> gaze::LocaleBasis {
        gaze::Recognizer::locale_basis(&self.real)
    }
    fn validator_kind(&self) -> Option<gaze_recognizers::ValidatorKind> {
        gaze::Recognizer::validator_kind(&self.real)
    }
    fn detect(
        &self,
        input: &str,
        _: &gaze::DetectContext<'_>,
    ) -> Result<Vec<gaze::Candidate>, gaze::DetectError> {
        Ok(input
            .find(self.span_of)
            .map(|start| {
                gaze::Candidate::new(
                    start..start + self.span_of.len(),
                    gaze::Recognizer::supported_class(&self.real).clone(),
                    "phone.national.us",
                    0.9,
                    90,
                    None,
                    "counter",
                    "phone.national.us",
                    ConflictTier::None,
                    Vec::new(),
                )
            })
            .into_iter()
            .collect())
    }
    fn benign_lookalike_grant(&self) -> Option<&gaze_recognizers::BenignLookalikeGrant> {
        gaze::Recognizer::benign_lookalike_grant(&self.real)
    }
}

fn clean_with(recognizer: Borrowed, locale: &str, input: &str) -> String {
    let policy = policy(locale);
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline_builder(&policy, &context, rulepacks(), &active, None)
        .expect("builder")
        .recognizer(recognizer)
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(input.to_string()),
            active.as_slice(),
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .expect("clean");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    text
}

/// Spoof 2: a borrowed genuine grant on a span the audited pattern never emits: a compact
/// `2125550187` passes the US phone validator, but the bundled pattern needs separators, so it
/// never emits it. Inside a 16-digit run the structure fires; validator veto re-matches the
/// audited pattern, so the candidate stays.
#[test]
fn a_borrowed_grant_vetoes_nothing_the_audited_pattern_does_not_emit() {
    let text = clean_with(
        Borrowed {
            real: bundled_phone_us(),
            span_of: "2125550187",
            global: false,
        },
        "en-US",
        "itemCode=2125550187-4444-99 status=done",
    );
    assert!(!text.contains("2125550187"), "{text}");
}

/// Spoof 3: a borrowed genuine grant presented by a recognizer that gates differently is not
/// the audited rule; its candidate on an audited-pattern match inside a long run is kept.
#[test]
fn a_borrowed_grant_with_another_identity_vetoes_nothing() {
    let text = clean_with(
        Borrowed {
            real: bundled_phone_us(),
            span_of: "212-555-0187",
            global: true,
        },
        "en-GB",
        "run 212-555-0187-4444-99 end",
    );
    assert!(!text.contains("212-555-0187"), "{text}");
}

#[test]
fn a_multi_branch_pattern_cannot_declare_benign_lookalikes() {
    let detector = gaze_recognizers::RegexDetector::with_rulepack_fields(
        r"(?:tel (\d{10})|(\d{10}) [A-Z][a-z]+)",
        gaze::PiiClass::custom("phone").expect("class"),
        "phone.national.us",
        vec![gaze::LocaleTag::EnUs],
        0.7,
        0,
        "counter",
        Some(vec![1, 2]),
        Vec::new(),
        None,
        None,
    )
    .expect("detector");
    assert!(matches!(
        detector.with_benign_lookalikes(vec![gaze_recognizers::BenignLookalike::DigitRunFragment]),
        Err(gaze_recognizers::RecognizerError::UnsupportedBenignLookalike { .. })
    ));
}

#[test]
fn a_recorded_failure_rule_cannot_declare_benign_lookalikes() {
    let detector = bundled_phone_us();
    let recording = gaze_recognizers::RegexDetector::with_rulepack_fields(
        r"\b\d{10}\b",
        gaze::PiiClass::custom("phone").expect("class"),
        "phone.national.us",
        vec![gaze::LocaleTag::EnUs],
        0.7,
        0,
        "counter",
        None,
        Vec::new(),
        // Not a checksum, so only the recorded-failure refusal can reject it.
        Some(
            gaze_recognizers::ValidatorKind::parse("e164_phone_national_us").expect("phone-parser"),
        ),
        None,
    )
    .expect("detector")
    .with_validator_on_fail(gaze_recognizers::ValidatorOnFail::Record)
    .expect("a US national phone may record");
    assert!(matches!(
        recording.with_benign_lookalikes(vec![gaze_recognizers::BenignLookalike::DigitRunFragment]),
        Err(gaze_recognizers::RecognizerError::UnsupportedBenignLookalike { .. })
    ));
    drop(detector);
}

fn object(fields: &[(&str, gaze::Value)]) -> gaze::Value {
    gaze::Value::Object(
        fields
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

fn string(text: &str) -> gaze::Value {
    gaze::Value::String(text.to_string())
}

/// The structured document with every string leaf, in path order.
fn leaves(value: &gaze::Value, out: &mut Vec<String>) {
    match value {
        gaze::Value::String(text) => out.push(text.clone()),
        gaze::Value::Array(values) => values.iter().for_each(|value| leaves(value, out)),
        gaze::Value::Object(fields) => fields.values().for_each(|value| leaves(value, out)),
        _ => {}
    }
}

fn clean_structured(locale: &str, root: &[(&str, gaze::Value)]) -> (Vec<String>, Vetoes) {
    let map = root
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect();
    let (clean, vetoes) = clean_document(locale, RawDocument::Structured(map));
    let CleanDocument::Structured(map) = clean else {
        panic!("expected structured");
    };
    let mut out = Vec::new();
    map.values().for_each(|value| leaves(value, &mut out));
    (out, vetoes)
}

/// A structured probe: the protected value, a name, and the document's top-level fields.
type Probe = (&'static str, &'static str, Vec<(&'static str, gaze::Value)>);

/// Review 10848, all nine probes: labelled values in nested records, sibling type or label
/// fields, block-level labels a few lines up, and French labels stay protected with no veto row.
#[test]
fn review_10848_labelled_values_stay_protected_with_no_veto_row() {
    let structured: [Probe; 4] = [
        (
            "90210",
            "shippingAddress.code",
            vec![(
                "shippingAddress",
                object(&[("code", string("ORDER-90210"))]),
            )],
        ),
        (
            "555-0187",
            "contact.value",
            vec![(
                "contact",
                object(&[("value", string("ORDER-212-555-0187"))]),
            )],
        ),
        (
            "555-0187",
            "type phone sibling",
            vec![(
                "item",
                object(&[
                    ("type", string("phone")),
                    ("value", string("ORDER-212-555-0187")),
                ]),
            )],
        ),
        (
            "90210",
            "label ZIP sibling",
            vec![(
                "item",
                object(&[("label", string("ZIP")), ("value", string("ORDER-90210"))]),
            )],
        ),
    ];
    for (value, name, root) in structured {
        let (leaves, vetoes) = clean_structured("en-US", &root);
        assert!(
            leaves.iter().all(|leaf| !leaf.contains(value)),
            "{name}: {leaves:?}"
        );
        assert!(vetoes.is_empty(), "{name}: {vetoes:?}");
    }
    for (locale, input, value) in [
        (
            "en-US",
            "ZIP for delivery:\nUse the customer value below.\nORDER-90210",
            "90210",
        ),
        (
            "de-DE",
            "Telefonnummer des Kunden:\nBitte den Wert unten verwenden.\nORDER-0301234567",
            "0301234567",
        ),
        ("en-US", "Téléphone: ORDER-212-555-0187", "555-0187"),
        (
            "en-US",
            "ZIP for shipping:\nUse the customer value below.\nEUR 90210",
            "90210",
        ),
        (
            "de-DE",
            "Telefon des Kunden:\nBitte den Wert unten verwenden.\n0593-9506-3395-7573",
            "0593-9506-3395",
        ),
    ] {
        assert_protected(locale, input, value);
    }
}

/// Counterweight: a long SKU in a structured record with no cue anywhere still leaves raw, one
/// audit row.
#[test]
fn structured_digit_run_fragment_still_leaves_raw_with_one_row() {
    let (leaves, vetoes) = clean_structured(
        "de-DE",
        &[(
            "items",
            gaze::Value::Array(vec![object(&[("itemCode", string("0593-9506-3395-7573"))])]),
        )],
    );
    assert_eq!(leaves, vec!["0593-9506-3395-7573"]);
    assert_eq!(
        vetoes,
        vec![(
            "phone.national.de".to_string(),
            ValidatorFailReason::BenignDigitRunFragment
        )]
    );
}

/// Drift guard: every phone label a bundled locale pack ships (`[locale.phone_labels]`) must be
/// a benign-lookalike phone cue, so a label the locale packs know can never be waved through.
#[test]
fn every_bundled_phone_label_is_a_benign_lookalike_cue() {
    let mut checked = 0;
    for name in [
        "locale-de",
        "locale-en",
        "locale-fr",
        "locale-nl",
        "locale-br",
        "locale-in",
        "locale-uk",
    ] {
        let pack = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded(name).expect("embedded locale pack"),
        ))
        .expect("locale pack");
        let Some(locale) = pack.locale.as_ref() else {
            continue;
        };
        if let Some(bucket) = locale.buckets.get("phone_labels") {
            for label in &bucket.names {
                assert!(
                    gaze_recognizers::CueEvidence::scan(label).phone(),
                    "{name} phone label {label:?} is not a cue"
                );
                checked += 1;
            }
        }
    }
    // de, fr, nl and br ship phone labels today; an empty scan would prove nothing.
    assert!(checked >= 15, "only {checked} phone labels checked");
}

/// Review 10848 round 2, and the brief's extras: labels far away, past a blank line, below the
/// value, in nested metadata, in a long type string, in a sibling array element, in another
/// script, or at the very end of a long document all keep the value protected with no veto row.
#[test]
fn a_cue_anywhere_in_the_document_or_record_keeps_the_value_protected() {
    let seven_notes = format!(
        "ZIP for delivery:\n{}ORDER-90210",
        "Record note, please keep it.\n".repeat(7)
    );
    let long_tail = format!(
        "ORDER-90210\n{}\nZIP",
        "Record note, please keep it. ".repeat(200)
    );
    for (input, value) in [
        (seven_notes.as_str(), "90210"),
        ("ZIP for delivery:\n\nORDER-90210", "90210"),
        ("ORDER-90210\n\nZIP for delivery:", "90210"),
        (long_tail.as_str(), "90210"),
        ("Телефон: ORDER-212-555-0187", "555-0187"),
        ("電話番号: ORDER-212-555-0187", "555-0187"),
        ("Bestellung ORDER-90210 заказ", "90210"),
    ] {
        assert_protected("en-US", input, value);
    }
    let structured: [Probe; 3] = [
        (
            "555-0187",
            "nested meta.type",
            vec![(
                "entry",
                object(&[
                    ("meta", object(&[("type", string("phone"))])),
                    ("value", string("ORDER-212-555-0187")),
                ]),
            )],
        ),
        (
            "555-0187",
            "long type string",
            vec![(
                "entry",
                object(&[
                    (
                        "type",
                        string(
                            "phone number for customer contact and delivery coordination, \
                             stored in the order record",
                        ),
                    ),
                    ("value", string("ORDER-212-555-0187")),
                ]),
            )],
        ),
        (
            "90210",
            "sibling array element",
            vec![(
                "items",
                gaze::Value::Array(vec![
                    object(&[("note", string("ZIP"))]),
                    object(&[("ref", string("ORDER-90210"))]),
                ]),
            )],
        ),
    ];
    for (value, name, root) in structured {
        let (leaves, vetoes) = clean_structured("en-US", &root);
        assert!(
            leaves.iter().all(|leaf| !leaf.contains(value)),
            "{name}: {leaves:?}"
        );
        assert!(vetoes.is_empty(), "{name}: {vetoes:?}");
    }
}

/// Single source with the benchmark generator (#722): the cue vocabulary the veto reads must
/// equal `scripts/bench/lookalike_cue_vocabulary.json` exactly, list by list and in order, so the
/// generated counterweights and the code can never drift apart. A missing file fails.
#[test]
fn cue_vocabulary_equals_the_benchmark_single_source() {
    use gaze_recognizers::benign_lookalike as v;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/bench/lookalike_cue_vocabulary.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} must exist: {error}", path.display()));
    let file: serde_json::Value = serde_json::from_str(&text).expect("vocabulary JSON");
    assert_eq!(file["schema_version"], 1);
    let list = |group: &str, family: &str| -> Vec<String> {
        file[group][family]
            .as_array()
            .unwrap_or_else(|| panic!("{group}.{family} must be a list"))
            .iter()
            .map(|word| word.as_str().expect("string").to_string())
            .collect()
    };
    let owned = |words: &[&str]| {
        words
            .iter()
            .map(|word| word.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(list("stems", "postal"), owned(v::POSTAL_STEMS));
    assert_eq!(list("stems", "phone"), owned(v::PHONE_STEMS));
    assert_eq!(list("stems", "address"), owned(v::ADDRESS_STEMS));
    assert_eq!(list("whole_words", "postal"), owned(v::POSTAL_WORDS));
    assert_eq!(list("whole_words", "phone"), owned(v::PHONE_WORDS));
    assert_eq!(list("whole_words", "address"), owned(v::ADDRESS_WORDS));
}

/// Review 10848 round 3: labels a cue list cannot read (markup, zero-width, decomposed accents,
/// Polish, Hungarian, Finnish, typos, percent- and base64-encoded labels). With the reference
/// and amount structures gone, every one of them stays protected with no benign row.
#[test]
fn review_10848_round_3_labels_stay_protected_with_no_benign_row() {
    for (input, value) in [
        ("Ph<b>one</b>: ORDER-212-555-0187", "555-0187"),
        ("Ph\u{200b}one: ORDER-212-555-0187", "555-0187"),
        ("Z<b>IP</b>: ORDER-90210", "90210"),
        ("Te\u{301}l.: ORDER-212-555-0187", "555-0187"),
        ("Kod pocztowy: ORDER-90210", "90210"),
        ("Irányítószám: ORDER-90210", "90210"),
        ("Puhelinnumero: ORDER-212-555-0187", "555-0187"),
        ("Phne: ORDER-212-555-0187", "555-0187"),
        ("Potsal: ORDER-90210", "90210"),
        ("%70%68%6F%6E%65: ORDER-212-555-0187", "555-0187"),
        ("cGhvbmU=: ORDER-212-555-0187", "555-0187"),
    ] {
        assert_protected("en-US", input, value);
    }
    let (leaves, vetoes) = clean_structured(
        "en-US",
        &[
            ("metadata", object(&[("kind", string("%5A%49%50"))])),
            ("entry", object(&[("ref", string("ORDER-90210"))])),
        ],
    );
    assert!(
        leaves.iter().all(|leaf| !leaf.contains("90210")),
        "{leaves:?}"
    );
    assert!(vetoes.is_empty(), "{vetoes:?}");
}

/// The digit-run veto is proven by the value, but a cue anywhere in the document or record, or
/// a non-Latin letter, still keeps it off: extra caution at no measured cost.
#[test]
fn a_cue_or_non_latin_letter_keeps_even_the_digit_run_veto_off() {
    for input in [
        "Telefon des Kunden:\nBitte den Wert unten verwenden.\n0593-9506-3395-7573",
        "itemCode=0593-9506-3395-7573\n\nTel<b>efon</b> siehe oben",
        "товар 0593-9506-3395-7573",
    ] {
        assert_protected("de-DE", input, "0593-9506-3395");
    }
}

/// A digit run of 12 to 15 digits can still be one phone number (E.164 allows 15), so a phone
/// inside it is never vetoed: `212-555-0187-44` keeps the phone protected with no benign row.
#[test]
fn a_run_short_enough_to_be_a_phone_keeps_the_phone() {
    assert_protected("en-US", "ref 212-555-0187-44 end", "555-0187");
    assert_protected("en-US", "ref 212-555-0187-4444-9 end", "555-0187");
}

