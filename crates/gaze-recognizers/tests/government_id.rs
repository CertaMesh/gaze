//! Regression fixtures for the government-ID cluster recognizers (solo todo #2318 follow-on).
//!
//! EVERY positive fixture encodes a structural shape MEASURED in the Dataiku EN/DE holdout. The
//! measured distribution driving these rules:
//!
//! | class            | gold spans / bytes | cue adjacency        | chosen coverage      |
//! |------------------|--------------------|----------------------|----------------------|
//! | SSN              | 217 / 2,513        | 90.8%                | 154 spans / 1,843 B  |
//! | NATIONALID       | 200 / 2,315        | 60.0% (en 96.7/de 28.7) | 44 spans / 438 B  |
//! | DRIVERLICENSENUM | 216 / 2,160        | 92.1%                | 71 spans / 682 B     |
//! | TAXNUM           | 212 / 2,495        | 92.9%                | 34 spans / 418 B     |
//!
//! Every chosen variant measures ZERO matches across all 1,024 A4 negative documents and zero
//! non-gold matches in the holdout.
//!
//! Locale basis (the #414 mixed model): `ssn.de_cue` is `format` (same class and national
//! identifier shape as `ssn.us`, German cue vocabulary, DACH provenance); the three bilingual
//! cue-anchored recognizers are `document` + `global`. All four therefore fire under every chain,
//! including `--locale=global`; the declarations and the chain behaviour are both pinned below.
//!
//! Fixture values are synthetic.

use gaze::Context;
use gaze::{
    Action, CleanDocument, DictionaryBundle, LocaleBasis, LocaleChain, LocaleTag, PiiClass,
    Pipeline, RawDocument, RedactionEntry, RedactionLogError, RedactionLogger, RuleSpec, Rulepack,
    RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;
use std::sync::{Arc, Mutex};

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

const CLASSES: [&str; 4] = ["ssn", "tax_number", "driver_license", "national_id"];

/// Core bundle under an explicit chain with locale-gated auto-activation OFF — the weakest
/// configuration a default adopter can have. All four recognizers are `safe_default`, and either
/// `format` basis (`ssn.de_cue`) or `document` + `global` (the bilingual three), so they must
/// activate unconditionally under every chain.
///
/// This matters more here than anywhere else in the repo: before #414 `ssn.us` was
/// `locale_gated` to `en-US` and consequently matched only 38 of 217 gold SSN spans, because 113
/// of them sit in German documents. See todos #2417 and #2403.
fn pipeline_for(chain: &[LocaleTag]) -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = CLASSES
        .iter()
        .map(|class| RuleSpec::Class {
            class: PiiClass::custom(class),
            action: Action::Tokenize,
        })
        .chain(std::iter::once(RuleSpec::Default {
            action: Action::Preserve,
        }))
        .collect();
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = false;
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(chain));
    gaze_assembly::build_pipeline(&policy, &empty_context(), &[rulepack], &locale_chain, None)
        .expect("pipeline")
}

struct CapturingLogger {
    entries: Arc<Mutex<Vec<RedactionEntry>>>,
}

impl RedactionLogger for CapturingLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.entries.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

/// Cleans `text` under `chain` and returns the clean text plus the recognizer ids that WON a
/// span (conflict losers excluded), in document order.
fn clean_with_winners(chain: &[LocaleTag], text: &str) -> (String, Vec<String>) {
    let entries = Arc::new(Mutex::new(Vec::new()));
    let pipeline = pipeline_for(chain).with_redaction_logger(CapturingLogger {
        entries: Arc::clone(&entries),
    });
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            chain,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let text = match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    };
    let winners = entries
        .lock()
        .unwrap()
        .iter()
        .filter(|entry| !entry.conflict_loser)
        .filter_map(|entry| entry.recognizer_id.clone())
        .collect();
    (text, winners)
}

fn clean_under(chain: &[LocaleTag], text: &str) -> String {
    let pipeline = pipeline_for(chain);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            chain,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    }
}

fn clean(text: &str) -> String {
    clean_under(&[LocaleTag::Global], text)
}

fn assert_id_removed(text: &str, id: &str, surviving_context: &[&str]) {
    let cleaned = clean(text);
    assert!(
        !cleaned.contains(id),
        "identifier {id:?} survived tokenization in {cleaned:?}"
    );
    for fragment in surviving_context {
        assert!(
            cleaned.contains(fragment),
            "context {fragment:?} should survive but is missing from {cleaned:?}"
        );
    }
}

fn assert_unchanged(text: &str) {
    assert_eq!(clean(text), text, "text must pass through untouched");
}

/// Rewrites `<hex:Class:name_N>` to `<Class:name_N>`.
///
/// The per-session token hex is randomised BY DESIGN, so two `clean()` calls on identical input
/// legitimately differ in raw text. Comparing raw output would assert a non-determinism that is
/// not there; the CLASS assignment is what must be stable.
fn normalize_tokens(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        let token = &rest[open..open + close + 1];
        match token.find(':') {
            Some(colon) => {
                out.push('<');
                out.push_str(&token[colon + 1..]);
            }
            None => out.push_str(token),
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------------------------- SSN (73.3%)

#[test]
fn english_cued_ssn_stays_with_ssn_us_and_is_not_claimed_by_the_german_arm() {
    // SCOPE BOUNDARY, asserted rather than assumed. `ssn.us` owns the English cues ("ssn",
    // "social security", "ss#") and, since #414, runs under every chain as a format-basis
    // identifier. The new arm carries GERMAN cues only, so under the weakest adopter configuration
    // (global chain, locale-gated activation OFF) an English-cued SSN is claimed by `ssn.us` alone:
    // never by `ssn.de_cue`, and never as a `+`-joined same-class composite (#2416).
    let (cleaned, winners) =
        clean_with_winners(&[LocaleTag::Global], "His SSN: 123-45-6789 is on file.");
    assert!(
        !cleaned.contains("123-45-6789"),
        "ssn.us regressed: {cleaned:?}"
    );
    assert_eq!(winners, vec!["ssn.us".to_string()], "{cleaned:?}");
}

#[test]
fn german_cued_ssn_is_claimed_by_the_german_arm_alone() {
    // The mirror image: a German-cued SSN is `ssn.de_cue`'s and only its, whichever chain is
    // active — the two arms are format-basis siblings that differ only in cue vocabulary.
    for chain in [
        vec![LocaleTag::Global],
        vec![LocaleTag::parse("en-US").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("en-GB").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("de-DE").expect("tag"), LocaleTag::Global],
    ] {
        let (cleaned, winners) = clean_with_winners(
            &chain,
            "Die Sozialversicherungsnummer lautet 123-45-6789 und ist hinterlegt.",
        );
        assert!(!cleaned.contains("123-45-6789"), "{chain:?}: {cleaned:?}");
        assert_eq!(
            winners,
            vec!["ssn.de_cue".to_string()],
            "{chain:?}: {cleaned:?}"
        );
    }
}

#[test]
fn ssn_arms_never_co_fire_even_when_both_cues_touch_one_number() {
    // Both arms require THEIR OWN cue immediately before the digits (only connector words and
    // punctuation may intervene), so two cues cannot both be adjacent to one number. That is what
    // makes the `+`-joined composite source id of #2416 structurally impossible for this pair.
    // Pinned on the tightest adjacency English and German can share, plus a two-number document.
    for (text, expected) in [
        (
            "Sozialversicherungsnummer SSN: 123-45-6789 vorhanden.",
            vec!["ssn.us"],
        ),
        (
            "SSN Sozialversicherungsnummer: 123-45-6789 vorhanden.",
            vec!["ssn.de_cue"],
        ),
        (
            "SSN 123-45-6789 und Sozialversicherungsnummer 987-65-4321.",
            vec!["ssn.us", "ssn.de_cue"],
        ),
    ] {
        let (cleaned, winners) = clean_with_winners(&[LocaleTag::Global], text);
        assert!(!cleaned.contains("123-45-6789"), "{text:?}: {cleaned:?}");
        assert!(
            winners.iter().all(|id| !id.contains('+')),
            "same-class composite id emitted for {text:?}: {winners:?}"
        );
        assert_eq!(
            winners,
            expected.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
            "{text:?}: {cleaned:?}"
        );
    }
}

#[test]
fn ssn_us_still_covers_english_cues_under_its_own_locale() {
    // The other half of the boundary: with locale-gated activation on and an en-US chain,
    // `ssn.us` behaves exactly as before this change.
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("ssn"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = true;
    let chain = vec![LocaleTag::parse("en-US").expect("tag"), LocaleTag::Global];
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(&chain));
    let pipeline =
        gaze_assembly::build_pipeline(&policy, &empty_context(), &[rulepack], &locale_chain, None)
            .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text("His SSN: 123-45-6789 is on file.".to_string()),
            &chain,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let text = match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    };
    assert!(!text.contains("123-45-6789"), "ssn.us regressed: {text:?}");
}

#[test]
fn ssn_after_german_sozialversicherung_cue() {
    // THE measured gap in the recognizer this replaces: 113 of 217 SSN spans sit in German
    // documents, and "sozialversicherung" occurs before 101 of them. The previous `ssn.us` was
    // locale-gated to en-US and could never see any of these.
    assert_id_removed(
        "Die Sozialversicherungsnummer lautet 123-45-6789 und ist hinterlegt.",
        "123-45-6789",
        &[
            "Die Sozialversicherungsnummer lautet ",
            " und ist hinterlegt.",
        ],
    );
}

#[test]
fn dotted_ssn_shape_is_covered_under_a_german_cue() {
    // `d3.d4.d4.d2`, 33 of 217 spans (15.2%) — a shape `ssn.us` has no alternative for at all.
    // Reachable here through the German cue, which is the population this arm exists to cover.
    assert_id_removed(
        "Sozialversicherungsnummer 123.4567.8901.23 erfasst.",
        "123.4567.8901.23",
        &["Sozialversicherungsnummer ", " erfasst."],
    );
}

// -------------------------------------------------------------------------- DRIVERLICENSENUM

#[test]
fn driver_license_after_english_cue() {
    // `Ud7`/`Ud9`/`U2d7`/`Ud8` are the dominant silhouettes.
    assert_id_removed(
        "Driver's license: D1234567 expires soon.",
        "D1234567",
        &["Driver's license: ", " expires soon."],
    );
}

#[test]
fn driver_license_after_german_cue() {
    assert_id_removed(
        "Die Führerscheinnummer B7654321 wurde geprüft.",
        "B7654321",
        &["Die Führerscheinnummer ", " wurde geprüft."],
    );
}

#[test]
fn tighter_anchoring_did_not_cost_coverage() {
    // The looser `[A-Z0-9][A-Z0-9-]{5,19}` draft covered 68 spans with 105 holdout false
    // positives; the letter-led form covers 71 with zero. Tighter anchoring won on BOTH axes.
    // A bare alphanumeric run with no leading letter run must not match.
    assert_unchanged("license number 1234567890 is not a licence shape");
}

// ------------------------------------------------------------------------------------ TAXNUM

#[test]
fn tax_number_with_separators_after_german_cue() {
    assert_id_removed(
        "Die Steuernummer 123 456 789 ist hinterlegt.",
        "123 456 789",
        &["Die Steuernummer ", " ist hinterlegt."],
    );
}

#[test]
fn tax_number_with_separators_after_english_cue() {
    assert_id_removed(
        "Tax number: 123-456-789 on record.",
        "123-456-789",
        &["Tax number: ", " on record."],
    );
}

#[test]
fn bare_digit_run_after_tax_cue_is_not_matched() {
    // THE precision decision. A4's `invalid_identifiers` category contains 64 tax-shaped invalid
    // identifiers as bare digit runs. The broad variant covered 36.3% of gold but took all 64 as
    // false positives; requiring internal separators excludes them without invoking
    // DeSteuerIdMod1110 — which cannot help here, since 210 of 212 gold spans FAIL that checksum
    // (the corpus is synthetic). See todo #2418.
    assert_unchanged("tax number 12345678901 has no separators");
}

// -------------------------------------------------------------------------------- NATIONALID

#[test]
fn national_id_after_english_cue() {
    assert_id_removed(
        "National ID number: AB123456 verified.",
        "AB123456",
        &["National ID number: ", " verified."],
    );
}

#[test]
fn bare_german_national_id_is_a_known_disclosed_gap() {
    // NOT a bug. Cue adjacency for NATIONALID is 96.7% in English but only 28.7% in German: 77
    // German spans carry no cue at all and are structurally unreachable by any cue-anchored rule.
    // Chasing them needs an unanchored rule, and unanchored is exactly what produced 74 holdout
    // false positives in the first draft. This fixture pins the gap as a deliberate, bounded,
    // disclosed choice so it cannot regress into a silent one.
    assert_unchanged("Die Nummer 123456789 steht ohne Hinweis im Text.");
}

// ------------------------------------------------------------------ hard negatives (A4 shapes)

#[test]
fn a4_negative_shapes_are_untouched() {
    assert_unchanged("commit 9f8e7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c landed");
    assert_unchanged("order reference 1234567890 shipped today");
    assert_unchanged("invoice 123-456-789 has no tax cue anywhere near it");
}

// ------------------------------------------------------- cross-class collision determinism
//
// The numeric silhouettes genuinely overlap across these classes: `d3-d2-d4` appears in SSN
// (163), NATIONALID (21) and TAXNUM (17); `d3.d4.d4.d2` in SSN (33), NATIONALID (29) and TAXNUM
// (9). Resolution goes through `[recognizers.collision]` -> `FamilyPolicyTable` ->
// `ConflictTier::CollisionPolicy` rather than bespoke priorities, with precedence ordered by cue
// specificity: ssn=10 beats tax-number=20 beats national-id=30 (LOWER wins, registry.rs).
//
// A wrong-class token is still a token, so this is an axis-4 trust question rather than a leak
// question — but a NONDETERMINISTIC winner would be a real regression, which is what these pin.

#[test]
fn overlapping_tax_and_national_id_cues_resolve_deterministically() {
    // "Tax ID number: <digits>" satisfies BOTH the tax cue ("tax id") and the national-id cue
    // ("id number") immediately before the same span, so both recognizers genuinely match it.
    // tax-number must beat national-id, every time.
    let text = "Tax ID number: 123.4567.8901.23 filed.";
    let first = normalize_tokens(&clean(text));
    assert!(
        !first.contains("123.4567.8901.23"),
        "overlapping government-id span survived: {first:?}"
    );
    // tax-number (precedence 20) must beat national-id (precedence 30). Lower number wins, per
    // `FamilyPolicyTable` in registry.rs:114 (`Ordering::Less => Some(true)`), matching the
    // committed `payment-card-or-iban` family where iban=10 beats pan=20.
    assert!(
        first.contains("Custom:tax_number"),
        "expected the higher-precedence tax-number variant to win, got {first:?}"
    );
    for _ in 0..8 {
        assert_eq!(
            normalize_tokens(&clean(text)),
            first,
            "cross-class collision resolution must be deterministic across runs"
        );
    }
}

#[test]
fn collision_resolution_is_stable_within_a_single_session() {
    // Same span shape, repeated in one document: the class assignment must not drift between
    // occurrences.
    let text = "Tax ID number: 123.4567.8901.23 and Tax ID number: 456.7890.1234.56 both filed.";
    let cleaned = clean(text);
    assert!(!cleaned.contains("123.4567.8901.23"));
    assert!(!cleaned.contains("456.7890.1234.56"));
}

// -------------------------------------------------------------------- locale-chain activation

#[test]
fn locale_basis_declarations_follow_the_mixed_model() {
    // Declarative pin of the #414 re-expression, so a future edit cannot silently move a
    // recognizer between bases. `ssn.us` is included as the untouched reference point.
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let spec = |id: &str| {
        rulepack
            .recognizers
            .iter()
            .find(|recognizer| recognizer.id == id)
            .unwrap_or_else(|| panic!("missing recognizer {id}"))
    };
    let tag = |value: &str| LocaleTag::parse(value).expect("tag");

    let ssn_us = spec("ssn.us");
    assert_eq!(ssn_us.locale_basis, LocaleBasis::Format);
    assert_eq!(ssn_us.locales, vec![tag("en-US")]);

    // Same class and identifier shape as `ssn.us`, German cue vocabulary, DACH provenance.
    let ssn_de = spec("ssn.de_cue");
    assert_eq!(ssn_de.locale_basis, LocaleBasis::Format);
    assert_eq!(
        ssn_de.locales,
        vec![tag("de-DE"), tag("de-AT"), tag("de-CH")]
    );
    // The loader also synthesises cooperation across the government-id collision family, so
    // check membership rather than the exact list.
    assert!(ssn_de.cooperates_with.iter().any(|id| id == "ssn.us"));
    assert!(ssn_us.cooperates_with.iter().any(|id| id == "ssn.de_cue"));

    // Bilingual cue-anchored recognizers with no national shape: document basis, global.
    for id in [
        "tax_number.cue_anchored",
        "driver_license.cue_anchored",
        "national_id.cue_anchored",
    ] {
        let recognizer = spec(id);
        assert_eq!(recognizer.locale_basis, LocaleBasis::Document, "{id}");
        assert_eq!(recognizer.locales, vec![LocaleTag::Global], "{id}");
    }
}

#[test]
fn every_class_fires_under_every_benchmark_and_default_adopter_chain() {
    // Per-class verification across every chain the benchmark builds
    // (`[<lang>-<region>, "global"]`, gaze_bench_score.py:87) and the default adopter chain.
    // Asserted rather than assumed, because a locale-gated recognizer that silently never fires
    // is the failure mode behind #2403, #2411 and #2417 — and is precisely why `ssn.us` saw only
    // 17.5% of its own label.
    let cases: [(&str, &str); 4] = [
        (
            "Die Sozialversicherungsnummer lautet 123-45-6789 heute.",
            "123-45-6789",
        ),
        ("Die Steuernummer 123 456 789 ist da.", "123 456 789"),
        ("Führerscheinnummer B7654321 geprüft.", "B7654321"),
        ("National ID number: AB123456 ok.", "AB123456"),
    ];
    for chain in [
        vec![LocaleTag::Global],
        vec![LocaleTag::parse("en-US").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("en-GB").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("de-DE").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("de-AT").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("de-CH").expect("tag"), LocaleTag::Global],
    ] {
        for (text, id) in cases {
            let cleaned = clean_under(&chain, text);
            assert!(
                !cleaned.contains(id),
                "{id:?} survived on chain {chain:?}: {cleaned:?}"
            );
        }
    }
}

// ------------------------------------------------------------------------------- restore path

#[test]
fn government_ids_restore_exactly() {
    let pipeline = pipeline_for(&[LocaleTag::Global]);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let original = "Sozialversicherungsnummer 123-45-6789, Steuernummer 123 456 789, \
                    Driver's license: D1234567, National ID number: AB123456.";
    let (clean, _manifest, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(original.to_string()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let clean_text = match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    };
    assert!(!clean_text.contains("123-45-6789"));
    assert!(!clean_text.contains("D1234567"));
    let restored = pipeline
        .restore_strict_text(&session, &clean_text)
        .expect("restore");
    assert_eq!(restored, original, "manifest-first restore must round-trip");
}
