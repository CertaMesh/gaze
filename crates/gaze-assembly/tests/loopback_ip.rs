//! Loopback IP addresses stay raw; everything else keeps its protection.
//!
//! The bundled IP validators reject 127.0.0.0/8, `::1`, and IPv4-mapped or IPv4-compatible
//! loopback with a typed validator-veto row. The neighbours of that range stay tokenized, and
//! the shapes an earlier benign-lookalike draft skipped (reference tails, long digit runs,
//! two phones in one run) are tokenized like any other postcode or phone.
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

fn clean(locale: &str, input: &str) -> (String, Vec<(String, ValidatorFailReason)>) {
    let text = format!(
        "schema_version = \"0.1.0\"\n\n[session]\nscope = \"persistent\"\nttl_secs = 86400\n\n\
         [policy.rulepacks]\nbundled = [\"core\", \"locale-de\", \"locale-en\"]\n\n\
         [locale]\nactive = [\"{locale}\"]\n\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n"
    );
    let path = std::env::temp_dir().join(format!(
        "gaze-loopback-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, text).expect("write policy");
    let policy = Policy::load(&path).expect("policy");
    let _ = std::fs::remove_file(&path);
    let packs = ["core", "locale-de", "locale-en"].map(|name| {
        Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded(name).expect("embedded rulepack"),
        ))
        .expect("rulepack")
    });
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let logger = MemoryLogger::default();
    let pipeline = build_pipeline_builder(&policy, &context, &packs, &active, None)
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
    let rows = logger
        .0
        .lock()
        .expect("entries")
        .iter()
        .filter(|entry| {
            matches!(
                entry.validator_fail_reason,
                Some(
                    ValidatorFailReason::Ipv4LoopbackRange | ValidatorFailReason::Ipv6LoopbackRange
                )
            )
        })
        .map(|entry| {
            assert_eq!(entry.decided_by, ConflictTier::ValidatorVeto);
            assert!(entry.conflict_loser);
            (
                entry.recognizer_id.clone().expect("recognizer id"),
                entry.validator_fail_reason.expect("reason"),
            )
        })
        .collect();
    (text, rows)
}

#[test]
fn loopback_addresses_stay_raw_with_one_typed_row_each() {
    use ValidatorFailReason::*;
    let input = "values: 127.0.0.8 ::ffff:127.0.0.5 ::127.0.0.6 ::1 done";
    let (text, mut rows) = clean("en-US", input);
    assert_eq!(text, input);
    rows.sort_by_key(|row| format!("{row:?}"));
    let mut expected = vec![
        ("ip.v4".to_string(), Ipv4LoopbackRange),
        ("ip.v4".to_string(), Ipv4LoopbackRange),
        ("ip.v4".to_string(), Ipv4LoopbackRange),
        ("ip.v6".to_string(), Ipv6LoopbackRange),
        ("ip.v6".to_string(), Ipv6LoopbackRange),
        ("ip.v6".to_string(), Ipv6LoopbackRange),
    ];
    expected.sort_by_key(|row| format!("{row:?}"));
    assert_eq!(rows, expected);
}

fn assert_protected(locale: &str, input: &str, value: &str) {
    let (text, rows) = clean(locale, input);
    assert!(!text.contains(value), "{value} left raw in {text:?}");
    assert!(rows.is_empty(), "no loopback row expected: {rows:?}");
}

#[test]
fn the_neighbours_of_loopback_stay_protected() {
    for address in [
        "126.255.255.255",
        "128.0.0.1",
        "10.0.0.7",
        "::2",
        "::ffff:10.0.0.7",
        "::10.0.0.7",
        "::ffff:84.12.3.4",
        "fe80::21a:2bff:fe3c:4d5e",
    ] {
        assert_protected("en-US", &format!("host {address} up"), address);
    }
}

/// Shapes a removed benign-lookalike draft left raw: they are tokenized.
#[test]
fn reference_tails_long_runs_and_phone_pairs_are_tokenized() {
    for (locale, input, value) in [
        ("en-US", "call 212-555-0187-212-555-0188 now", "555-0187"),
        ("en-US", "call 212-555-0187-212-555-0188 now", "555-0188"),
        ("en-US", "Phne: 212-555-0187-4444-99", "555-0187"),
        ("en-US", "ZIP for delivery:\n\nORDER-90210", "90210"),
        ("en-US", "Kod pocztowy: ORDER-90210", "90210"),
        (
            "de-DE",
            "itemCode=0593-9506-3395-7573 status=done",
            "0593-9506-3395",
        ),
    ] {
        assert_protected(locale, input, value);
    }
}

#[test]
fn a_rulepack_that_still_declares_benign_lookalikes_fails_with_a_typed_error() {
    let pack = Rulepack::parse(
        "schema_version = \"0.1.0\"\nrulepack_id = \"probe\"\nrulepack_version = \"0.1.0\"\n\
         default_locales = [\"global\"]\n\n[[recognizers]]\nid = \"probe.zip\"\n\
         class = \"custom:postal_code\"\nlocales = [\"global\"]\n[recognizers.match]\n\
         kind = \"regex\"\npattern = '''\\b\\d{5}\\b'''\n[recognizers.context]\n\
         benign_lookalikes = [\"digit_run_fragment\"]\n",
    );
    assert!(
        matches!(
            pack,
            Err(gaze::RulepackError::RemovedKey {
                key: "benign_lookalikes",
                ..
            })
        ),
        "{pack:?}"
    );
}
