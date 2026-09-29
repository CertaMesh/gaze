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
                    ValidatorFailReason::BenignJoinedIdentifier
                        | ValidatorFailReason::BenignCurrencyAmount
                        | ValidatorFailReason::BenignDigitRunFragment
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
fn reference_tails_amounts_skus_and_loopbacks_leave_raw_with_one_row_per_veto() {
    use ValidatorFailReason::*;
    assert_raw(
        "en-US",
        "Lagerartikel SKU-DEMO-73821 und Charge BATCH-SAMPLE-92163; fertig.",
        &[
            ("postal.us", BenignJoinedIdentifier),
            ("postal.us", BenignJoinedIdentifier),
        ],
    );
    assert_raw(
        "de-DE",
        "invoice INVOICE-TEST-03687455, done",
        &[("phone.national.de", BenignJoinedIdentifier)],
    );
    assert_raw(
        "en-US",
        "level=info svc=orders grandTotal=EUR 22186,12 status=done",
        &[("postal.us", BenignCurrencyAmount)],
    );
    assert_raw(
        "de-DE",
        "level=info svc=orders itemCode=0593-9506-3395-7573 status=done",
        &[("phone.national.de", BenignDigitRunFragment)],
    );
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

#[test]
fn a_trailing_cue_a_field_name_or_a_city_anchor_keeps_the_value_protected() {
    // Review 10815 probes: each leaked raw before the fix.
    assert_protected("en-US", "ORDER-90210 (ZIP)", "90210");
    assert_protected("en-US", "ORDER-212-555-0187 (phone)", "555-0187");
    assert_protected("de-AT", "Room 1010 Wien", "1010");
    assert_protected("en-US", r#"{"postal_code":"ORDER-90210"}"#, "90210");
    // The same value as a structured field: the field name is the cue.
    for field in ["postal_code", "zipCode", "phone"] {
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            field.to_string(),
            gaze::Value::String("ORDER-90210".to_string()),
        );
        let (clean, vetoes) = clean_document("en-US", RawDocument::Structured(map));
        let CleanDocument::Structured(map) = clean else {
            panic!("expected structured");
        };
        let gaze::Value::String(value) = &map[field] else {
            panic!("expected string");
        };
        assert!(!value.contains("90210"), "{field}: {value}");
        assert!(vetoes.is_empty(), "{field}: {vetoes:?}");
    }
    // A field name without a cue leaves the order tail raw, with its row.
    let mut map = std::collections::BTreeMap::new();
    map.insert(
        "orderRef".to_string(),
        gaze::Value::String("ORDER-90210".to_string()),
    );
    let (clean, vetoes) = clean_document("en-US", RawDocument::Structured(map));
    let CleanDocument::Structured(map) = clean else {
        panic!("expected structured");
    };
    assert_eq!(
        map["orderRef"],
        gaze::Value::String("ORDER-90210".to_string())
    );
    assert_eq!(
        vetoes,
        vec![(
            "postal.us".to_string(),
            ValidatorFailReason::BenignJoinedIdentifier
        )]
    );
    // Review 10815 rev 2: a label line directly above the value.
    assert_protected("en-US", "ZIP:\nORDER-90210", "90210");
    assert_protected("de-DE", "Telefon:\nORDER-0301234567", "0301234567");
    // Review 10815 rev 2: address-labelled fields, in every spelling.
    for field in [
        "shippingAddress",
        "billing_address",
        "delivery-address",
        "address",
        "Anschrift",
        "adresse",
        "street",
        "city",
    ] {
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            field.to_string(),
            gaze::Value::String("ORDER-90210".to_string()),
        );
        let (clean, vetoes) = clean_document("en-US", RawDocument::Structured(map));
        let CleanDocument::Structured(map) = clean else {
            panic!("expected structured");
        };
        let gaze::Value::String(value) = &map[field] else {
            panic!("expected string");
        };
        assert!(!value.contains("90210"), "{field}: {value}");
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
    let (text, vetoes) = clean("de-DE", "Rechnung RECHNUNG-2026-80331 an 80331 München");
    assert!(
        text.starts_with("Rechnung RECHNUNG-2026-80331 an <"),
        "{text}"
    );
    assert!(!text.ends_with("80331 München"), "{text}");
    assert!(!vetoes.is_empty());
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

/// The bundled opt-in set is exactly the four uncued single-branch shape rules. Adding a
/// rule here needs a leak-direction review first.
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
    let pair = |id: &str, structures: &[&str]| {
        (
            id.to_string(),
            structures.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        declared,
        vec![
            pair(
                "phone.national.de",
                &["joined_identifier", "digit_run_fragment"]
            ),
            pair(
                "phone.national.us",
                &["joined_identifier", "digit_run_fragment"]
            ),
            pair("postal.de", &["joined_identifier", "currency_amount"]),
            pair("postal.us", &["joined_identifier", "currency_amount"]),
        ]
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

#[test]
fn the_loader_refuses_benign_lookalikes_outside_the_audited_bundled_rules() {
    let rule = |id: &str, pattern: &str, extra: &str| {
        format!(
            "id = \"{id}\"\nclass = \"custom:postal_code\"\nlocales = [\"global\"]\n\
             locale_basis = \"document\"\n[recognizers.match]\nkind = \"regex\"\n\
             pattern = '''{pattern}'''\n{extra}\
             [recognizers.context]\nbenign_lookalikes = [\"joined_identifier\"]\n"
        )
    };
    let plain = rule("postal.us", r"\b\d{5}\b", "");
    assert!(
        bundled_pack(&plain).is_ok(),
        "the audited bundled rule may opt in"
    );
    // Review 10815 rev 2: a one-capture, city-anchored custom rule. Refused from a file and
    // even when handed to the bundled parser.
    let beverly = rule(
        "custom.order_zip",
        r"ORDER-(\d{5})\s+Beverly",
        "capture_groups = [1]\n",
    );
    assert!(refused(load_pack(&beverly)));
    assert!(refused(bundled_pack(&beverly)));
    // An adopter file may not borrow an audited id either.
    assert!(refused(load_pack(&plain)));
    // Defence in depth on an audited id: a mandatory anchor or a non-regex matcher.
    assert!(refused(bundled_pack(&rule(
        "postal.us",
        r"\b\d{5}\b",
        "[recognizers.collision]\nfamily = \"probe-family\"\nvariant = \"a\"\nprecedence = 10\n\
         mandatory_anchor = \"iban\"\n"
    ))));
    assert!(refused(bundled_pack(
        "id = \"postal.us\"\nclass = \"custom:postal_code\"\nlocales = [\"global\"]\n\
         locale_basis = \"document\"\n[recognizers.match]\nkind = \"dictionary\"\n\
         terms = [\"90210\"]\n[recognizers.context]\nbenign_lookalikes = [\"currency_amount\"]\n"
    )));
}

#[test]
fn the_regex_builder_refuses_ids_outside_the_allowlist() {
    let detector = gaze_recognizers::RegexDetector::with_rulepack_fields(
        r"ORDER-(\d{5})\s+Beverly",
        gaze::PiiClass::custom("postal_code").expect("class"),
        "custom.order_zip",
        vec![gaze::LocaleTag::Global],
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
        detector.with_benign_lookalikes(vec![gaze_recognizers::BenignLookalike::JoinedIdentifier]),
        Err(gaze_recognizers::RecognizerError::UnsupportedBenignLookalike { .. })
    ));
}

/// Review 10815 rev 3, spoof 1: a rule that borrows the audited id `postal.us` with a different
/// pattern, handed to the bundled parser (or `RulepackSource::Embedded`), parses, but no grant is
/// minted for it, so building the pipeline fails closed.
#[test]
fn a_spoofed_bundled_rule_gets_no_grant() {
    let spoof = "id = \"postal.us\"\nclass = \"custom:postal_code\"\nlocales = [\"en-US\"]\n\
                 locale_basis = \"document\"\n[recognizers.match]\nkind = \"regex\"\n\
                 pattern = '''ORDER-(\\d{5})\\s+Beverly'''\ncapture_groups = [1]\n\
                 [recognizers.context]\nbenign_lookalikes = [\"joined_identifier\"]\n";
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

/// Review 10815 rev 3, spoof 2: a custom recognizer claims the audited identity and presents a
/// genuine grant borrowed from the real rule, but emits a span the audited pattern never
/// matches. Validator veto re-matches the audited pattern, so the span stays protected.
#[test]
fn a_borrowed_grant_vetoes_nothing_the_audited_pattern_does_not_emit() {
    use gaze_recognizers::{BenignLookalike, RegexDetector};
    let real = RegexDetector::with_rulepack_fields(
        r"\b\d{5}(-\d{4})?\b",
        gaze::PiiClass::custom("postal_code").expect("class"),
        "postal.us",
        vec![gaze::LocaleTag::EnUs],
        0.70,
        70,
        "counter",
        None,
        Vec::new(),
        None,
        None,
    )
    .expect("detector")
    .with_benign_lookalikes(vec![
        BenignLookalike::JoinedIdentifier,
        BenignLookalike::CurrencyAmount,
    ])
    .expect("the exact bundled postal.us rule is granted");
    struct Spoof(RegexDetector);
    impl gaze::Recognizer for Spoof {
        fn id(&self) -> &str {
            self.0.id()
        }
        fn supported_class(&self) -> &gaze::PiiClass {
            self.0.supported_class()
        }
        fn token_family(&self) -> &str {
            "counter"
        }
        fn locales(&self) -> &[gaze::LocaleTag] {
            self.0.locales()
        }
        fn locale_basis(&self) -> gaze::LocaleBasis {
            self.0.locale_basis()
        }
        fn detect(
            &self,
            input: &str,
            _: &gaze::DetectContext<'_>,
        ) -> Result<Vec<gaze::Candidate>, gaze::DetectError> {
            // Four digits after a currency code: the audited five-digit pattern never emits it.
            Ok(input
                .find("9021")
                .map(|start| {
                    gaze::Candidate::new(
                        start..start + 4,
                        self.0.supported_class().clone(),
                        "postal.us",
                        0.9,
                        90,
                        None,
                        "counter",
                        "postal.us",
                        ConflictTier::None,
                        Vec::new(),
                    )
                })
                .into_iter()
                .collect())
        }
        fn benign_lookalike_grant(&self) -> Option<&gaze_recognizers::BenignLookalikeGrant> {
            self.0.benign_lookalike_grant()
        }
    }
    let policy = policy("en-US");
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline_builder(&policy, &context, rulepacks(), &active, None)
        .expect("builder")
        .recognizer(Spoof(real))
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text("total EUR 9021 today".to_string()),
            active.as_slice(),
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .expect("clean");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(!text.contains("9021"), "{text}");
}

#[test]
fn a_multi_branch_pattern_cannot_declare_benign_lookalikes() {
    let detector = gaze_recognizers::RegexDetector::with_rulepack_fields(
        r"(?:plz (\d{4})|(\d{4}) [A-Z][a-z]+)",
        gaze::PiiClass::custom("postal_code").expect("class"),
        "postal.us",
        vec![gaze::LocaleTag::Global],
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
        detector.with_benign_lookalikes(vec![gaze_recognizers::BenignLookalike::CurrencyAmount]),
        Err(gaze_recognizers::RecognizerError::UnsupportedBenignLookalike { .. })
    ));
}

#[test]
fn a_recorded_failure_rule_cannot_declare_benign_lookalikes() {
    let detector = gaze_recognizers::RegexDetector::with_rulepack_fields(
        r"\b\d{10}\b",
        gaze::PiiClass::custom("phone").expect("class"),
        "phone.national.us",
        vec![gaze::LocaleTag::Global],
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
        detector.with_benign_lookalikes(vec![gaze_recognizers::BenignLookalike::CurrencyAmount]),
        Err(gaze_recognizers::RecognizerError::UnsupportedBenignLookalike { .. })
    ));
}

/// Review 10815 rev 3: a recognizer that borrows the real `postal.us` grant but gates
/// differently (global locales, so it runs on an en-GB document where the audited rule is
/// off) is not the audited rule; its candidate is never vetoed, even on an audited-pattern
/// match inside a benign structure.
#[test]
fn a_borrowed_grant_with_another_identity_vetoes_nothing() {
    use gaze_recognizers::{BenignLookalike, RegexDetector};
    let real = RegexDetector::with_rulepack_fields(
        r"\b\d{5}(-\d{4})?\b",
        gaze::PiiClass::custom("postal_code").expect("class"),
        "postal.us",
        vec![gaze::LocaleTag::EnUs],
        0.70,
        70,
        "counter",
        None,
        Vec::new(),
        None,
        None,
    )
    .expect("detector")
    .with_benign_lookalikes(vec![
        BenignLookalike::JoinedIdentifier,
        BenignLookalike::CurrencyAmount,
    ])
    .expect("the exact bundled postal.us rule is granted");
    struct Everywhere(RegexDetector);
    impl gaze::Recognizer for Everywhere {
        fn id(&self) -> &str {
            self.0.id()
        }
        fn supported_class(&self) -> &gaze::PiiClass {
            self.0.supported_class()
        }
        fn token_family(&self) -> &str {
            "counter"
        }
        fn locales(&self) -> &[gaze::LocaleTag] {
            &[gaze::LocaleTag::Global]
        }
        fn detect(
            &self,
            input: &str,
            _: &gaze::DetectContext<'_>,
        ) -> Result<Vec<gaze::Candidate>, gaze::DetectError> {
            Ok(input
                .find("90210")
                .map(|start| {
                    gaze::Candidate::new(
                        start..start + 5,
                        self.0.supported_class().clone(),
                        "postal.us",
                        0.9,
                        90,
                        None,
                        "counter",
                        "postal.us",
                        ConflictTier::None,
                        Vec::new(),
                    )
                })
                .into_iter()
                .collect())
        }
        fn benign_lookalike_grant(&self) -> Option<&gaze_recognizers::BenignLookalikeGrant> {
            self.0.benign_lookalike_grant()
        }
    }
    let policy = policy("en-GB");
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline_builder(&policy, &context, rulepacks(), &active, None)
        .expect("builder")
        .recognizer(Everywhere(real))
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text("Lagerartikel SKU-DEMO-90210 fertig".to_string()),
            active.as_slice(),
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .expect("clean");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(!text.contains("90210"), "{text}");
}
