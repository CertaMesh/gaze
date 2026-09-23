//! Containment precedence and the per-character protection safety net on the
//! bundled `core` + `locale-de` rulepacks (solo todo #3740, concept v2).
//!
//! One entity, one token: a candidate that wholly contains a candidate of
//! another class wins the whole span when it is at least as certain
//! (validator > anchored or structural cue > plain regex or dictionary >
//! learned NER; ties go to the container). Under every policy, every byte a
//! protected class claimed leaves protected, even inside a `preserve` winner.
//!
//! Every expected string below is the exact clean text with the session hex
//! normalised away; the byte assertions never quote a token's random prefix.
use std::sync::{Arc, Mutex};

use gaze::{
    Action, Candidate, CleanDocument, ConflictTier, Context, DetectContext, DictionaryBundle,
    EmittedTokenSpan, LocaleChain, PiiClass, Policy, RawDocument, Recognizer, RedactionEntry,
    RedactionLogError, RedactionLogger, Rulepack, RulepackSource, SafetyNetPolicy, Scope, Session,
};
use gaze_assembly::build_pipeline_builder;

const PL_LETTER: &str = "IBAN PL56 0942 8981 7280 5663 2200 4500 BIC";
const PL_TARGET: &str = "IBAN <Custom:iban_1> BIC";
/// Candidate pool of the PL letter under `core` + `locale-de`, de-AT (byte
/// offsets): the IBAN (mod-97 valid), a Luhn-valid card run, a German
/// national phone shape, and an Austrian postal code before `BIC`.
const PL_CLAIMS: [(&str, &str); 4] = [
    ("custom:iban", "PL56 0942 8981 7280 5663 2200 4500"),
    ("custom:credit_card", "0942 8981 7280 5663"),
    ("custom:phone", "0942 8981 7280"),
    ("custom:postal_code", "4500"),
];
const CARD_PHONE: &str = "Karte 4539 1488 0343 6467 bitte";
const ADDRESS: &str = "Adresse: Hauptstraße 5, 10115 Berlin";
const URL_WITH_EMAIL: &str = "see https://mail.example.org/u/anna@example.org now";
const JSON_PHONE: &str = r#"{"phone": "+49 30 1234567"}"#;
const JSON_IBAN: &str = r#"{"customer": "K-1", "iban": "DE89 3704 0044 0532 0130 00", "ok": true}"#;
const CONTACT_LINE: &str = "Kontakt: +49 30 1234567, anna@example.org";
/// No IBAN cue, a Luhn-valid card run inside: family policy settles the
/// card-versus-IBAN rivalry as `custom:iban` before containment runs.
const AL_LETTER: &str = "Bitte überweisen auf AL93 8581 4730 8741 0352 4259 0155 BIC";

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

/// A learned-tier stand-in: a recognizer whose id is `ner` and whose source
/// is `ner/<backend>`, exactly how the bundled NER recognizer labels its
/// candidates, covering one fixed span.
#[derive(Clone)]
struct LearnedSpan {
    class: PiiClass,
    span: std::ops::Range<usize>,
}
impl Recognizer for LearnedSpan {
    fn id(&self) -> &str {
        "ner"
    }
    fn supported_class(&self) -> &PiiClass {
        &self.class
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(&self, _: &str, _: &DetectContext<'_>) -> Result<Vec<Candidate>, gaze::DetectError> {
        Ok(vec![Candidate::new(
            self.span.clone(),
            self.class.clone(),
            "ner",
            0.99,
            0,
            None,
            "counter",
            "ner/stand-in",
            ConflictTier::None,
            Vec::new(),
        )])
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

/// A `core` + `locale-de` policy: every class tokenized except the named
/// overrides, an optional custom recognizer block, one document locale.
fn policy(locale: &str, overrides: &[(&str, Action)], extra: &str) -> Policy {
    let mut text = format!(
        "schema_version = \"0.1.0\"\n\n[session]\nscope = \"persistent\"\nttl_secs = 86400\n\n\
         [policy.rulepacks]\nbundled = [\"core\", \"locale-de\"]\n\n[locale]\nactive = [\"{locale}\"]\n{extra}\n"
    );
    for (class, action) in overrides {
        text.push_str(&format!(
            "[[rule]]\nkind = \"class\"\nclass = \"{class}\"\naction = \"{}\"\n\n",
            action_name(*action)
        ));
    }
    text.push_str("[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n");
    // The policy loader reads files only; write the document to a private
    // temp file named after this process and thread.
    let path = std::env::temp_dir().join(format!(
        "gaze-containment-precedence-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, text).expect("write policy");
    let policy = Policy::load(&path).expect("policy");
    let _ = std::fs::remove_file(&path);
    policy
}

struct Run {
    text: String,
    manifest: Vec<EmittedTokenSpan>,
    logger: MemoryLogger,
    session: Session,
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

fn clean_run(policy: &Policy, input: &str, learned: Option<LearnedSpan>) -> Run {
    let rulepacks = rulepacks();
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let logger = MemoryLogger::default();
    let mut builder = build_pipeline_builder(policy, &context, rulepacks, &active, None)
        .expect("builder")
        .redaction_logger(logger.clone());
    if let Some(learned) = learned {
        builder = builder.recognizer(learned);
    }
    let pipeline = builder.build().expect("pipeline");
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

/// The clean text with every token's random session prefix removed, so
/// `<1a2b3c4d:Custom:iban_1>` reads `<Custom:iban_1>`.
fn shape(text: &str) -> String {
    regex::Regex::new(r"<[0-9a-f]{8}:")
        .expect("regex")
        .replace_all(text, "<")
        .into_owned()
}

/// Raw bytes of `input` that the clean text copied verbatim: the complement
/// of the manifest's raw spans. Output bytes, never manifest arithmetic: the
/// clean text is checked against the manifest too.
fn raw_survivors(input: &str, run: &Run) -> Vec<(usize, usize)> {
    let mut covered = vec![false; input.len()];
    for span in &run.manifest {
        let raw = &input[span.raw_span.clone()];
        let clean = &run.text[span.clean_span.clone()];
        assert_ne!(
            raw, clean,
            "a manifest entry must replace its bytes: {:?}",
            span.raw_span
        );
        for flag in &mut covered[span.raw_span.clone()] {
            *flag = true;
        }
    }
    let mut runs = Vec::new();
    let mut start = None;
    for (index, flag) in covered.iter().enumerate() {
        match (start, *flag) {
            (None, false) => start = Some(index),
            (Some(from), true) => {
                runs.push((from, index));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        runs.push((from, input.len()));
    }
    runs
}

fn byte_range(input: &str, needle: &str) -> (usize, usize) {
    let start = input.find(needle).expect("claim present in the fixture");
    (start, start + needle.len())
}

/// The protection invariant, on output bytes: no byte claimed by a class the
/// policy protects survives raw.
fn assert_protected_claims_replaced(
    input: &str,
    run: &Run,
    claims: &[(&str, &str)],
    action_of: impl Fn(&str) -> Action,
) {
    let survivors = raw_survivors(input, run);
    for (class, claim) in claims {
        if !action_of(class).is_protective() {
            continue;
        }
        let (from, to) = byte_range(input, claim);
        for (start, end) in &survivors {
            assert!(
                *end <= from || to <= *start,
                "{class} claimed {from}..{to} but bytes {start}..{end} left raw: {}",
                shape(&run.text)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// One entity, one token.

/// The reference letter: five tokens on main, one IBAN token now.
#[test]
fn pl_letter_is_one_iban_token_under_all_tokenize() {
    let run = clean_run(&policy("de-AT", &[], ""), PL_LETTER, None);
    assert_eq!(shape(&run.text), PL_TARGET);
    assert_eq!(run.manifest.len(), 1);
    assert_eq!(run.manifest[0].raw_span, 5..39);
    assert!(!run.manifest[0].origin.is_residual_fragment());
    assert_eq!(
        run.session.restore_strict_text(&run.text).unwrap(),
        PL_LETTER
    );
    let winner = run
        .logger
        .entries()
        .into_iter()
        .find(|entry| !entry.conflict_loser)
        .expect("winner row");
    assert_eq!(winner.recognizer_id.as_deref(), Some("iban.structural"));
    assert_eq!(winner.decided_by, ConflictTier::ContainmentPrecedence);
    let losers = run
        .logger
        .entries()
        .into_iter()
        .filter(|entry| entry.conflict_loser)
        .map(|entry| entry.recognizer_id.unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(
        losers.contains(&"phone.national.de".to_string())
            && losers.contains(&"postal.at_ch".to_string()),
        "swallowed candidates keep loser rows: {losers:?}"
    );
}

/// The postal code's action no longer matters: it is swallowed whole, and the
/// letter is the same one token under every action on `custom:postal_code`
/// (main shipped 20 raw IBAN bytes under `preserve`).
#[test]
fn pl_letter_is_one_iban_token_under_every_postal_action() {
    for action in ACTIONS {
        let run = clean_run(
            &policy("de-AT", &[("custom:postal_code", action)], ""),
            PL_LETTER,
            None,
        );
        assert_eq!(shape(&run.text), PL_TARGET, "postal_code = {action:?}");
        assert_eq!(
            run.session.restore_strict_text(&run.text).unwrap(),
            PL_LETTER
        );
    }
}

/// A Luhn-valid card whose tail is a German phone shape: one card token
/// (`<credit_card_1><phone_1>` on main), and preserving phones no longer
/// leaks the card (main shipped it whole).
#[test]
fn card_containing_a_phone_shape_is_one_card_token() {
    for phone in [Action::Tokenize, Action::Preserve] {
        let run = clean_run(
            &policy("de-CH", &[("custom:phone", phone)], ""),
            CARD_PHONE,
            None,
        );
        assert_eq!(
            shape(&run.text),
            "Karte <Custom:credit_card_1> bitte",
            "phone = {phone:?}"
        );
    }
}

/// An adopter address regex (plain pattern) over a bundled postal code (plain
/// pattern): equal tiers, the container wins the whole address.
#[test]
fn adopter_address_regex_swallows_the_postal_code_inside_it() {
    let extra = r#"
[[policy.custom_recognizers]]
kind = "regex"
name = "address_line"
pattern = '[A-ZÄÖÜ][a-zäöüß]+(?:straße|strasse|weg|platz|gasse) \d{1,4}[a-z]?, \d{5} [A-ZÄÖÜ][a-zäöüß]+'
class = "custom:address"
"#;
    let run = clean_run(&policy("de-DE", &[], extra), ADDRESS, None);
    assert_eq!(shape(&run.text), "Adresse: <Custom:address_1>");
}

// ---------------------------------------------------------------------------
// The guard.

/// A learned-tier span over a whole contact line must not relabel the
/// validated phone and email inside it as a name: today's shape stays.
#[test]
fn learned_container_does_not_swallow_validated_identifiers() {
    let learned = LearnedSpan {
        class: PiiClass::Name,
        span: 0..CONTACT_LINE.len(),
    };
    let run = clean_run(&policy("de-DE", &[], ""), CONTACT_LINE, Some(learned));
    assert_eq!(
        shape(&run.text),
        "<Name_1><Custom:phone_1><Name_2><Email_1>"
    );
}

/// A plain adopter regex over a validated phone (JSON field): the guard
/// refuses the container and today's shape stays.
#[test]
fn plain_regex_container_does_not_swallow_a_validated_phone() {
    let extra = r#"
[[policy.custom_recognizers]]
kind = "regex"
name = "json_string_field"
pattern = '"(?:iban|phone|email|note|address|customer)"\s*:\s*"[^"]*"'
class = "custom:json_field"
"#;
    let run = clean_run(&policy("de-DE", &[], extra), JSON_PHONE, None);
    assert_eq!(
        shape(&run.text),
        "{<Custom:json_field_1><Custom:phone_1><Custom:json_field_2>}"
    );
}

// ---------------------------------------------------------------------------
// Placement: after collision-family policy and the anchor rung.

/// A cue-less IBAN enclosing a Luhn-valid card run: family policy settles
/// the rivalry as `custom:iban` first, containment then folds the phone in,
/// and the settled span never falls to the family fallback. With the rung
/// placed before family policy the token would carry the family class.
#[test]
fn family_policy_settles_the_class_before_containment_runs() {
    for locale in ["de-AT", "de-DE"] {
        let run = clean_run(&policy(locale, &[], ""), AL_LETTER, None);
        assert_eq!(
            shape(&run.text),
            "Bitte überweisen auf <Custom:iban_1> BIC",
            "{locale}"
        );
    }
}

/// An anchored IBAN inside an adopter's JSON-field regex keeps its own
/// token: the anchor rung hands the incoming IBAN the slot before containment
/// could let the field swallow it.
#[test]
fn anchor_rung_keeps_an_anchored_iban_inside_a_plain_container() {
    let extra = r#"
[[policy.custom_recognizers]]
kind = "regex"
name = "json_string_field"
pattern = '"(?:iban|phone|email|note|address|customer)"\s*:\s*"[^"]*"'
class = "custom:json_field"
"#;
    let run = clean_run(&policy("de-DE", &[], extra), JSON_IBAN, None);
    assert_eq!(
        shape(&run.text),
        r#"{<Custom:json_field_1>, <Custom:json_field_2><Custom:iban_1><Custom:json_field_3>, "ok": true}"#
    );
}

// ---------------------------------------------------------------------------
// The protection safety net.

/// A preserved URL containing a tokenized email: the email's bytes leave as
/// one fragment inside the preserved URL (main shipped the email raw), and
/// the fragment's audit row says protection overrode a preserve.
#[test]
fn email_inside_a_preserved_url_leaves_as_one_fragment() {
    let run = clean_run(
        &policy("de-DE", &[("custom:url", Action::Preserve)], ""),
        URL_WITH_EMAIL,
        None,
    );
    assert_eq!(
        shape(&run.text),
        "see https://mail.example.org/u/<Email_1> now"
    );
    assert_eq!(run.manifest.len(), 1);
    assert_eq!(run.manifest[0].raw_span, 31..47);
    assert!(run.manifest[0].origin.is_residual_fragment());
    assert_eq!(
        run.session.restore_strict_text(&run.text).unwrap(),
        URL_WITH_EMAIL
    );
    let fragment = run
        .logger
        .entries()
        .into_iter()
        .find(|entry| entry.provenance_stage.as_deref() == Some("primary_pipeline.residual"))
        .expect("fragment row");
    assert_eq!(fragment.class, PiiClass::Email);
    assert_eq!(fragment.action, Action::Tokenize);
    assert_eq!(fragment.decided_by, ConflictTier::ProtectionOverride);
    // Tokenizing the URL is unchanged: one URL token, the email inside it.
    let run = clean_run(&policy("de-DE", &[], ""), URL_WITH_EMAIL, None);
    assert_eq!(shape(&run.text), "see <Custom:url_1> now");
}

/// The invariant on output bytes over every action value on the container
/// class and on each inner class of the reference letter: no byte claimed
/// by a protected class leaves raw, whatever the neighbours' actions, and a
/// fragment inside a preserved IBAN carries its claimant's own action.
///
/// Arms: a protective IBAN swallows the inner candidates whatever their
/// actions, so each inner class is varied alone against the four protective
/// IBAN actions; a preserved IBAN exposes the inner claims, so their actions
/// are enumerated in full (`5 x 5 x 5`).
#[test]
fn every_action_matrix_on_the_pl_letter_leaves_no_protected_claim_raw() {
    let classes = [
        "custom:iban",
        "custom:credit_card",
        "custom:phone",
        "custom:postal_code",
    ];
    let mut arms = Vec::new();
    for iban in ACTIONS {
        if iban == Action::Preserve {
            for card in ACTIONS {
                for phone in ACTIONS {
                    for postal in ACTIONS {
                        arms.push([iban, card, phone, postal]);
                    }
                }
            }
        } else {
            for inner in 1..4 {
                for action in ACTIONS {
                    let mut arm = [iban, Action::Tokenize, Action::Tokenize, Action::Tokenize];
                    arm[inner] = action;
                    arms.push(arm);
                }
            }
        }
    }
    assert_eq!(arms.len(), 125 + 60);
    for actions in arms {
        let [iban, card, phone, postal] = actions;
        {
            {
                {
                    let overrides = classes.iter().copied().zip(actions).collect::<Vec<_>>();
                    let run = clean_run(&policy("de-AT", &overrides, ""), PL_LETTER, None);
                    let action_of = |class: &str| {
                        actions[classes.iter().position(|c| *c == class).expect("class")]
                    };
                    assert_protected_claims_replaced(PL_LETTER, &run, &PL_CLAIMS, action_of);
                    let text = shape(&run.text);
                    match iban {
                        // The IBAN owns the whole span: one replacement under
                        // its own action, the inner candidates never surface.
                        Action::Tokenize => assert_eq!(text, PL_TARGET, "{actions:?}"),
                        Action::Redact => assert_eq!(text, "IBAN [REDACTED] BIC", "{actions:?}"),
                        Action::Generalize => assert_eq!(text, "IBAN [IBAN] BIC", "{actions:?}"),
                        Action::FormatPreserve => {
                            assert!(
                                text.starts_with("IBAN ") && text.ends_with(" BIC"),
                                "{actions:?}: {text}"
                            );
                            assert_eq!(run.manifest.len(), 1, "{actions:?}: {text}");
                        }
                        // A preserved IBAN keeps only the bytes no protected
                        // class claimed; the card run (the highest-ranked
                        // claimant of the middle) and the postal code leave
                        // as fragments under their own actions.
                        Action::Preserve => {
                            let marker = |class: &str| {
                                gaze::redaction_marker(&PiiClass::custom(class).expect("class"))
                            };
                            let middle = match card {
                                Action::Tokenize | Action::FormatPreserve => {
                                    "<Custom:credit_card_1>".to_string()
                                }
                                Action::Redact => marker("credit_card"),
                                Action::Generalize => "[CREDIT_CARD]".to_string(),
                                Action::Preserve => match phone {
                                    Action::Tokenize | Action::FormatPreserve => {
                                        "<Custom:phone_1> 5663".to_string()
                                    }
                                    Action::Redact => format!("{} 5663", marker("phone")),
                                    Action::Generalize => "[PHONE] 5663".to_string(),
                                    Action::Preserve => "0942 8981 7280 5663".to_string(),
                                    _ => unreachable!(),
                                },
                                _ => unreachable!(),
                            };
                            let tail = match postal {
                                Action::Tokenize | Action::FormatPreserve => {
                                    "<Custom:postal_code_1>".to_string()
                                }
                                Action::Redact => marker("postal_code"),
                                Action::Generalize => "[POSTAL_CODE]".to_string(),
                                Action::Preserve => "4500".to_string(),
                                _ => unreachable!(),
                            };
                            assert_eq!(
                                text,
                                format!("IBAN PL56 {middle} 2200 {tail} BIC"),
                                "{actions:?}"
                            );
                            for entry in run.logger.entries() {
                                if entry.provenance_stage.as_deref()
                                    == Some("primary_pipeline.residual")
                                {
                                    assert_eq!(
                                        entry.decided_by,
                                        ConflictTier::ProtectionOverride,
                                        "{actions:?}"
                                    );
                                    assert!(entry.action.is_protective(), "{actions:?}");
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                    if actions.iter().all(|action| {
                        matches!(
                            action,
                            Action::Tokenize | Action::FormatPreserve | Action::Preserve
                        )
                    }) {
                        assert_eq!(
                            run.session.restore_strict_text(&run.text).unwrap(),
                            PL_LETTER,
                            "{actions:?}"
                        );
                    }
                }
            }
        }
    }
}
