//! IBAN and payment card numbers are tokenized even when mod-97 or Luhn fails (user ruling
//! 2026-09-27): a mistyped or masked account or card number is still someone's
//! financial data.
//!
//! In these two families, values can tokenize without a passing checksum:
//!
//! - `iban.structural`: a registry country code with that country's exact IBAN length, with or
//!   without a cue (unchanged shape, `on_fail = "record"`).
//! - `iban.cued`: a real ISO 3166-1 country code outside the IBAN registry, two digits and a BBAN
//!   (up to four letters then 6 to 26 digits compact, or three to eight digit-bearing groups)
//!   within 32 characters after the word `IBAN` on the same line. Registry countries stay with
//!   `iban.structural`, so a registry IBAN with a dropped digit is not covered.
//! - `card.cued`: a card layout (4-4-4-4-3 whole, 4-4-4-4, 4-6-5, 4-6-4, compact 16 to 19 from
//!   2-6, compact 14 to 15 from 3) within 32 characters after a card cue (`card`, German card
//!   compounds, card brands) on the same line. A span that starts with a Luhn-valid card is
//!   kept whole, so a cued 4-4-4-4-3 number is one token (without a cue `card.structural` still
//!   keeps a valid card's CVV outside).
//!
//! Both cued rules share one cue window: one `:`, `,` or `=` right after the cue, one nested JSON
//! key, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or
//! `=` ends it (review of #694, rounds 2 and 3).
//!
//! `card.structural` still vetoes a Luhn failure: without a cue a 16-digit run is as likely an
//! order, voucher or tracking number. Every failed value keeps its reason on the audit row and is
//! never swept to other copies.
//!
//! Every card and IBAN value here is synthetic; each test asserts the checksum verdict it relies
//! on, so a fixture cannot silently pass for the wrong reason.

// drift-ack: the core no-policy snapshot's Luhn-valid card now lists `card.cued+card.structural`
// as merged sources for the same span and class; the tokenized output is unchanged.

use std::sync::{Arc, Mutex, OnceLock};

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    RawDocument, RedactionEntry, RedactionLogError, RedactionLogger, RuleSpec, Rulepack,
    RulepackSource, Scope, Session,
};
use gaze_recognizers::{embedded, RegexDetector, ValidatorKind, ValidatorOnFail};
use gaze_types::{ValidatorFailReason, ValidatorOutcome};

fn financial_events(text: &str) -> Vec<gaze::RestoreEvent> {
    Session::new(Scope::Ephemeral)
        .expect("session")
        .restore_boundary_events(text)
        .into_iter()
        .filter(|event| event.class == custom("iban") || event.class == custom("credit_card"))
        .collect()
}

#[test]
fn restore_boundary_reports_checksum_failed_financial_values() {
    for (text, value, class) in [
        (
            "IBAN DE89 3704 0044 0532 0130 01",
            "DE89 3704 0044 0532 0130 01",
            "iban",
        ),
        (
            "IBAN US12 3456 7890 1234 5678",
            "US12 3456 7890 1234 5678",
            "iban",
        ),
        (
            "card number 4532 7812 3456 7890",
            "4532 7812 3456 7890",
            "credit_card",
        ),
        (
            "Card 0000 0000 0000 0000",
            "0000 0000 0000 0000",
            "credit_card",
        ),
        (
            "credit card number 4532 7812 3456 7890 123",
            "4532 7812 3456 7890 123",
            "credit_card",
        ),
    ] {
        let kind = if class == "iban" {
            ValidatorKind::IbanMod97
        } else {
            ValidatorKind::Luhn
        };
        fails(kind, value);
        assert_tokenized(&text[..text.find(value).unwrap()], value, "", class);
        let events = financial_events(text);
        assert_eq!(events.len(), 1, "{}: {events:?}", shape(text));
        assert_eq!(events[0].kind, gaze::RestoreEventKind::FreshPiiDetected);
        assert_eq!(events[0].class, custom(class));
        let start = text.find(value).unwrap();
        assert_eq!(events[0].location, start..start + value.len());
    }
}

#[test]
fn restore_boundary_cue_windows_match_forward_recognizers() {
    for (prefix, value, trailer, class) in [
        (
            "{\"card\": {\"number\": \"",
            "4111111111111112",
            "\"}}",
            "credit_card",
        ),
        ("Card number is: ", "4111 1111 1111 1112", "", "credit_card"),
        ("Amex ", "3782 822463 10006", " expired", "credit_card"),
        (
            "{\"bank\": {\"iban\": {\"value\": \"",
            "US12345678901234567",
            "\"}}}",
            "iban",
        ),
        ("IBAN is: ", "US12 3456 7890 1234 5678", "", "iban"),
    ] {
        assert_tokenized(prefix, value, trailer, class);
        let text = format!("{prefix}{value}{trailer}");
        let events = financial_events(&text);
        assert_eq!(events.len(), 1, "{}: {events:?}", shape(&text));
        assert_eq!(events[0].class, custom(class));
        assert_eq!(events[0].location, prefix.len()..prefix.len() + value.len());
    }
}

#[test]
fn restore_boundary_classifies_checksum_failed_manifest_bypasses() {
    for (text, value, class) in [
        (
            "IBAN DE89 3704 0044 0532 0130 01",
            "DE89 3704 0044 0532 0130 01",
            "iban",
        ),
        (
            "card number 4532 7812 3456 7890",
            "4532 7812 3456 7890",
            "credit_card",
        ),
    ] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        session.tokenize(&custom(class), value).expect("token");
        let events = session.restore_boundary_events(text);
        let financial: Vec<_> = events
            .iter()
            .filter(|event| event.class == custom(class))
            .collect();
        assert_eq!(financial.len(), 1, "{}: {events:?}", shape(text));
        assert_eq!(financial[0].kind, gaze::RestoreEventKind::ManifestBypass);
    }
}

#[test]
fn restore_with_events_reports_raw_checksum_failures() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let text = "IBAN DE89 3704 0044 0532 0130 01; card number 4532 7812 3456 7890";
    let (restored, events) = session
        .restore_strict_text_with_events(text)
        .expect("audit-only restore");
    assert_eq!(restored, text);
    let financial: Vec<_> = events
        .iter()
        .filter(|event| event.class == custom("iban") || event.class == custom("credit_card"))
        .collect();
    assert_eq!(financial.len(), 2, "{financial:?}");
    assert!(financial
        .iter()
        .all(|event| event.kind == gaze::RestoreEventKind::FreshPiiDetected));
}

#[test]
fn restore_boundary_obeys_financial_shape_and_cue_negatives() {
    for text in [
        "Order 4532 7812 3456 7890 shipped.",
        "Voucher code 4532 7812 3456 7890.",
        "Tracking: 4532781234567890",
        "card reference 4532 7812 3456 7890 5555 5",
        "credit card number 8818 1900 5934 6058 1769",
        "Card created at 1695827361000 ms.",
        "Card game id (optional): 4000 1234 5678 9011",
        "Your Visa interview reference is: 4000 1234 5678 9011",
        "IBAN DE00 TEST",
        "Ticket US29 1234 5678 9012 3456 7890 12 was closed.",
        "IBAN pending. Order US29 1234 5678 9012 3456 7890 12 shipped.",
        "IBAN US29CITI12345678901234_x9",
    ] {
        assert!(financial_events(text).is_empty(), "{}", shape(text));
        let cleaned = clean(text);
        assert!(!cleaned.contains(":Custom:iban_"), "{}", shape(text));
        assert!(!cleaned.contains(":Custom:credit_card_"), "{}", shape(text));
    }
}

#[test]
fn restore_boundary_financial_scan_matches_a4_forward_rule_scope() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../xtask/fixtures/negative_corpus/en_de_negative.jsonl");
    let corpus = std::fs::read_to_string(path).expect("committed A4 corpus");
    let mut documents = 0;
    let mut flagged = 0;
    for line in corpus.lines() {
        let row: serde_json::Value = serde_json::from_str(line).expect("A4 row");
        let text = row["text"].as_str().expect("A4 text");
        let events = financial_events(text);
        for event in &events {
            // The German A4 invalid-account placeholder has a registry country and exact DE
            // length. The forward rule records its failed checksum under the user's ruling.
            assert_eq!(row["category"], "invalid_identifiers", "A4 {}", row["id"]);
            assert_eq!(row["language"], "de", "A4 {}", row["id"]);
            assert_eq!(event.class, custom("iban"), "A4 {}", row["id"]);
            assert_eq!(
                &text[event.location.clone()],
                "DE00 TEST 0000 0000 0000 00",
                "A4 {}",
                row["id"]
            );
        }
        flagged += events.len();
        documents += 1;
    }
    assert_eq!(documents, 1024);
    assert_eq!(flagged, 64);
}

fn custom(class: &str) -> PiiClass {
    PiiClass::custom(class).expect("valid custom class")
}

#[derive(Clone, Default)]
struct MemoryLogger {
    entries: Arc<Mutex<Vec<RedactionEntry>>>,
}

impl RedactionLogger for MemoryLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.entries.lock().expect("lock").push(entry.clone());
        Ok(())
    }
}

const LOCALES: &[LocaleTag] = &[LocaleTag::EnUs, LocaleTag::DeDe, LocaleTag::Global];

fn pipeline(logger: MemoryLogger) -> Pipeline {
    let rulepacks: Vec<Rulepack> = ["core", "locale-de", "locale-en"]
        .into_iter()
        .map(|name| {
            Rulepack::load(RulepackSource::Embedded(
                embedded(name).unwrap_or_else(|| panic!("{name} rulepack")),
            ))
            .unwrap_or_else(|error| panic!("{name} loads: {error}"))
        })
        .collect();
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: custom("iban"),
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: custom("credit_card"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec![
        "core".to_string(),
        "locale-de".to_string(),
        "locale-en".to_string(),
    ];
    let context = Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    };
    let chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(LOCALES));
    gaze_assembly::build_pipeline(&policy, &context, &rulepacks, &chain, None)
        .expect("pipeline")
        .with_redaction_logger(logger)
}

fn shared() -> &'static Pipeline {
    static PIPELINE: OnceLock<Pipeline> = OnceLock::new();
    PIPELINE.get_or_init(|| pipeline(MemoryLogger::default()))
}

fn clean_with(pipeline: &Pipeline, session: &Session, text: &str) -> String {
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            session,
            RawDocument::Text(text.to_string()),
            LOCALES,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    }
}

fn clean(text: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    clean_with(shared(), &session, text)
}

/// A value's shape for failure messages: digits as 9, letters as A, tokens as `<token>`.
/// Fixture values are synthetic, but no value, card or IBAN is ever printed.
fn shape(text: &str) -> String {
    gaze::token_shape::pattern()
        .replace_all(text, "<token>")
        .chars()
        .map(|ch| match ch {
            '0'..='9' => '9',
            ch if ch.is_alphabetic() => 'A',
            ch => ch,
        })
        .collect()
}

fn fails(kind: ValidatorKind, value: &str) {
    assert!(
        matches!(kind.validate(value), ValidatorOutcome::Fail { .. }),
        "fixture {} must fail {kind:?}",
        shape(value)
    );
}

/// `prefix + value + trailer` cleans to `prefix + one <class> token + trailer`.
fn assert_tokenized(prefix: &str, value: &str, trailer: &str, class: &str) {
    let cleaned = clean(&format!("{prefix}{value}{trailer}"));
    let blanked = gaze::token_shape::pattern().replace_all(&cleaned, "\u{0}");
    assert!(
        blanked == format!("{prefix}\u{0}{trailer}"),
        "{} after {prefix:?}: {}",
        shape(value),
        shape(&cleaned)
    );
    assert!(
        cleaned.contains(&format!(":Custom:{class}_")),
        "{} must be a {class} token: {}",
        shape(value),
        shape(&cleaned)
    );
}

fn assert_raw(text: &str) {
    let cleaned = clean(text);
    assert!(cleaned == text, "must stay unchanged: {}", shape(&cleaned));
}

// ------------------------------------------------------------------ IBAN

#[test]
fn registry_shaped_iban_failing_mod97_is_tokenized_with_or_without_a_cue() {
    for (prefix, value, trailer) in [
        ("Bitte überweisen an ", "DE89 3704 0044 0532 0130 01", "."),
        ("Konto ", "DE00 TEST 0000 0000 0000 00", "."),
        ("IBAN: ", "AT61 1904 3002 3457 3202", " BIC: BKAUATWW"),
        ("account ", "NL91ABNA0417164301", " for rent"),
        ("{\"iban\": \"", "GB82WEST12345698765433", "\"}"),
    ] {
        fails(ValidatorKind::IbanMod97, value);
        assert_tokenized(prefix, value, trailer, "iban");
    }
}

#[test]
fn cued_iban_with_a_country_outside_the_registry_is_tokenized() {
    for (prefix, value, trailer) in [
        ("His IBAN is ", "US29 1234 5678 9012 3456 7890 12", "."),
        ("IBAN: ", "NZ12ABCD1234567890", ""),
        (
            "Her IBAN, starting with ",
            "CA29 1234 5678 9012 3456 78",
            ", was updated.",
        ),
        ("IBAN-Nr ", "NZ56 BNZ 0123 4567 8910 11", " bitte prüfen"),
        ("the IBAN ", "NZ71-028-020-1234567-00", " is new"),
        ("IBAN ", "US29CITI12345678901234", "BIC"),
    ] {
        assert_tokenized(prefix, value, trailer, "iban");
    }
}

#[test]
fn cued_iban_never_absorbs_a_following_word() {
    assert_tokenized(
        "IBAN ",
        "US29 1234 5678 9012 3456 7890 12",
        " BIC NWBKGB2L",
        "iban",
    );
    assert_tokenized("IBAN ", "US29 1234 5678 9012 3456 78", " and more", "iban");
}

#[test]
fn iban_shapes_without_a_cue_or_structure_stay_raw() {
    // No registry country, no cue: nothing says this is an account.
    assert_raw("Ticket US29 1234 5678 9012 3456 7890 12 was closed.");
    // Cue, but the value is far away on the next line.
    assert_raw("IBAN\nUS29 1234 5678 9012 3456 7890 12");
    // Cue, but too short to be an IBAN.
    assert_raw("IBAN status: AB12 34 is pending.");
    // Cue, but a digit before the value breaks the anchor.
    assert_raw("IBAN 2 of 3: US29 1234 5678 9012 3456 7890 12");
    // Cue, but the compact value runs into a longer identifier.
    assert_raw("IBAN US29CITI12345678901234_x9");
    // Review of #694, F2: a two-letter pair that is no country, and a cue whose clause ends.
    for text in [
        "IBAN field left blank, ticket XY12 3456 7890 1234 opened.",
        "IBAN: n/a, VAT ID US123456789012",
        "IBAN pending. Order US29 1234 5678 9012 3456 7890 12 shipped.",
    ] {
        let cleaned = clean(text);
        assert!(!cleaned.contains(":Custom:iban_"), "{}", shape(&cleaned));
    }
}

/// ISO 3166-1 alpha-2, all 249 codes (checked against `pycountry` when this list was written).
const ISO_3166_ALPHA_2: &[&str] = &[
    "AD", "AE", "AF", "AG", "AI", "AL", "AM", "AO", "AQ", "AR", "AS", "AT", "AU", "AW", "AX", "AZ",
    "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BL", "BM", "BN", "BO", "BQ", "BR", "BS",
    "BT", "BV", "BW", "BY", "BZ", "CA", "CC", "CD", "CF", "CG", "CH", "CI", "CK", "CL", "CM", "CN",
    "CO", "CR", "CU", "CV", "CW", "CX", "CY", "CZ", "DE", "DJ", "DK", "DM", "DO", "DZ", "EC", "EE",
    "EG", "EH", "ER", "ES", "ET", "FI", "FJ", "FK", "FM", "FO", "FR", "GA", "GB", "GD", "GE", "GF",
    "GG", "GH", "GI", "GL", "GM", "GN", "GP", "GQ", "GR", "GS", "GT", "GU", "GW", "GY", "HK", "HM",
    "HN", "HR", "HT", "HU", "ID", "IE", "IL", "IM", "IN", "IO", "IQ", "IR", "IS", "IT", "JE", "JM",
    "JO", "JP", "KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ", "LA", "LB", "LC",
    "LI", "LK", "LR", "LS", "LT", "LU", "LV", "LY", "MA", "MC", "MD", "ME", "MF", "MG", "MH", "MK",
    "ML", "MM", "MN", "MO", "MP", "MQ", "MR", "MS", "MT", "MU", "MV", "MW", "MX", "MY", "MZ", "NA",
    "NC", "NE", "NF", "NG", "NI", "NL", "NO", "NP", "NR", "NU", "NZ", "OM", "PA", "PE", "PF", "PG",
    "PH", "PK", "PL", "PM", "PN", "PR", "PS", "PT", "PW", "PY", "QA", "RE", "RO", "RS", "RU", "RW",
    "SA", "SB", "SC", "SD", "SE", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN", "SO", "SR", "SS",
    "ST", "SV", "SX", "SY", "SZ", "TC", "TD", "TF", "TG", "TH", "TJ", "TK", "TL", "TM", "TN", "TO",
    "TR", "TT", "TV", "TW", "TZ", "UA", "UG", "UM", "US", "UY", "UZ", "VA", "VC", "VE", "VG", "VI",
    "VN", "VU", "WF", "WS", "YE", "YT", "ZA", "ZM", "ZW",
];

#[test]
fn iban_cued_country_class_is_iso_3166_minus_the_registry() {
    assert_eq!(ISO_3166_ALPHA_2.len(), 249);
    let core = Rulepack::load(RulepackSource::Embedded(embedded("core").expect("core")))
        .expect("core loads");
    let spec = core
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "iban.cued")
        .expect("iban.cued");
    let gaze::RawMatch::Regex {
        pattern: Some(pattern),
        ..
    } = &spec.matcher
    else {
        panic!("iban.cued is a regex recognizer");
    };
    let regex = regex::Regex::new(pattern).expect("pattern");
    for first in 'A'..='Z' {
        for second in 'A'..='Z' {
            let country = format!("{first}{second}");
            let text = format!("IBAN {country}12 3456 7890 1234 5678 90");
            let matched = regex.is_match(&text);
            let registry = gaze_types::iban_registry_length(&country).is_some();
            let iso = ISO_3166_ALPHA_2.contains(&country.as_str());
            // `UK` is how people mistype a GB IBAN (ISO-reserved); `EU` is not taken.
            let mistyped_gb = country == "UK";
            assert_eq!(
                matched,
                (iso || mistyped_gb) && !registry,
                "{country}: iso={iso} registry={registry}"
            );
        }
    }
}

/// The cue window is one rule written twice (`iban.cued` and `card.cued`); a fix to one copy
/// must reach the other (review of #694, cleanup).
#[test]
fn the_cue_window_is_byte_identical_in_both_cued_rules() {
    let core = Rulepack::load(RulepackSource::Embedded(embedded("core").expect("core")))
        .expect("core loads");
    let window = |id: &str| -> String {
        let spec = core
            .recognizers
            .iter()
            .find(|recognizer| recognizer.id == id)
            .unwrap_or_else(|| panic!("{id}"));
        let gaze::RawMatch::Regex {
            pattern: Some(pattern),
            ..
        } = &spec.matcher
        else {
            panic!("{id} is a regex recognizer");
        };
        let start = pattern.find("# cue-window:start").expect("start marker");
        let end = pattern.find("# cue-window:end").expect("end marker");
        pattern[start..end].to_string()
    };
    assert_eq!(window("iban.cued"), window("card.cued"));
}

/// Review of #694, round 3: the window reaches a value behind one nested JSON key, or behind one
/// `label:` after a copula or a parenthetical, as round 1 did; other clauses stay closed.
#[test]
fn the_cue_window_reaches_nested_json_and_a_copula_label() {
    for (prefix, value, trailer, class) in [
        (
            "{\"credit_card\": {\"number\": \"",
            "4111 1111 1111 1112",
            "\"}}",
            "credit_card",
        ),
        (
            "{\"card\": {\"number\": \"",
            "4111111111111112",
            "\", \"cvc\": \"123\"}}",
            "credit_card",
        ),
        (
            "{\"bank\": {\"iban\": {\"value\": \"",
            "US12345678901234567",
            "\"}}}",
            "iban",
        ),
        ("Card number is: ", "4111 1111 1111 1112", "", "credit_card"),
        (
            "Card number (see below): ",
            "4111 1111 1111 1112",
            "",
            "credit_card",
        ),
        ("IBAN is: ", "US12 3456 7890 1234 5678", "", "iban"),
        (
            "IBAN (USD account): ",
            "US12 3456 7890 1234 5678",
            "",
            "iban",
        ),
        ("IBAN, bitte: ", "US12 3456 7890 1234 5678", "", "iban"),
        ("Karte, Nummer ", "4111 1111 1111 1112", "", "credit_card"),
        ("IBAN ", "UK12 3456 7890 1234 5678", "", "iban"),
    ] {
        assert_tokenized(prefix, value, trailer, class);
    }
}

/// Review of #694, round 3: a 4-4-4-4-3 number after a card cue is one token even when its first
/// 16 digits pass Luhn: it may be a mistyped 19-digit card, and a CVV is sensitive too.
#[test]
fn a_cued_four_four_four_four_three_number_is_one_token() {
    assert_tokenized(
        "Maestro card ",
        "6759 6498 2643 8453 012",
        "",
        "credit_card",
    );
    assert_tokenized(
        "credit card number ",
        "4111 1111 1111 1111 123",
        ".",
        "credit_card",
    );
}

// ------------------------------------------------------------------ cards

#[test]
fn card_after_a_cue_failing_luhn_is_tokenized() {
    for (prefix, value, trailer) in [
        ("My credit card number is ", "4532 7812 3456 7890", "."),
        ("Kreditkartennummer: ", "4916-1234-5678-9012", " (Visa)"),
        (
            "Die Karte mit der Nummer ",
            "4556 7375 8689 9852",
            " ist gesperrt.",
        ),
        ("{\"card_number\": \"", "4556 7375 8689 9852", "\"}"),
        ("cardNumber=", "4024007136512387", " status=declined"),
        ("Amex ", "3782 822463 10006", " expired"),
        (
            "paid with Mastercard ",
            "5500\u{00A0}1234\u{00A0}5678\u{00A0}9012",
            "",
        ),
        // A Luhn-failing 4-4-4-4-3 is one token, trailing group included: never partial.
        ("credit card number ", "4532 7812 3456 7890 123", " (CVV)"),
        ("Sim card ICCID ", "8949 0200 0012 3456 789", ""),
    ] {
        fails(ValidatorKind::Luhn, value);
        assert_tokenized(prefix, value, trailer, "credit_card");
    }
}

/// A cue proves nothing when the digits say otherwise. `run` must hold no Luhn-valid card window
/// at all, or `card.structural` would tokenize it for its own reasons and the fixture would pass
/// by accident.
fn assert_raw_card_run(prefix: &str, run: &str, trailer: &str) {
    let scan = gaze_types::payment_card::scan_card_run(run, 0..run.len(), None);
    assert!(
        scan.cards.is_empty(),
        "{} holds a Luhn-valid window",
        shape(run)
    );
    assert_raw(&format!("{prefix}{run}{trailer}"));
}

#[test]
fn luhn_failing_digits_without_a_card_shape_or_cue_stay_raw() {
    // No cue: order, voucher and tracking numbers keep the Luhn veto.
    assert_raw_card_run("Order ", "4532 7812 3456 7890", " shipped.");
    assert_raw_card_run("Voucher code ", "4532 7812 3456 7890", ".");
    assert_raw_card_run("Tracking: ", "4532781234567890", "");
    // Cue, but the run continues past a card layout: a longer reference, not a card.
    assert_raw_card_run("card reference ", "4532 7812 3456 7890 5555 5", "");
    // Five groups of four after a cue: the fifth group is no CVV, so no card-shaped prefix.
    assert_raw_card_run("credit card number ", "8818 1900 5934 6058 1769", ".");
    // Cue, but grouped like an amount or a phone number, not a card.
    assert_raw_card_run("card fee ", "1 234 567 890 123", " EUR");
    // Cue words inside other words are not cues.
    assert_raw_card_run("Scorecard ", "4532 7812 3456 7890", "");
    assert_raw_card_run("Eintrittskarte ", "4532 7812 3456 7890", "");
    // Review of #694, F1: epoch-millisecond timestamps and compact phone numbers near a card
    // cue are not cards (compact cards are 16-19 digits from 2-6, or 14-15 from 3).
    assert_raw_card_run("{\"object\": \"card\", \"created\": ", "1695827361000", "}");
    assert_raw_card_run("{\"card\": \"active\", \"ts\": ", "1695827361000", "}");
    assert_raw_card_run("Card created at ", "1695827361000", " ms.");
    assert_raw_card_run("Kartennummer? Nein, ruf mich an: ", "4915123456789", "");
    assert_raw_card_run("credit card number ", "4915123456789", "");
    assert_raw_card_run("credit card number ", "491761234567890", "");
    // F2: the cue does not reach into another clause, field or word sense.
    assert_raw_card_run("card game; game id ", "4000 1234 5678 9011", "");
    assert_raw_card_run("Die Karte zeigt Planquadrat ", "4000 1234 5678 9011", "");
    assert_raw_card_run("Karte: Menü ", "1234 5678 9012 3456", "");
    assert_raw_card_run("Visa application number ", "1234567890123456", "");
    assert_raw_card_run("Your Visa interview reference: ", "4000 1234 5678 9011", "");
    // Review of #694, round 4: the copula or parenthetical must directly follow the cue, and a
    // parenthetical holds no digits.
    assert_raw_card_run("The card game score is: ", "4000 1234 5678 9011", "");
    assert_raw_card_run(
        "Your Visa interview reference is: ",
        "4000 1234 5678 9011",
        "",
    );
    assert_raw_card_run("Card game id (optional): ", "4000 1234 5678 9011", "");
    assert_raw_card_run("card (qty 4000): ", "1234 5678 9012 3456", "");
    // Cue, but the number is past the 32-character window.
    assert_raw_card_run(
        "card on file, see the attached billing statement ",
        "4532 7812 3456 7890",
        "",
    );
}

// ------------------------------------------------------------------ audit + sweep

#[test]
fn a_kept_checksum_failure_is_written_on_the_winner_audit_row() {
    let logger = MemoryLogger::default();
    let pipeline = pipeline(logger.clone());
    let session = Session::new(Scope::Ephemeral).expect("session");
    clean_with(
        &pipeline,
        &session,
        "IBAN DE89 3704 0044 0532 0130 01 and card number 4532 7812 3456 7890; \
         valid IBAN DE89 3704 0044 0532 0130 00 and card 4111 1111 1111 1111.",
    );
    let entries = logger.entries.lock().expect("lock").clone();
    let winners = |class: &str| -> Vec<Option<ValidatorFailReason>> {
        entries
            .iter()
            .filter(|entry| !entry.conflict_loser && entry.class == custom(class))
            .map(|entry| entry.validator_fail_reason)
            .collect()
    };
    let mut ibans = winners("iban");
    ibans.sort_by_key(|reason| reason.is_some());
    assert_eq!(
        ibans,
        vec![None, Some(ValidatorFailReason::IbanMod97Failed)]
    );
    let mut cards = winners("credit_card");
    cards.sort_by_key(|reason| reason.is_some());
    assert_eq!(cards, vec![None, Some(ValidatorFailReason::LuhnFailed)]);
    // `card.structural` still vetoes the Luhn-failing runs (the card, and the digit run inside the
    // invalid IBAN) and logs them as losers; `card.cued` and `iban.*` keep theirs. No IBAN
    // recognizer vetoes anything. International-prefix phone candidates inside the IBAN
    // grouping are also vetoed, with their own typed parser reason.
    let vetoed: Vec<_> = entries
        .iter()
        .filter(|entry| entry.decided_by == gaze::ConflictTier::ValidatorVeto)
        .map(|entry| (entry.recognizer_id.clone(), entry.validator_fail_reason))
        .collect();
    assert!(!vetoed.is_empty(), "{entries:?}");
    assert!(
        vetoed.iter().all(|row| matches!(
            (row.0.as_deref(), row.1),
            (
                Some("card.structural"),
                Some(ValidatorFailReason::LuhnFailed)
            ) | (
                Some("phone.e164.spaced"),
                Some(ValidatorFailReason::PhoneE164Rejected)
            )
        )),
        "{entries:?}"
    );
}

#[test]
fn a_checksum_failed_value_is_never_swept_to_an_uncued_copy() {
    // The cued card fails Luhn, so its value is `Learned` evidence. The later copy has no cue and
    // `card.structural` vetoes it; only the repeat-value sweep could reach it, and must not.
    let value = "4532 7812 3456 7890";
    fails(ValidatorKind::Luhn, value);
    let cleaned = clean(&format!("card number {value}. Order {value} shipped."));
    assert_eq!(
        cleaned.matches(":Custom:credit_card_").count(),
        1,
        "{}",
        shape(&cleaned)
    );
    assert!(
        cleaned.ends_with(&format!("Order {value} shipped.")),
        "{}",
        shape(&cleaned)
    );

    // The cued IBAN carries a canonical form from its normalizer although mod-97 never passed; a
    // canonical form alone must not make it `Validated` evidence the sweep spreads.
    let iban = "US29 1234 5678 9012 3456 7890 12";
    fails(ValidatorKind::IbanMod97, iban);
    let cleaned = clean(&format!("IBAN {iban}. Ticket {iban} closed."));
    assert_eq!(
        cleaned.matches(":Custom:iban_").count(),
        1,
        "{}",
        shape(&cleaned)
    );
    assert!(
        cleaned.ends_with(&format!("Ticket {iban} closed.")),
        "{}",
        shape(&cleaned)
    );
}

// ------------------------------------------------------------------ construction guards

fn detector(pattern: &str, validator: Option<ValidatorKind>) -> RegexDetector {
    RegexDetector::with_rulepack_fields(
        pattern,
        custom("probe"),
        "probe",
        vec![LocaleTag::Global],
        0.7,
        80,
        "counter",
        None,
        Vec::new(),
        validator,
        None,
    )
    .expect("detector")
}

#[test]
fn record_on_fail_is_refused_for_broad_card_runs_and_unlisted_validators() {
    // A card-run recognizer offers every digit run in the text.
    let card_runs = detector(
        gaze_types::payment_card::CARD_RUN_PATTERN,
        Some(ValidatorKind::Luhn),
    );
    assert!(card_runs
        .with_validator_on_fail(ValidatorOnFail::Record)
        .is_err());
    // Validators outside the allowlist, or none.
    for kind in [ValidatorKind::CnpjMod11, ValidatorKind::EmailRfc] {
        assert!(
            detector(r"\d+", Some(kind))
                .with_validator_on_fail(ValidatorOnFail::Record)
                .is_err(),
            "{kind:?}"
        );
    }
    assert!(detector(r"\d+", None)
        .with_validator_on_fail(ValidatorOnFail::Record)
        .is_err());
    // A pattern-anchored candidate with an eligible validator may record.
    for kind in [
        ValidatorKind::IbanMod97,
        ValidatorKind::Luhn,
        ValidatorKind::DeSteuerIdMod1110,
        ValidatorKind::BsnMod11,
        ValidatorKind::CpfMod11,
        ValidatorKind::UkNhsMod11,
    ] {
        assert!(detector(r"card (\d{16})", Some(kind))
            .with_validator_on_fail(ValidatorOnFail::Record)
            .is_ok());
    }
}
