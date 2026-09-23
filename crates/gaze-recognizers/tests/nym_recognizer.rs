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
    assert_eq!(row.source, "nym/LICENSE_PLATE");
    assert_eq!(row.recognizer_id.as_deref(), Some("nym/LICENSE_PLATE"));
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
        .find(|entry| entry.recognizer_id.as_deref() == Some("nym/BUILDING_NUMBER"))
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
        .any(|entry| entry.recognizer_id.as_deref() == Some("nym/BUILDING_NUMBER")));
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
        .find(|entry| entry.recognizer_id.as_deref() == Some("nym/DATE_OF_BIRTH"))
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
