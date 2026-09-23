//! Nym-small as a recognizer (single-pass Stage A) inside a real pipeline: the bundled `core` +
//! `locale-de` rule floor plus the four Nym adapters over a scripted model, so every test runs
//! without the model bundle. What is pinned:
//!
//! * a learned (tier 1) Nym candidate never swallows a rule candidate it contains, and a rule
//!   container (tier 2 or above) swallows a Nym candidate as one token;
//! * a rule wins every other overlap, and the Nym bytes outside the rule token still leave
//!   protected (the per-character residual net);
//! * the matrix of every action on the Nym class against every action on the rule class
//!   (twenty-five pairs per shape) never ships a byte a protected class claimed, scored on the
//!   output bytes;
//! * the model reads the normalized text and its spans map back to the raw bytes;
//! * one inference per request across the four adapters, no stale result across requests, no
//!   sharing between concurrent requests;
//! * the default assembled pipeline never registers or loads Nym.
//!
//! Every expected string is the exact clean text with the session hex normalised away.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use gaze::{
    Action, CleanDocument, ConflictTier, Context, DictionaryBundle, EmittedTokenSpan, LocaleChain,
    PiiClass, Policy, RawDocument, RedactionEntry, RedactionLogError, RedactionLogger, Rulepack,
    RulepackSource, SafetyNetPolicy, Scope, Session,
};
use gaze_assembly::build_pipeline_builder;
use gaze_recognizers::nym_recognizer::test_support::{ScriptLog, ScriptedSpan};
use gaze_recognizers::safety_net::nym::{NymLabel, NymOperatingPoint};
use gaze_recognizers::NymRecognizers;

const ACTIONS: [Action; 5] = [
    Action::Tokenize,
    Action::Preserve,
    Action::Redact,
    Action::FormatPreserve,
    Action::Generalize,
];

#[derive(Clone, Default)]
struct MemoryLogger(Arc<Mutex<Vec<RedactionEntry>>>);
impl MemoryLogger {
    fn entries(&self) -> Vec<RedactionEntry> {
        self.0.lock().expect("entries").clone()
    }
}
impl RedactionLogger for MemoryLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.0.lock().expect("entries").push(entry.clone());
        Ok(())
    }
}

fn action_name(action: Action) -> &'static str {
    match action {
        Action::Tokenize => "tokenize",
        Action::Preserve => "preserve",
        Action::Redact => "redact",
        Action::FormatPreserve => "format_preserve",
        Action::Generalize => "generalize",
        _ => unreachable!("closed action set"),
    }
}

/// A `core` + `locale-de` policy with explicit actions for the named classes, every other class
/// tokenized, one document locale.
fn policy(locale: &str, overrides: &[(&str, Action)]) -> Policy {
    let mut text = format!(
        "schema_version = \"0.1.0\"\n\n[session]\nscope = \"persistent\"\nttl_secs = 86400\n\n\
         [policy.rulepacks]\nbundled = [\"core\", \"locale-de\"]\n\n[locale]\nactive = [\"{locale}\"]\n\n"
    );
    for (class, action) in overrides {
        text.push_str(&format!(
            "[[rule]]\nkind = \"class\"\nclass = \"{class}\"\naction = \"{}\"\n\n",
            action_name(*action)
        ));
    }
    text.push_str("[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n");
    let path = std::env::temp_dir().join(format!(
        "gaze-nym-recognizer-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, text).expect("write policy");
    let policy = Policy::load(&path).expect("policy");
    let _ = std::fs::remove_file(&path);
    policy
}

/// Every learned class tokenized explicitly, as the bench arm declares them.
fn nym_policy(locale: &str, extra: &[(&str, Action)]) -> Policy {
    let mut overrides = extra.to_vec();
    for class in [
        "custom:building_number",
        "custom:date",
        "custom:license_plate",
        "custom:username",
    ] {
        if !overrides.iter().any(|(named, _)| *named == class) {
            overrides.push((class, Action::Tokenize));
        }
    }
    policy(locale, &overrides)
}

fn rulepacks() -> &'static [Rulepack; 2] {
    static PACKS: std::sync::OnceLock<[Rulepack; 2]> = std::sync::OnceLock::new();
    PACKS.get_or_init(|| {
        ["core", "locale-de"].map(|name| {
            Rulepack::load(RulepackSource::Embedded(
                gaze_recognizers::embedded(name).expect("embedded rulepack"),
            ))
            .expect("rulepack")
        })
    })
}

/// A scripted model that flags each `(needle, label)` wherever the needle occurs in the text it
/// is given (the normalized text).
fn scripted(flags: &[(&str, NymLabel)]) -> (NymRecognizers, Arc<ScriptLog>) {
    let flags = flags
        .iter()
        .map(|(needle, label)| (needle.to_string(), *label))
        .collect::<Vec<_>>();
    NymRecognizers::scripted(NymOperatingPoint::op_b(), move |text| {
        let mut spans: Vec<ScriptedSpan> = Vec::new();
        for (needle, label) in &flags {
            for (start, _) in text.match_indices(needle.as_str()) {
                spans.push((start..start + needle.len(), *label, 0.95));
            }
        }
        spans.sort_by_key(|(range, _, _)| (range.start, range.end));
        Ok(spans)
    })
}

struct Run {
    text: String,
    manifest: Vec<EmittedTokenSpan>,
    logger: MemoryLogger,
    session: Session,
}

fn pipeline(
    policy: &Policy,
    nym: Option<&NymRecognizers>,
    logger: &MemoryLogger,
) -> gaze::Pipeline {
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let mut builder = build_pipeline_builder(policy, &context, rulepacks(), &active, None)
        .expect("builder")
        .redaction_logger(logger.clone());
    if let Some(nym) = nym {
        for adapter in nym.adapters() {
            builder = builder.recognizer(adapter);
        }
    }
    builder.build().expect("pipeline")
}

fn clean_run(policy: &Policy, input: &str, nym: Option<&NymRecognizers>) -> Run {
    let logger = MemoryLogger::default();
    let pipeline = pipeline(policy, nym, &logger);
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, manifest, _) = pipeline
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
    Run {
        text,
        manifest,
        logger,
        session,
    }
}

/// The clean text with every token's random session prefix removed.
fn shape(text: &str) -> String {
    regex::Regex::new(r"<[0-9a-f]{8}:")
        .expect("regex")
        .replace_all(text, "<")
        .into_owned()
}

/// Raw byte runs the clean text copied verbatim (the complement of the manifest's raw spans),
/// after checking every manifest entry really replaced its bytes. Output bytes, never manifest
/// arithmetic.
fn raw_survivors(input: &str, run: &Run) -> Vec<Range<usize>> {
    let mut covered = vec![false; input.len()];
    for span in &run.manifest {
        assert_ne!(
            &input[span.raw_span.clone()],
            &run.text[span.clean_span.clone()],
            "a manifest entry must replace its bytes: {:?}",
            span.raw_span
        );
        covered[span.raw_span.clone()]
            .iter_mut()
            .for_each(|flag| *flag = true);
    }
    let mut runs = Vec::new();
    let mut start = None;
    for (index, flag) in covered.iter().enumerate() {
        match (start, *flag) {
            (None, false) => start = Some(index),
            (Some(from), true) => {
                runs.push(from..index);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        runs.push(from..input.len());
    }
    runs
}

fn byte_range(input: &str, needle: &str) -> Range<usize> {
    let start = input.find(needle).expect("claim present in the fixture");
    start..start + needle.len()
}

/// No byte claimed by a class whose action protects survives raw in the output. Redact writes
/// no manifest entry, so a redacted claim is checked on the clean text instead: none of its
/// characters may appear where it stood.
fn assert_protected_claims_replaced(
    input: &str,
    run: &Run,
    claims: &[(&str, &str)],
    action_of: impl Fn(&str) -> Action,
    label: &str,
) {
    let survivors = raw_survivors(input, run);
    for (class, claim) in claims {
        let action = action_of(class);
        if !action.is_protective() {
            continue;
        }
        if action == Action::Redact {
            assert!(
                !run.text.contains(claim),
                "{label}: redacted {class} claim left raw: {}",
                shape(&run.text)
            );
            continue;
        }
        let claimed = byte_range(input, claim);
        for survivor in &survivors {
            assert!(
                survivor.end <= claimed.start || claimed.end <= survivor.start,
                "{label}: {class} claimed {claimed:?} but bytes {survivor:?} left raw: {}",
                shape(&run.text)
            );
        }
    }
}

fn winner_and_losers(run: &Run) -> (Vec<RedactionEntry>, Vec<RedactionEntry>) {
    run.logger
        .entries()
        .into_iter()
        .partition(|entry| !entry.conflict_loser)
}

// ---------------------------------------------------------------------------
// A Nym candidate alone.

#[test]
fn a_lone_nym_candidate_is_one_token_of_its_class_with_nym_provenance() {
    let input = "Das Fahrzeug mit dem Kennzeichen M-AB 1234 wurde abgeschleppt.";
    let (nym, log) = scripted(&[("M-AB 1234", NymLabel::LicensePlate)]);
    let run = clean_run(&nym_policy("de-DE", &[]), input, Some(&nym));
    assert_eq!(
        shape(&run.text),
        "Das Fahrzeug mit dem Kennzeichen <Custom:license_plate_1> wurde abgeschleppt."
    );
    assert_eq!(run.session.restore_strict_text(&run.text).unwrap(), input);
    assert_eq!(log.calls(), 1);
    let (winners, _) = winner_and_losers(&run);
    let row = winners
        .iter()
        .find(|entry| entry.class == PiiClass::custom("license_plate").unwrap())
        .expect("plate row");
    assert_eq!(row.source, "nym/license_plate");
    assert_eq!(row.recognizer_id.as_deref(), Some("nym/license_plate"));
    assert!(
        row.recognizer_version_id.as_deref().is_some_and(
            |id| id.contains("/LICENSE_PLATE>=0.5/") && id.ends_with("/input=normalized")
        ),
        "{:?}",
        row.recognizer_version_id
    );
}

// ---------------------------------------------------------------------------
// Containment: rule container over a Nym candidate, Nym span over a rule candidate.

const IBAN_LETTER: &str = "Bitte auf IBAN DE89 3704 0044 0532 0130 00 überweisen.";
const IBAN: &str = "DE89 3704 0044 0532 0130 00";

/// A validated IBAN (tier 4) contains a Nym building-number guess on one of its groups: the
/// IBAN swallows it as one token and the Nym candidate keeps a loser row naming the rung.
#[test]
fn a_rule_container_swallows_a_contained_nym_candidate() {
    let (nym, _) = scripted(&[("0532", NymLabel::BuildingNumber)]);
    let run = clean_run(&nym_policy("de-DE", &[]), IBAN_LETTER, Some(&nym));
    assert_eq!(
        shape(&run.text),
        "Bitte auf IBAN <Custom:iban_1> überweisen."
    );
    let (_, losers) = winner_and_losers(&run);
    let nym_loser = losers
        .iter()
        .find(|entry| entry.recognizer_id.as_deref() == Some("nym/building_number"))
        .expect("the swallowed Nym candidate keeps a loser row");
    assert_eq!(nym_loser.decided_by, ConflictTier::ContainmentPrecedence);
}

const ADDRESS: &str = "Adresse: Hauptstraße 5, 10115 Berlin";

/// A Nym span that contains a plain-regex postal code (tier 2) never swallows it: the rule
/// token stays, and the Nym bytes around it leave as fragments of the Nym class.
#[test]
fn a_nym_span_never_swallows_a_contained_rule_candidate() {
    let (nym, _) = scripted(&[("Hauptstraße 5, 10115 Berlin", NymLabel::BuildingNumber)]);
    let run = clean_run(&nym_policy("de-DE", &[]), ADDRESS, Some(&nym));
    assert_eq!(
        shape(&run.text),
        "Adresse: <Custom:building_number_1><Custom:postal_code_1><Custom:building_number_2>"
    );
    assert_eq!(run.session.restore_strict_text(&run.text).unwrap(), ADDRESS);
}

const LOGIN: &str = "Login: user anna.beispiel@example.org bitte";

/// A Nym username span over a validated email (tier 4, builtin class): the structured
/// containment rung must not hand the custom-class learned span the email either.
#[test]
fn a_nym_span_never_swallows_a_contained_validated_email() {
    let (nym, _) = scripted(&[("user anna.beispiel@example.org", NymLabel::Username)]);
    let run = clean_run(&nym_policy("de-DE", &[]), LOGIN, Some(&nym));
    assert_eq!(
        shape(&run.text),
        "Login: <Custom:username_1><Email_1> bitte"
    );
}

// ---------------------------------------------------------------------------
// Partial overlap and same-span rivalry.

const PHONE_LINE: &str = "Tel +49 30 1234567 Berlin";

/// A validated phone and a Nym span that overlap without containment: the phone wins, and the
/// Nym bytes it does not cover stay protected.
#[test]
fn a_rule_wins_a_partial_overlap_and_the_nym_remainder_stays_protected() {
    let (nym, _) = scripted(&[("1234567 Berlin", NymLabel::BuildingNumber)]);
    let run = clean_run(&nym_policy("de-DE", &[]), PHONE_LINE, Some(&nym));
    assert_eq!(
        shape(&run.text),
        "Tel <Custom:phone_1><Custom:building_number_1>"
    );
    let (_, losers) = winner_and_losers(&run);
    assert!(losers
        .iter()
        .any(|entry| entry.recognizer_id.as_deref() == Some("nym/building_number")));
}

const BIRTH_LINE: &str = "Herr Beispiel, geboren am 12.03.1985, wohnt hier.";

/// The bundled `birth_date.cue` rule and a Nym date-of-birth span on the same bytes: different
/// classes, same span, so the base ladder decides and the rule's priority wins; the Nym
/// candidate keeps a loser row naming that rung.
#[test]
fn a_rule_wins_the_same_span_against_a_nym_candidate() {
    let (nym, _) = scripted(&[("12.03.1985", NymLabel::DateOfBirth)]);
    let run = clean_run(&nym_policy("de-DE", &[]), BIRTH_LINE, Some(&nym));
    assert_eq!(
        shape(&run.text),
        "Herr Beispiel, geboren am <Custom:birth_date_1>, wohnt hier."
    );
    let (_, losers) = winner_and_losers(&run);
    let nym_loser = losers
        .iter()
        .find(|entry| entry.recognizer_id.as_deref() == Some("nym/date_of_birth"))
        .expect("nym loser row");
    assert_eq!(nym_loser.decided_by, ConflictTier::RulePriority);
}

// ---------------------------------------------------------------------------
// Every action on the Nym class against every action on the rule class, per shape.

struct Shape {
    name: &'static str,
    locale: &'static str,
    input: &'static str,
    nym: (&'static str, NymLabel, &'static str),
    rule: (&'static str, &'static str),
}

fn shapes() -> [Shape; 4] {
    [
        Shape {
            name: "rule container",
            locale: "de-DE",
            input: IBAN_LETTER,
            nym: ("0532", NymLabel::BuildingNumber, "custom:building_number"),
            rule: ("custom:iban", IBAN),
        },
        Shape {
            name: "nym container",
            locale: "de-DE",
            input: ADDRESS,
            nym: (
                "Hauptstraße 5, 10115 Berlin",
                NymLabel::BuildingNumber,
                "custom:building_number",
            ),
            rule: ("custom:postal_code", "10115"),
        },
        Shape {
            name: "nym container over email",
            locale: "de-DE",
            input: LOGIN,
            nym: (
                "user anna.beispiel@example.org",
                NymLabel::Username,
                "custom:username",
            ),
            rule: ("Email", "anna.beispiel@example.org"),
        },
        Shape {
            name: "partial overlap",
            locale: "de-DE",
            input: PHONE_LINE,
            nym: (
                "1234567 Berlin",
                NymLabel::BuildingNumber,
                "custom:building_number",
            ),
            rule: ("custom:phone", "+49 30 1234567"),
        },
    ]
}

/// Twenty-five action pairs per shape, scored on output bytes: whatever the pair, no byte a
/// protected class claimed ships raw, restore is exact where the output is reversible, and
/// with both classes tokenized the Nym candidate never takes the rule's bytes.
#[test]
fn every_action_pair_leaves_no_protected_claim_raw() {
    let mut runs = 0;
    for shape_case in shapes() {
        let (nym_needle, nym_label, nym_class) = shape_case.nym;
        let (rule_class, rule_claim) = shape_case.rule;
        let (nym, _) = scripted(&[(nym_needle, nym_label)]);
        for nym_action in ACTIONS {
            for rule_action in ACTIONS {
                let label = format!(
                    "{}: {nym_class}={nym_action:?} {rule_class}={rule_action:?}",
                    shape_case.name
                );
                let run = clean_run(
                    &nym_policy(
                        shape_case.locale,
                        &[(nym_class, nym_action), (rule_class, rule_action)],
                    ),
                    shape_case.input,
                    Some(&nym),
                );
                let action_of = |class: &str| {
                    if class == nym_class {
                        nym_action
                    } else {
                        rule_action
                    }
                };
                assert_protected_claims_replaced(
                    shape_case.input,
                    &run,
                    &[(nym_class, nym_needle), (rule_class, rule_claim)],
                    action_of,
                    &label,
                );
                let reversible = |action: Action| {
                    matches!(
                        action,
                        Action::Tokenize | Action::Preserve | Action::FormatPreserve
                    )
                };
                if reversible(nym_action) && reversible(rule_action) {
                    assert_eq!(
                        run.session.restore_strict_text(&run.text).unwrap(),
                        shape_case.input,
                        "{label}: {}",
                        shape(&run.text)
                    );
                }
                runs += 1;
            }
        }
    }
    assert_eq!(runs, 4 * 25);
}

// ---------------------------------------------------------------------------
// Input representation.

/// The model reads the normalized text (joiners dropped, fullwidth folded) and its spans map
/// back onto the raw bytes, joiner and fullwidth characters included.
#[test]
fn the_model_reads_normalized_text_and_spans_map_back_to_raw_bytes() {
    let input = "Benutzer jdoe\u{200D}_1977 und Kennzeichen \u{FF2D}-AB 1234 bitte";
    let normalized = "Benutzer jdoe_1977 und Kennzeichen M-AB 1234 bitte";
    let (nym, log) = scripted(&[
        ("jdoe_1977", NymLabel::Username),
        ("M-AB 1234", NymLabel::LicensePlate),
    ]);
    let run = clean_run(&nym_policy("de-DE", &[]), input, Some(&nym));
    assert_eq!(log.inputs(), [normalized.to_string()]);
    assert_eq!(
        shape(&run.text),
        "Benutzer <Custom:username_1> und Kennzeichen <Custom:license_plate_1> bitte"
    );
    let raw = run
        .manifest
        .iter()
        .map(|span| &input[span.raw_span.clone()])
        .collect::<Vec<_>>();
    assert_eq!(raw, ["jdoe\u{200D}_1977", "\u{FF2D}-AB 1234"]);
    assert_eq!(run.session.restore_strict_text(&run.text).unwrap(), input);
}

// ---------------------------------------------------------------------------
// Request scoping.

#[test]
fn one_inference_per_request_and_none_stale_across_requests() {
    let (nym, log) = scripted(&[("M-AB 1234", NymLabel::LicensePlate)]);
    let logger = MemoryLogger::default();
    let policy = nym_policy("de-DE", &[]);
    let pipeline = pipeline(&policy, Some(&nym), &logger);
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = |text: &str| {
        let (clean, _, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(text.to_string()),
                active.as_slice(),
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .expect("clean");
        let CleanDocument::Text(text) = clean else {
            panic!("expected text");
        };
        shape(&text)
    };
    assert_eq!(
        clean("Kennzeichen M-AB 1234"),
        "Kennzeichen <Custom:license_plate_1>"
    );
    assert_eq!(log.calls(), 1, "four adapters share one inference");
    assert_eq!(clean("Kennzeichen unbekannt"), "Kennzeichen unbekannt");
    assert_eq!(log.calls(), 2, "the second request runs its own inference");
}

#[test]
fn concurrent_requests_never_share_an_inference() {
    let (nym, log) = scripted(&[("M-AB 1234", NymLabel::LicensePlate)]);
    let logger = MemoryLogger::default();
    let policy = nym_policy("de-DE", &[]);
    let pipeline = Arc::new(pipeline(&policy, Some(&nym), &logger));
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let threads = (0..8)
        .map(|index| {
            let pipeline = Arc::clone(&pipeline);
            let active = active.clone();
            std::thread::spawn(move || {
                let session = Session::new(Scope::Ephemeral).expect("session");
                let text = if index % 2 == 0 {
                    format!("Kennzeichen M-AB 1234 Nummer {index}")
                } else {
                    format!("Kennzeichen unbekannt Nummer {index}")
                };
                let (clean, _, _) = pipeline
                    .clean_with_safety_net_policy_detect_context(
                        &session,
                        RawDocument::Text(text),
                        active.as_slice(),
                        &DictionaryBundle::default(),
                        SafetyNetPolicy::default(),
                    )
                    .expect("clean");
                let CleanDocument::Text(text) = clean else {
                    panic!("expected text");
                };
                (index, shape(&text))
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        let (index, text) = thread.join().expect("thread");
        if index % 2 == 0 {
            assert_eq!(
                text,
                format!("Kennzeichen <Custom:license_plate_1> Nummer {index}")
            );
        } else {
            assert_eq!(text, format!("Kennzeichen unbekannt Nummer {index}"));
        }
    }
    assert_eq!(log.calls(), 8, "one inference per request, never shared");
}

// ---------------------------------------------------------------------------
// Opt-in only.

/// The default assembly registers no Nym adapter, whatever the environment says.
#[test]
fn the_default_pipeline_never_registers_nym() {
    let logger = MemoryLogger::default();
    let pipeline = pipeline(&policy("de-DE", &[]), None, &logger);
    for label in NymLabel::ALL {
        assert!(pipeline
            .registry()
            .recognizer(&format!("nym/{label}"))
            .is_none());
    }
}

// ---------------------------------------------------------------------------
// Captured real-model output: agent tool calls.
//
// `fixtures/nym_recognizer_pieces.json` holds the tokenizer offsets and per-piece scores the
// pinned model produced for these texts as the recognizer reads them (normalized). The
// `captured_*` tests replay them through the production decoder at the frozen recognizer
// operating point, so the default run checks real model behaviour without the bundle; the
// ignored `live_*` tests rerun the model (xtask safety-net-sanity runs them when
// `GAZE_NYM_MODEL_DIR` is set) and one proves the fixture is still what the model outputs.

use std::collections::HashMap;

use gaze_recognizers::nym_recognizer::recognizer_operating_point;
use gaze_recognizers::safety_net::nym::test_support::{capture, decode_captured, PieceScore};
use gaze_recognizers::safety_net::nym::{NymConfig, NymSafetyNet};
use gaze_recognizers::safety_net::SafetyNetError;
use serde_json::{json, Value};

const CAPTURE: &str = include_str!("fixtures/nym_recognizer_pieces.json");
const CAPTURE_PATH: &str = "tests/fixtures/nym_recognizer_pieces.json";

/// Synthetic, fictional tool-call traffic: the Nym labels sit in values, next to keys that name
/// them. Keys are not personal data and must survive byte for byte.
fn tool_call_cases() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "tool-call-en",
            r#"{"tool": "vehicle_lookup", "arguments": {"license_plate": "M-AB 1234", "owner_username": "jdoe_1977"}}"#,
        ),
        (
            "tool-call-profile-en",
            r#"{"name": "update_profile", "arguments": {"username": "anna.schmidt92", "building_number": "12a", "date_of_birth": "1985-03-12"}}"#,
        ),
        (
            "tool-call-de",
            r#"{"werkzeug": "fahrzeug_suchen", "parameter": {"kennzeichen": "HH-XY 4711", "benutzername": "mueller_x9", "geburtsdatum": "12.03.1985", "hausnummer": "7b"}}"#,
        ),
        (
            "tool-result-de",
            r#"{"role": "tool", "content": "Das Fahrzeug B-XY 99E gehört dem Benutzer k.wagner, Hausnummer 14."}"#,
        ),
    ]
}

/// Keys of a JSON text, every nesting level.
fn json_keys(text: &str) -> Vec<String> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    out.push(key.clone());
                    walk(value, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            _ => {}
        }
    }
    let mut keys = Vec::new();
    walk(
        &serde_json::from_str(text).expect("fixture is JSON"),
        &mut keys,
    );
    keys
}

type Pieces = (Vec<(usize, usize)>, Vec<PieceScore>);

fn committed_capture() -> HashMap<String, Pieces> {
    let fixture: Value = serde_json::from_str(CAPTURE).expect("capture is JSON");
    fixture["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| {
            let offsets = case["offsets"]
                .as_array()
                .expect("offsets")
                .iter()
                .map(|pair| {
                    (
                        pair[0].as_u64().unwrap() as usize,
                        pair[1].as_u64().unwrap() as usize,
                    )
                })
                .collect();
            let scores = case["scores"]
                .as_array()
                .expect("scores")
                .iter()
                .map(|score| PieceScore {
                    label: NymLabel::parse(score[0].as_str().unwrap()).expect("label"),
                    mass: score[1].as_f64().unwrap() as f32,
                    is_begin: score[2].as_bool().unwrap(),
                })
                .collect();
            (
                case["text"].as_str().unwrap().to_string(),
                (offsets, scores),
            )
        })
        .collect()
}

/// The adapters over the committed capture at the frozen recognizer operating point; a text
/// without a capture fails the request, like a model error would.
fn captured_nym() -> NymRecognizers {
    let operating_point = recognizer_operating_point().expect("frozen operating point");
    let capture = committed_capture();
    let decoder_point = operating_point.clone();
    NymRecognizers::scripted(operating_point, move |text| {
        let (offsets, scores) = capture.get(text).ok_or_else(|| SafetyNetError::Runtime {
            message: "no committed capture for this text".to_string(),
        })?;
        decode_captured(text, offsets, scores, &decoder_point)
    })
    .0
}

fn assert_keys_survive(case: &str, input: &str, clean: &str) {
    for key in json_keys(input) {
        assert!(
            clean.contains(&format!("\"{key}\"")),
            "{case}: key `{key}` did not survive: {}",
            shape(clean)
        );
    }
    serde_json::from_str::<Value>(clean)
        .unwrap_or_else(|error| panic!("{case}: clean text is no longer JSON ({error})"));
}

#[test]
fn captured_tool_calls_keep_every_key_and_stay_json() {
    let nym = captured_nym();
    for (case, input) in tool_call_cases() {
        let locale = if case.ends_with("-de") {
            "de-DE"
        } else {
            "en-US"
        };
        let run = clean_run(&nym_policy(locale, &[]), input, Some(&nym));
        assert_keys_survive(case, input, &run.text);
        assert_eq!(
            run.session.restore_strict_text(&run.text).unwrap(),
            input,
            "{case}"
        );
    }
}

/// The tool-call values the model flags at the frozen operating point leave as tokens of the
/// learned classes; exact shapes pinned from the committed capture.
#[test]
fn captured_tool_call_values_leave_as_learned_class_tokens() {
    let nym = captured_nym();
    let expected: HashMap<&str, &str> = captured_tool_call_expectations().into_iter().collect();
    for (case, input) in tool_call_cases() {
        let locale = if case.ends_with("-de") {
            "de-DE"
        } else {
            "en-US"
        };
        let run = clean_run(&nym_policy(locale, &[]), input, Some(&nym));
        assert_eq!(shape(&run.text), expected[case], "{case}");
    }
}

/// Structured documents are cleaned leaf by leaf, keys never scanned: each leaf is its own
/// request with its own inference.
#[test]
fn captured_structured_leaves_are_separate_requests() {
    let nym = captured_nym();
    let logger = MemoryLogger::default();
    let policy = nym_policy("en-US", &[]);
    let pipeline = pipeline(&policy, Some(&nym), &logger);
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let raw = RawDocument::Structured(std::collections::BTreeMap::from([
        (
            "license_plate".to_string(),
            gaze::Value::String("M-AB 1234".to_string()),
        ),
        (
            "username".to_string(),
            gaze::Value::String("jdoe_1977".to_string()),
        ),
    ]));
    // The structured path only observes (no net is registered here either way).
    let observe =
        SafetyNetPolicy::new(gaze::SafetyNetMode::Strict, gaze::SafetyNetFallback::Redact);
    let (clean, _, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            raw,
            active.as_slice(),
            &DictionaryBundle::default(),
            observe,
        )
        .expect("clean");
    let CleanDocument::Structured(fields) = clean else {
        panic!("expected structured output");
    };
    let shaped = fields
        .iter()
        .map(|(key, value)| (key.clone(), shape(value.as_str().expect("string leaf"))))
        .collect::<Vec<_>>();
    assert_eq!(shaped, captured_structured_expectations());
}

// ---- live: real pinned bundle ----

fn live_net() -> NymSafetyNet {
    let net = NymSafetyNet::new(
        NymConfig::from_env().expect("set GAZE_NYM_MODEL_DIR to a verified nym bundle"),
    );
    net.preload().expect("pinned nym bundle loads");
    net
}

fn capture_texts() -> Vec<String> {
    let mut texts = tool_call_cases()
        .into_iter()
        .map(|(_, text)| gaze::normalize_for_tests(text).0)
        .collect::<Vec<_>>();
    texts.extend(["M-AB 1234".to_string(), "jdoe_1977".to_string()]);
    texts
}

fn capture_all(net: &NymSafetyNet) -> Value {
    let cases = capture_texts()
        .into_iter()
        .map(|text| {
            let (offsets, scores) = capture(net, &text).expect("capture");
            json!({
                "text": text,
                "offsets": offsets.iter().map(|(s, e)| json!([s, e])).collect::<Vec<_>>(),
                "scores": scores
                    .iter()
                    .map(|score| json!([score.label.as_str(), score.mass, score.is_begin]))
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "provenance": {
            "model": "Wismut/nym-pii-multilingual-small@4348999cd3c2e20c49615e9af7c6bbb45b64cd85 int8",
            "bundle_sha256": gaze_recognizers::safety_net::nym::NYM_SMALL_INT8_BUNDLE_SHA256,
            "generator": "GAZE_NYM_WRITE_FIXTURE=1 cargo test -p gaze-recognizers --features safety-net-nym,test-support --test nym_recognizer -- --ignored live_recognizer_capture_matches_the_committed_fixture",
            "input": "the normalized text the recognizer reads",
            "offsets": "tokenizer character offsets (encode_char_offsets, no special tokens)",
            "scores": "[label with the largest B+I mass, that mass, P(B) >= P(I)] per piece",
            "texts": "synthetic, fictional"
        },
        "cases": cases,
    })
}

/// Re-captures every text and compares it with the committed fixture; with
/// `GAZE_NYM_WRITE_FIXTURE` set it rewrites the fixture instead.
#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by xtask safety-net-sanity when set)"]
fn live_recognizer_capture_matches_the_committed_fixture() {
    let fresh = capture_all(&live_net());
    if std::env::var_os("GAZE_NYM_WRITE_FIXTURE").is_some() {
        std::fs::write(CAPTURE_PATH, serde_json::to_string(&fresh).unwrap() + "\n").unwrap();
        return;
    }
    let committed: Value = serde_json::from_str(CAPTURE).unwrap();
    let fresh_cases = fresh["cases"].as_array().unwrap();
    let committed_cases = committed["cases"].as_array().unwrap();
    assert_eq!(fresh_cases.len(), committed_cases.len());
    for (fresh, committed) in fresh_cases.iter().zip(committed_cases) {
        assert_eq!(fresh["text"], committed["text"]);
        assert_eq!(fresh["offsets"], committed["offsets"]);
        let fresh_scores = fresh["scores"].as_array().unwrap();
        let committed_scores = committed["scores"].as_array().unwrap();
        assert_eq!(fresh_scores.len(), committed_scores.len());
        for (a, b) in fresh_scores.iter().zip(committed_scores) {
            assert_eq!(a[0], b[0], "label");
            assert_eq!(a[2], b[2], "begin");
            assert!(
                (a[1].as_f64().unwrap() - b[1].as_f64().unwrap()).abs() < 1e-4,
                "mass {a} vs {b}"
            );
        }
    }
}

/// The real model through the real pipeline at the frozen operating point: a plate in prose
/// leaves as one learned-class token, with Nym provenance.
#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by xtask safety-net-sanity when set)"]
fn live_recognizer_tokenizes_a_plate_in_one_pass() {
    let config = NymConfig::from_env()
        .expect("set GAZE_NYM_MODEL_DIR to a verified nym bundle")
        .with_operating_point(recognizer_operating_point().expect("frozen operating point"));
    let nym = NymRecognizers::load(config).expect("pinned nym bundle loads");
    let input = "Das Fahrzeug mit dem Kennzeichen M-AB 1234 wurde abgeschleppt.";
    let run = clean_run(&nym_policy("de-DE", &[]), input, Some(&nym));
    assert_eq!(
        shape(&run.text),
        "Das Fahrzeug mit dem Kennzeichen <Custom:license_plate_1> wurde abgeschleppt."
    );
    let (winners, _) = winner_and_losers(&run);
    assert!(winners
        .iter()
        .any(|row| row.recognizer_id.as_deref() == Some("nym/license_plate")));
}

/// Pinned from the committed capture at the frozen operating point. `tool-call-de` keeps its
/// `geburtsdatum` value raw: the model's date-of-birth mass there stays under the frozen 0.95
/// threshold and no bundled rule has a cue for a bare JSON date value (a disclosed miss, not a
/// leak this change introduces).
fn captured_tool_call_expectations() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "tool-call-en",
            r#"{"tool": "vehicle_lookup", "arguments": {"license_plate": "<Custom:license_plate_1>", "owner_username": "<Custom:username_1>"}}"#,
        ),
        (
            "tool-call-profile-en",
            r#"{"name": "update_profile", "arguments": {"username": "<Custom:username_1>", "building_number": "<Custom:building_number_1>", "date_of_birth": "<Custom:date_1>"}}"#,
        ),
        (
            "tool-call-de",
            r#"{"werkzeug": "fahrzeug_suchen", "parameter": {"kennzeichen": "<Custom:license_plate_1>", "benutzername": "<Custom:username_1>", "geburtsdatum": "12.03.1985", "hausnummer": "<Custom:building_number_1>"}}"#,
        ),
        (
            "tool-result-de",
            r#"{"role": "tool", "content": "Das Fahrzeug <Custom:license_plate_1> gehört dem Benutzer <Custom:username_1>, Hausnummer <Custom:building_number_1>."}"#,
        ),
    ]
}

/// A leaf is scanned alone, without its key or any surrounding words: the model flags the
/// username leaf but not a bare plate leaf, which needs context (a disclosed miss of leaf-wise
/// structured cleaning, the same input the structured safety-net pass sees).
fn captured_structured_expectations() -> Vec<(String, String)> {
    vec![
        ("license_plate".to_string(), "M-AB 1234".to_string()),
        ("username".to_string(), "<Custom:username_1>".to_string()),
    ]
}
