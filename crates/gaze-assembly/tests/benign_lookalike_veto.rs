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
use gaze_recognizers::ValidatorFailReason;
use gaze_assembly::build_pipeline_builder;

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

/// Clean `input` under one document locale; returns the clean text with token session
/// prefixes removed and the typed reasons of every benign or loopback veto row.
fn clean(locale: &str, input: &str) -> (String, Vec<(String, ValidatorFailReason)>) {
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
    let text = regex::Regex::new(r"<[0-9a-f]{8}:")
        .expect("regex")
        .replace_all(&text, "<")
        .into_owned();
    let vetoes = logger
        .0
        .lock()
        .expect("entries")
        .iter()
        .filter(|entry| entry.decided_by == ConflictTier::ValidatorVeto)
        .filter_map(|entry| {
            let id = entry.recognizer_id.clone().unwrap_or_else(|| entry.source.clone());
            Some((id, entry.validator_fail_reason?))
        })
        .filter(|(_, reason)| {
            matches!(
                reason,
                ValidatorFailReason::BenignJoinedIdentifier
                    | ValidatorFailReason::BenignCurrencyAmount
                    | ValidatorFailReason::BenignDigitRunFragment
                    | ValidatorFailReason::BenignLabelNumber
                    | ValidatorFailReason::Ipv4LoopbackRange
                    | ValidatorFailReason::Ipv6LoopbackRange
            )
        })
        .collect();
    (text, vetoes)
}

fn assert_raw(locale: &str, input: &str, expected: &[(&str, ValidatorFailReason)]) {
    let (text, vetoes) = clean(locale, input);
    assert_eq!(text, input, "{locale}: benign text must leave raw");
    let mut vetoes = vetoes;
    vetoes.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    vetoes.dedup();
    let mut expected: Vec<(String, ValidatorFailReason)> =
        expected.iter().map(|(source, reason)| (source.to_string(), *reason)).collect();
    expected.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    assert_eq!(vetoes, expected, "{locale}: {input}");
}

#[test]
fn reference_tails_amounts_skus_rooms_and_loopbacks_leave_raw_with_typed_rows() {
    use ValidatorFailReason::*;
    assert_raw(
        "en-US",
        "Lagerartikel SKU-DEMO-73821 und Charge BATCH-SAMPLE-92163; fertig.",
        &[("postal.us", BenignJoinedIdentifier)],
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
        "de-AT",
        "The handover lists adjacent values: Room 4833 Room 4844.",
        &[("postal.at_ch", BenignLabelNumber)],
    );
    assert_raw(
        "en-US",
        "values: 127.0.0.8 ::ffff:127.0.0.5 ::1 done",
        &[
            ("ip.v4", Ipv4LoopbackRange),
            ("ip.v6", Ipv6LoopbackRange),
        ],
    );
}

fn assert_protected(locale: &str, input: &str, value: &str) {
    let (text, vetoes) = clean(locale, input);
    assert!(
        !text.contains(value),
        "{locale}: {value} must not leave raw in {input:?}, got {text:?}"
    );
    assert!(vetoes.is_empty(), "{locale}: no benign veto expected, got {vetoes:?}");
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
    assert_protected("en-US", "peer fe80::21a:2bff:fe3c:4d5e up", "fe80::21a:2bff:fe3c:4d5e");
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
    assert_value_protected("de-DE", "Tel: 030 1234567, Bestellung ORDER-2026-145684", "1234567");
    assert_value_protected("de-DE", "Telefon 0301234567 itemCode=0593-9506-3395-7573", "0301234567");
    // A phone cue in front of a digit run keeps even the fragment protected.
    assert_value_protected("de-DE", "Phone: 0593-9506-3395-7573", "9506-3395");
    // Financial identifiers beside amounts: checksum rules never opt in.
    assert_value_protected("en-US", "IBAN DE89 3704 0044 0532 0130 00 EUR 500,00", "3704 0044 0532");
    assert_value_protected("en-US", "Card 4539 1488 0343 6467 USD 12.00", "4539 1488 0343 6467");
    assert_value_protected("en-US", "paid $12.00 by card 4539148803436467", "4539148803436467");
    // Cued and plain postcodes beside references, amounts and rooms.
    assert_value_protected("en-US", "ZIP 90210, SKU-DEMO-73821", "90210");
    assert_value_protected("en-US", "SKU-DEMO-73821 ships to Beverly Hills, CA 90210", "90210");
    assert_value_protected("de-AT", "PLZ: 1010 Wien, Room 4833", "1010");
    assert_value_protected("en-US", "EUR 12,00 to 10115 Berlin", "10115");
    // The same five digits: vetoed as an order tail, still protected as a postcode.
    let (text, vetoes) = clean("de-DE", "Rechnung RECHNUNG-2026-80331 an 80331 München");
    assert!(text.starts_with("Rechnung RECHNUNG-2026-80331 an <"), "{text}");
    assert!(!text.ends_with("80331 München"), "{text}");
    assert!(!vetoes.is_empty());
    // An email and a routable IP next to loopback and a reference stay protected.
    assert_value_protected("en-US", "SKU-DEMO-73821 owner anna@example.org", "anna@example.org");
    assert_value_protected("en-US", "proxy 127.0.0.1 forwarded client 84.12.3.4", "84.12.3.4");
    assert_value_protected("en-US", "::1 and 2a00:1450:4001:82a::200e", "2a00:1450:4001:82a::200e");
}
