//! Literal `<` / `>` beside a token must round-trip through strict restore.
//!
//! Strict restore used to reject any token whose neighbouring byte was `<` or `>` as a
//! "nested wrapper", so ordinary text such as `<alice@example.invalid>` or
//! `4111111111111111><AB12CD34>` cleaned fine but could never be restored. Tokens are
//! recognised by their exact grammar, so neighbouring angle brackets are ordinary text.
//!
//! The seeded corpus runs the full bundled `core` pack. `GAZE_ANGLE_PROBE_DOCS` raises the
//! document count for the before/after measurement in the PR (20,000 there).
use gaze::{
    CleanDocument, Context, DictionaryBundle, LocaleChain, Pipeline, Policy, RawDocument, Rulepack,
    RulepackSource, SafetyNetPolicy, Scope, Session,
};
use gaze_assembly::build_pipeline;

const LOCALES: &str =
    r#"["de-DE", "de-AT", "de-CH", "en-US", "en-GB", "en-AU", "en-CA", "en-IE", "global"]"#;

fn core_pipeline() -> (Pipeline, LocaleChain) {
    let text = format!(
        "schema_version = \"0.1.0\"\n\n[session]\nscope = \"persistent\"\nttl_secs = 86400\n\n\
         [policy.rulepacks]\nbundled = [\"core\", \"locale-de\", \"locale-en\"]\n\n\
         [locale]\nactive = {LOCALES}\n\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n"
    );
    let path = std::env::temp_dir().join(format!(
        "gaze-angle-restore-{}-{:?}.toml",
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
    let pipeline = build_pipeline(&policy, &context, &packs, &active, None).expect("pipeline");
    (pipeline, active)
}

fn clean(pipeline: &Pipeline, active: &LocaleChain, session: &Session, input: &str) -> String {
    let (clean, _, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            session,
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

/// Clean, then strict-restore. Returns the cleaned text on a round-trip failure.
fn round_trip(pipeline: &Pipeline, active: &LocaleChain, input: &str) -> Result<String, String> {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let cleaned = clean(pipeline, active, &session, input);
    match pipeline.restore_strict_text(&session, &cleaned) {
        Ok(restored) if restored == input => Ok(cleaned),
        Ok(restored) => Err(format!("{cleaned}\n  restored: {restored}")),
        Err(error) => Err(format!("{cleaned}\n  error: {error}")),
    }
}

#[test]
fn angle_brackets_beside_tokens_restore_exactly() {
    let (pipeline, active) = core_pipeline();
    for input in [
        // A review example: `>` after one token, `<`/`>` around another.
        "Driver license - 4111111111111111><AB12CD34>",
        "Contact: Alice <alice@example.invalid>",
        "<alice@example.invalid>",
        "<<alice@example.invalid>>",
        "Card <4111111111111111> and IBAN <<DE89370400440532013000>>",
        "a<b> alice@example.invalid> x <4111 1111 1111 1111",
    ] {
        let cleaned = round_trip(&pipeline, &active, input)
            .unwrap_or_else(|failure| panic!("round trip failed for {input:?}: {failure}"));
        assert!(
            !cleaned.contains("alice@example.invalid") && !cleaned.contains("4111111111111111"),
            "value stayed raw: {cleaned}"
        );
    }
}

const FRAGMENTS: &[&str] = &[
    "<",
    ">",
    "<<",
    ">>",
    "<x>",
    "</a>",
    "<br/>",
    "Vec<T>",
    "->",
    "=>",
    "alice@example.invalid",
    "bob.smith@example.org",
    "4111111111111111",
    "4111 1111 1111 1111",
    "DE89370400440532013000",
    "GB82WEST12345698765432",
    "123-45-6789",
    "10115",
    "10115 Berlin",
    "ZZ9 9ZZ",
    "03.04.1988",
    "1988-04-03",
    "+49 171 3920055",
    "(555) 555-0142",
    "192.168.10.20",
    "AB12CD34",
    "AB123456",
    "Driver license - ",
    "Tax number: ",
    "Ausweisnummer: ",
    "SSN ",
    "IBAN ",
    "Email: ",
    "Tel. ",
    "geboren am ",
    "Kunde ",
    "Hauptstraße 5",
    " ",
    " ",
    "  ",
    "\n",
    "\t",
    ".",
    ",",
    ":",
    ";",
    "-",
    "/",
    "(",
    ")",
    "\"",
    "'",
    "é",
    "ü",
    "ß",
    "中文",
    "😀",
    "e\u{0301}",
    "\u{200B}",
    "\u{2060}",
    "\u{FEFF}",
    "\u{00A0}",
    "\u{202F}",
    "und",
    "the",
    "Berlin",
    "order",
    "7",
    "42",
];

/// xorshift64*: deterministic without a new dev-dependency, so a failing seed replays.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn document(rng: &mut Rng) -> String {
    let len = 2 + rng.below(8);
    (0..len)
        .map(|_| FRAGMENTS[rng.below(FRAGMENTS.len())])
        .collect()
}

#[test]
fn seeded_fragment_corpus_round_trips_under_core() {
    let docs = std::env::var("GAZE_ANGLE_PROBE_DOCS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2_000);
    let (pipeline, active) = core_pipeline();
    let mut rng = Rng(0x4009_4009_4009_4009);
    let mut failures = Vec::new();
    let mut angle_adjacent = 0usize;
    for index in 0..docs {
        let input = document(&mut rng);
        match round_trip(&pipeline, &active, &input) {
            Ok(cleaned) => {
                angle_adjacent += usize::from(cleaned.contains("><") || cleaned.contains(">>"));
            }
            Err(failure) => failures.push(format!("#{index} {input:?}\n  cleaned: {failure}")),
        }
    }
    eprintln!(
        "angle probe: docs={docs} failures={} angle_adjacent_ok={angle_adjacent}",
        failures.len()
    );
    if let Ok(path) = std::env::var("GAZE_ANGLE_PROBE_DUMP") {
        std::fs::write(path, failures.join("\n")).expect("write failure dump");
    }
    // The corpus must actually exercise the defect class, or a green run proves nothing.
    assert!(
        angle_adjacent * 20 >= docs,
        "only {angle_adjacent} of {docs} docs put an angle bracket beside a token"
    );
    assert!(
        failures.is_empty(),
        "{} of {docs} docs failed strict restore; first:\n{}",
        failures.len(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
