//! Regression fixtures for the `iban.structural` candidate span (solo todo #3708).
//!
//! Before this change the pattern was:
//!
//! ```text
//! \b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7} ?[A-Z0-9]{1,4}\b
//! ```
//!
//! Both the repeated four-character group and the mandatory one-to-four character tail accept an
//! optional LEADING space, so an upper-case or digit word following the IBAN could be absorbed
//! into the candidate: as a whole group (` SWIF` + tail `T`) or as the tail alone (` BIC`). The
//! over-long candidate then failed `iban_mod97`, which gates on the country's registry length, so
//! validator veto dropped it and the IBAN shipped RAW. Where the digits happened to be Luhn-valid,
//! `card.structural` claimed them and the IBAN was tokenized as `custom:credit_card` with the
//! country code and check digits still raw in front of it.
//!
//! Measured on the shipped binary at main `e9c266cc`, `gaze clean --rulepack-bundled
//! core,locale-de --locale de-DE`:
//!
//! | input                                        | base output                                        |
//! |----------------------------------------------|----------------------------------------------------|
//! | `IBAN AT61 1904 3002 3457 3201 BIC: BKAUATWW`| `IBAN AT61 <..:Custom:credit_card_1> BIC: BKAUATWW`|
//! | `IBAN BE62 6589 3795 9627 SWIFT GEBABEBB`    | unchanged, `detections: 0`                          |
//! | `IBAN LU88 9379 3020 6543 9565 EUR`          | unchanged, `detections: 0`                          |
//! | `IBAN HU39 8260 7742 8582 3842 6735 2153 OK` | unchanged, `detections: 0`                          |
//!
//! `IBAN ... BIC: ...` is the standard European invoice and e-mail footer layout, so this is an
//! ordinary shape rather than an adversarial one.
//!
//! The fix carries one alternation branch per ISO 13616 registry length, so the candidate stops at
//! the country's real IBAN length. That is a strict NARROWING of the matched language. It costs no
//! recall, because `iban_mod97` already rejects every candidate whose canonical length is not
//! `gaze_types::iban_registry_length(country)` — everything the new pattern declines to match could
//! never have produced a token. `iban_pattern_branch_lengths_match_the_validator_registry` below
//! pins the two tables to each other so they cannot drift apart silently.
//!
//! IBAN fixture values are synthetic: every one is generated here from a seeded alphanumeric BBAN
//! and a computed mod-97 check, so they are format-valid and checksum-valid but address no real
//! account. `AT61 1904 3002 3457 3201` and `DE89 3704 0044 0532 0130 00` are the published ISO
//! 13616 / Wikipedia documentation examples.

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    RawDocument, RawMatch, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;
use gaze_types::iban_registry_length;
use std::collections::BTreeMap;
use std::sync::OnceLock;

// ============================================================================ harness

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

fn custom(class: &str) -> PiiClass {
    PiiClass::custom(class).expect("valid custom class")
}

/// The core bundle through the real activation path.
///
/// `custom:credit_card` is tokenized alongside `custom:iban` on purpose: the defect's worst shape
/// mis-classes the IBAN digits as a card, and a policy that preserved cards would hide that by
/// leaving the digits raw either way.
fn pipeline_for(locales: &[LocaleTag]) -> Pipeline {
    // `core` alone is not enough: `iban.structural` declares `mandatory_anchor = "iban"`, and the
    // `[locale.cues.iban]` bucket that satisfies it lives in the locale packs. Without them every
    // IBAN fails closed to a family-level token, which would make these fixtures pass for the
    // wrong reason. This is the same `core,locale-de,locale-en` wiring the CLI repro used.
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
    let chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(locales));
    gaze_assembly::build_pipeline(&policy, &empty_context(), &rulepacks, &chain, None)
        .expect("pipeline")
}

/// One pipeline for the whole file. Building it per document dominated the runtime of the
/// every-country fixture, which cleans several thousand documents.
fn shared_pipeline() -> &'static Pipeline {
    static PIPELINE: OnceLock<Pipeline> = OnceLock::new();
    PIPELINE.get_or_init(|| pipeline_for(LOCALES))
}

const LOCALES: &[LocaleTag] = &[LocaleTag::DeDe, LocaleTag::Global];

fn clean(text: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = shared_pipeline()
        .clean_with_safety_net_detect_context(
            &session,
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

/// The cleaned text with every emitted token replaced by a single NUL.
const TOKEN_BLANK: &str = "\u{0}";

fn clean_without_tokens(text: &str) -> String {
    gaze::token_shape::pattern()
        .replace_all(&clean(text), TOKEN_BLANK)
        .into_owned()
}

/// Asserts `prefix + iban + trailer` cleans to exactly `prefix + <one iban token> + trailer`.
///
/// This is an EQUALITY assertion on the token-blanked output, not a set of `contains` checks, and
/// that is load-bearing in both directions. A `contains` check for the IBAN string is too weak:
/// the shipped defect left `AT61 ` raw in front of a `credit_card` token, so the full IBAN string
/// was indeed absent while its country code and check digits leaked. A `contains` check for each
/// IBAN group is too strong: a one-character group such as RU's trailing `1` also occurs inside
/// the emitted token (`...:Custom:iban_1>`) and inside the literal `IBAN` cue in the prefix, so it
/// reports leaks that are not there. Equality on the blanked output has neither failure mode.
fn assert_iban_tokenized(prefix: &str, iban: &str, trailer: &str) {
    let text = format!("{prefix}{iban}{trailer}");
    let cleaned = clean(&text);
    let fixture = fixture_label(prefix, iban, trailer);
    assert!(
        !cleaned.contains(":Custom:credit_card_"),
        "{fixture} must not be claimed by card.structural: {}",
        shape_of(&cleaned)
    );
    assert_eq!(
        cleaned.matches(":Custom:iban_").count(),
        1,
        "expected exactly one custom:iban token for {fixture} in {}",
        shape_of(&cleaned)
    );
    assert_eq!(
        clean_without_tokens(&text),
        format!("{prefix}{TOKEN_BLANK}{trailer}"),
        "the IBAN token must cover the IBAN exactly, nothing more and nothing less: {fixture} -> {}",
        shape_of(&cleaned)
    );
}

/// Names a fixture without printing its IBAN: country, registry length, spaced or compact, and
/// the raw prefix and trailer (which carry no PII).
///
/// The values are synthetic and reproducible from the seed, so a failure needs only this to be
/// re-run. The assert messages print nothing else about the value: a panic message is test
/// output that gets pasted into issues and CI logs, and the same rule that keeps real IBANs
/// out of logs is applied to these by CodeQL's cleartext-logging query.
fn fixture_label(prefix: &str, iban: &str, trailer: &str) -> String {
    let compact: String = iban.chars().filter(|ch| !ch.is_whitespace()).collect();
    let shape = if compact.len() == iban.len() {
        "compact"
    } else {
        "spaced"
    };
    format!(
        "{}/len {}/{shape}/prefix {prefix:?}/trailer {trailer:?}",
        &compact[..2],
        compact.len()
    )
}

/// The cleaned text with every token replaced by `<token>` and every run of five or more
/// alphanumerics replaced by `<alnum×N>`, so a failing assert shows structure but no value.
fn shape_of(cleaned: &str) -> String {
    let blanked = gaze::token_shape::pattern().replace_all(cleaned, "<token>");
    regex::Regex::new(r"[A-Za-z0-9]{5,}")
        .expect("alnum run regex")
        .replace_all(&blanked, |caps: &regex::Captures<'_>| {
            format!("<alnum×{}>", caps[0].len())
        })
        .into_owned()
}

// ============================================================================ IBAN generation

/// mod-97-10 check digits for `country` + BBAN, per ISO 7064.
fn check_digits(country: &str, bban: &str) -> u32 {
    let rearranged = format!("{bban}{country}00");
    let mut remainder = 0u32;
    for byte in rearranged.bytes() {
        let value = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'A'..=b'Z' => u32::from(byte - b'A') + 10,
            other => panic!("non-alphanumeric byte {other} in {rearranged}"),
        };
        remainder = if value > 9 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    98 - remainder
}

/// A deterministic, checksum-valid IBAN for `country`.
///
/// The BBAN is alphanumeric for every country. That is wider than some national formats allow, but
/// `iban.structural`'s character class is `[A-Z0-9]` for every branch, so an alphanumeric BBAN
/// exercises exactly the shape the pattern accepts.
fn synthetic_iban(country: &str, seed: u64) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let length = iban_registry_length(country).expect("registry country");
    let mut state = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(u64::from(country.as_bytes()[0]) << 8)
        .wrapping_add(u64::from(country.as_bytes()[1]));
    let bban: String = (0..length - 4)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            char::from(ALPHABET[((state >> 33) % ALPHABET.len() as u64) as usize])
        })
        .collect();
    format!("{country}{:02}{bban}", check_digits(country, &bban))
}

fn spaced(compact: &str) -> String {
    compact
        .as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).expect("ascii"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn every_registry_country() -> Vec<String> {
    let mut countries = Vec::new();
    for first in b'A'..=b'Z' {
        for second in b'A'..=b'Z' {
            let code = String::from_utf8(vec![first, second]).expect("ascii");
            if iban_registry_length(&code).is_some() {
                countries.push(code);
            }
        }
    }
    countries
}

// ============================================================================ the defect

/// Both shipped outcome classes, per affected country, as MEASURED on main `e9c266cc`.
///
/// The defect had two distinct outcomes and which one an adopter got depended on whether the
/// IBAN's BBAN happened to be Luhn-valid as a card number, so a fixture set that covered only one
/// of them would leave half the class unpinned:
///
/// * WHOLE-IBAN-RAW — the over-long candidate is vetoed and nothing else claims the span, so the
///   entire IBAN ships raw with `detections: 0`, an empty leak report and a success exit. This is
///   the only outcome available to BE, whose 12-digit BBAN is below `card.structural`'s 13-digit
///   floor, and it is what every alphanumeric BBAN produced.
/// * COUNTRY-CODE-AND-CHECK-DIGIT PREFIX LEAK — the digits are Luhn-valid, so `card.structural`
///   claims them as `custom:credit_card` and the leading `CC99 ` is left raw beside the token.
///   The leaked prefix is 5 bytes for a 20-character IBAN and grows with length, because the card
///   run starts at the first group boundary after the check digits.
///
/// The `base` column is the literal `clean_text` from
/// `gaze clean --rulepack-bundled core,locale-de --locale de-DE` at main `e9c266cc`:
///
/// | country | length | base outcome                    | raw bytes |
/// |---------|-------:|---------------------------------|----------:|
/// | `BE`    |     16 | whole-IBAN-raw                  |        16 |
/// | `AT`    |     20 | prefix leak + `credit_card`     |         5 |
/// | `EE`    |     20 | prefix leak + `credit_card`     |         5 |
/// | `LT`    |     20 | prefix leak + `credit_card`     |         5 |
/// | `LU`    |     20 | prefix leak + `credit_card`     |         5 |
/// | `CZ`    |     24 | prefix leak + `credit_card`     |        10 |
/// | `PL`    |     28 | prefix leak + `credit_card`     |        15 |
/// | `HU`    |     28 | prefix leak + `credit_card`     |        15 |
/// | `LC`    |     32 | prefix leak + `credit_card`     |        20 |
///
/// Every value below is synthetic: the BBAN is seeded and the check digits are computed, so each
/// is checksum-valid but addresses no real account.
#[test]
fn both_shipped_outcome_classes_are_fixed_for_every_affected_country() {
    // Luhn-valid digit BBANs: these produced the `credit_card` prefix leak on base.
    for iban in [
        "AT75 9174 0029 7550 4736",
        "EE84 9174 0029 7550 4736",
        "LT73 9174 0029 7550 4736",
        "LU70 9174 0029 7550 4736",
        "CZ97 9174 0029 7550 4736 8813",
        "PL23 9174 0029 7550 4736 8813 9849",
        "HU68 9174 0029 7550 4736 8813 9849",
        "LC87 9174 0029 7550 4736 8813 9849 1651",
    ] {
        assert_iban_tokenized("IBAN ", iban, " BIC: BKAUATWW");
    }
    // Digit BBANs that are NOT Luhn-valid, and alphanumeric BBANs: these shipped the whole IBAN
    // raw on base, with no detection and no error.
    for iban in [
        "BE48 6604 8764 7593",
        "BE90 OQ2G WVPJ UMDW",
        "AT45 6604 8764 7593 8242",
        "AT56 OQ2G WVPJ UMDW 8I86",
        "EE54 6604 8764 7593 8242",
        "LT43 6604 8764 7593 8242",
        "LU40 6604 8764 7593 8242",
        "CZ09 6604 8764 7593 8242 1948",
        "PL30 6604 8764 7593 8242 1948 9241",
        "HU75 6604 8764 7593 8242 1948 9241",
        "LC84 6604 8764 7593 8242 1948 9241 1578",
        "LC89 OQ2G WVPJ UMDW 8I86 GY9J 64LU Z6MR",
    ] {
        assert_iban_tokenized("IBAN ", iban, " BIC: BKAUATWW");
    }
}

/// The exact shipped repro from todo #3708, as an AT IBAN is written on an invoice.
#[test]
fn published_at_iban_before_a_bic_label_tokenizes_whole() {
    assert_iban_tokenized("IBAN ", "AT61 1904 3002 3457 3201", " BIC: BKAUATWW");
}

/// The absorbable trailing shapes, across the length-multiple-of-four countries.
///
/// Those countries are the ones the old pattern could not express without borrowing: their IBAN is
/// an exact number of four-character groups, so the mandatory one-to-four character tail had to
/// come from the next word.
#[test]
fn upper_case_word_after_a_multiple_of_four_iban_stays_outside_the_candidate() {
    // Every one of these lengths is 0 mod 4: AT/BE/LU/EE/LT 20 or 16, PL/HU 28, LC 32, CZ 24.
    for country in ["AT", "BE", "LU", "EE", "LT", "PL", "HU", "CZ", "LC"] {
        let compact = synthetic_iban(country, 3708);
        assert_eq!(
            iban_registry_length(country).expect("registry country") % 4,
            0,
            "{country} must be a multiple-of-four length for this fixture"
        );
        for shape in [spaced(&compact), compact.clone()] {
            for trailer in [
                " BIC",
                " BIC: BKAUATWW",
                " SWIFT",
                " EUR",
                " OK",
                " A",
                " 1234",
                " und",
                " Bank PKO",
                ".",
                "",
            ] {
                assert_iban_tokenized("IBAN ", &shape, trailer);
            }
            // A line break between the IBAN and the next word, the e-mail footer shape.
            assert_iban_tokenized("IBAN ", &shape, "\nBIC: BKAUATWW");
        }
    }
}

/// Countries whose length is NOT a multiple of four were already correct; they must stay correct.
///
/// `DE89 ... 0130 00` is the control the todo named: its final group is two characters, so the old
/// pattern's tail was satisfied inside the IBAN and never reached for the next word.
#[test]
fn non_multiple_of_four_ibans_are_unchanged_by_the_length_branches() {
    assert_iban_tokenized("IBAN ", "DE89 3704 0044 0532 0130 00", " BIC: COBADEFF");
    assert_iban_tokenized("IBAN ", "GB82 WEST 1234 5698 7654 32", " BIC");
    for country in [
        "NO", "MK", "CH", "GB", "AE", "PT", "IS", "FR", "BR", "MT", "RU",
    ] {
        let compact = synthetic_iban(country, 3708);
        assert_ne!(
            iban_registry_length(country).expect("registry country") % 4,
            0,
            "{country} must NOT be a multiple-of-four length for this fixture"
        );
        for shape in [spaced(&compact), compact.clone()] {
            for trailer in [" BIC", " SWIFT", " EUR", ""] {
                assert_iban_tokenized("IBAN ", &shape, trailer);
            }
        }
    }
}

// ============================================================================ recall

/// Every ISO 13616 registry country still tokenizes whole, bare and before a `BIC` label.
///
/// This is the recall half of the narrowing: the new pattern matches only registry lengths, so a
/// country missing from a length branch would silently lose ALL protection. Driving every country
/// in `gaze_types::iban_registry_length` makes that a test failure rather than a quiet leak.
#[test]
fn every_registry_country_tokenizes_whole_with_and_without_a_trailing_label() {
    let countries = every_registry_country();
    assert_eq!(
        countries.len(),
        89,
        "ISO 13616 registry country count changed; re-derive the pattern length branches"
    );
    for country in countries {
        for seed in [1u64, 2, 3] {
            let compact = synthetic_iban(&country, seed);
            assert_eq!(
                compact.len(),
                iban_registry_length(&country).expect("registry country"),
                "generated {country} seed {seed} IBAN has the wrong length"
            );
            for shape in [spaced(&compact), compact.clone()] {
                assert_iban_tokenized("IBAN ", &shape, "");
                assert_iban_tokenized("IBAN ", &shape, " BIC");
            }
        }
    }
}

/// A country code outside the registry produces no candidate at all.
///
/// On base it produced a candidate that validator veto then dropped. Dropping it earlier is the
/// intended behaviour: `iban_registry_length` returns `None`, so no such string could ever have
/// become a token.
#[test]
fn non_registry_country_codes_never_tokenize() {
    for code in ["ZZ", "QQ", "XX"] {
        assert!(
            iban_registry_length(code).is_none(),
            "{code} must stay outside the registry for this fixture"
        );
        let text = format!("IBAN {code}61 1904 3002 3457 3201");
        let cleaned = clean(&text);
        assert!(
            !cleaned.contains(":Custom:iban_"),
            "{code} is not a registry country and must not produce an IBAN token: {}",
            shape_of(&cleaned)
        );
    }
}

// ============================================================================ drift

/// The pattern's country -> length map must equal `gaze_types::iban_registry_length`.
///
/// Two tables now encode IBAN length: the validator's and the recognizer pattern's. If the
/// validator gains a country the pattern does not, that country silently loses all detection; if
/// the pattern gains one the validator does not, its candidates are vetoed and the IBAN ships raw.
/// Either direction is an axis-1 leak, so both are compared here over the whole ISO 3166-1 alpha-2
/// space rather than over a hand-written list that could itself drift.
#[test]
fn iban_pattern_branch_lengths_match_the_validator_registry() {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let spec = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "iban.structural")
        .expect("iban.structural");
    let RawMatch::Regex {
        pattern: Some(pattern),
        ..
    } = &spec.matcher
    else {
        panic!("iban.structural must be a plain regex recognizer");
    };

    // `(?:AT|BA|...)\d{2}(?:\x20?[A-Z0-9]{4}){4}` optionally followed by `\x20?[A-Z0-9]{3}`.
    let branch = regex::Regex::new(
        r"\(\?:([A-Z|]+)\)\\d\{2\}\(\?:\\x20\?\[A-Z0-9\]\{4\}\)\{(\d+)\}(?:\\x20\?\[A-Z0-9\]\{(\d+)\})?",
    )
    .expect("branch regex");

    let mut from_pattern: BTreeMap<String, usize> = BTreeMap::new();
    for captures in branch.captures_iter(pattern) {
        let groups: usize = captures[2].parse().expect("group count");
        let remainder: usize = captures
            .get(3)
            .map_or(0, |m| m.as_str().parse().expect("remainder"));
        let length = 4 + groups * 4 + remainder;
        for country in captures[1].split('|') {
            assert!(
                from_pattern.insert(country.to_string(), length).is_none(),
                "country {country} appears in more than one length branch"
            );
        }
    }

    let mut from_validator: BTreeMap<String, usize> = BTreeMap::new();
    for country in every_registry_country() {
        from_validator.insert(
            country.clone(),
            iban_registry_length(&country).expect("registry country"),
        );
    }

    assert_eq!(
        from_pattern, from_validator,
        "iban.structural length branches and gaze_types::iban_registry_length disagree"
    );
}

/// The pattern must not regain an open-ended trailing group.
///
/// The defect was `{2,7}` repetitions plus a `{1,4}` tail: both ranges let the candidate run past
/// the IBAN. Every quantifier in the pattern is now exact, and this is the mutation the fix has to
/// fail on.
#[test]
fn iban_pattern_has_no_open_ended_quantifier() {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let spec = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "iban.structural")
        .expect("iban.structural");
    let RawMatch::Regex {
        pattern: Some(pattern),
        ..
    } = &spec.matcher
    else {
        panic!("iban.structural must be a plain regex recognizer");
    };
    let ranged = regex::Regex::new(r"\{\d+,\d*\}").expect("range regex");
    assert!(
        !ranged.is_match(pattern),
        "iban.structural must use exact repetition counts only, found {:?} in {pattern}",
        ranged.find(pattern).map(|m| m.as_str())
    );
}

// ============================================================================ glued boundary (#3756)

/// A label glued directly to a compact IBAN stays outside the token; the IBAN tokenizes whole.
///
/// Solo todo #3756. The pattern used to end in `\b`, so a candidate immediately followed by a
/// letter or digit was never a candidate at all, and `IBAN AT611904300234573201BIC` shipped raw
/// with `detections: 0`, an empty leak report and a success exit, in every release since
/// v0.4.3-rc.1. The shape is ordinary machine output and dense footers
/// (`IBAN:<value>BIC:<value>`). Rust `regex` has no lookahead, so the trailing boundary now lives
/// in code: `gaze_types::word_run_extends_identifier` accepts a validated registry-length
/// candidate when the word run after it is empty or letters only (a glued label or word), and
/// rejects it when the run holds a digit or an underscore (it could be more identifier).
#[test]
fn label_glued_to_a_compact_iban_stays_outside_the_token() {
    // The shipped repros, verbatim.
    assert_iban_tokenized("IBAN ", "AT611904300234573201", "BIC");
    assert_iban_tokenized("IBAN:", "AT611904300234573201", "BIC:BKAUATWW");
    for iban in [
        "AT611904300234573201",
        "AT61 1904 3002 3457 3201",
        "DE89370400440532013000",
        "DE89 3704 0044 0532 0130 00",
    ] {
        for prefix in ["IBAN ", "IBAN:", "IBAN: "] {
            for trailer in [
                "BIC",
                "BIC:BKAUATWW",
                "BICBKAUATWW",
                "SWIFT",
                "EUR",
                "OK",
                "Bank",
                "bic",
                "und",
                "BIC\nBKAUATWW",
            ] {
                assert_iban_tokenized(prefix, iban, trailer);
            }
        }
    }
    // Every registry country, compact, glued to the two footer labels.
    for country in every_registry_country() {
        let compact = synthetic_iban(&country, 3756);
        assert_iban_tokenized("IBAN ", &compact, "BIC");
        assert_iban_tokenized("IBAN:", &compact, "BIC:BKAUATWW");
    }
}

/// `phone.national.de` opens with a no-capture branch that consumes a 22-character IBAN grouping
/// so its phone branches never see `0532 0130` inside a German IBAN. That branch used to end in
/// `\b` as well, so a glued label switched it off, the phone rule (priority 85) claimed the
/// inner run, and the IBAN token was fragmented around a phone token (or, with `custom:phone`
/// preserved as here, the whole IBAN stayed raw). This is the fixture that reddens when that
/// trailing `\b` comes back.
#[test]
fn german_phone_rule_does_not_fragment_a_spaced_iban_glued_to_a_label() {
    for trailer in ["BIC", "BIC:COBADEFF", "EUR", "Bank"] {
        assert_iban_tokenized("IBAN ", "DE89 3704 0044 0532 0130 00", trailer);
        assert_iban_tokenized("IBAN:", "DE89 3704 0044 0532 0130 00", trailer);
    }
}

/// "Letters only" is Unicode `char::is_alphabetic`, not ASCII: a German word glued to the IBAN
/// behaves like an English one.
///
/// This is the fixture that reddens when the run test is narrowed to `is_ascii_alphabetic`.
#[test]
fn non_ascii_letters_glued_to_an_iban_are_a_word_not_more_identifier() {
    for trailer in ["Überweisung", "über", "ÄrgerBIC", "Straße", "élan"] {
        assert_iban_tokenized("IBAN ", "AT611904300234573201", trailer);
        assert_iban_tokenized("IBAN ", "DE89 3704 0044 0532 0130 00", trailer);
    }
}

/// An IBAN-shaped, checksum-valid prefix of a longer identifier is not an IBAN.
///
/// The other direction of the boundary, and the one the old `\b` guarded: with the boundary gone
/// from the pattern, the exact-length branches happily match a valid prefix of an opaque token
/// and `iban_mod97` accepts it. A digit, an underscore or a non-ASCII digit anywhere in the word
/// run after the candidate means the run could be more identifier, so the candidate is dropped
/// before validation: no `custom:iban` token, no partial token. The no-cue `ref … end` shapes are
/// the ones todo #3756 measured; the `IBAN …` shapes show the cue does not override the boundary.
/// This is the fixture that reddens when the code boundary is dropped.
#[test]
fn iban_shaped_prefix_of_a_longer_identifier_is_not_tokenized() {
    for text in [
        "ref AT611904300234573201XQ7 end",
        "ref AT6119043002345732019 end",
        "ref AT611904300234573201BIC1 end",
        "IBAN AT611904300234573201XQ7",
        "IBAN AT6119043002345732011234",
        "IBAN AT611904300234573201_x",
        "IBAN AT611904300234573201_",
        "IBAN AT611904300234573201\u{661}",
        "IBAN DE89370400440532013000ABC1",
    ] {
        let cleaned = clean(text);
        assert_eq!(
            cleaned,
            text,
            "an IBAN-shaped prefix of a longer identifier must produce no token at all: {}",
            shape_of(&cleaned)
        );
    }
    // Spaced form with digits glued to the last group. `card.structural` may still claim a
    // Luhn-valid digit run inside it (pre-existing, not this boundary), so only the IBAN half is
    // asserted here.
    for text in [
        "IBAN AT61 1904 3002 3457 32011234",
        "IBAN AT61 1904 3002 3457 3201_1",
    ] {
        let cleaned = clean(text);
        assert!(
            !cleaned.contains(":Custom:iban_"),
            "digits glued to the last group must not yield an IBAN token: {}",
            shape_of(&cleaned)
        );
    }
}

/// The trailing boundary must stay out of the pattern.
///
/// A trailing `\b` is the mutation that silently re-opens the glued-label leak while every
/// space-separated fixture stays green, so it is pinned here structurally as well as by the
/// glued fixtures above. The leading `\b` is required: matches must start at a word start.
#[test]
fn iban_pattern_keeps_the_leading_boundary_and_has_no_trailing_one() {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let spec = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "iban.structural")
        .expect("iban.structural");
    let RawMatch::Regex {
        pattern: Some(pattern),
        ..
    } = &spec.matcher
    else {
        panic!("iban.structural must be a plain regex recognizer");
    };
    let body = pattern.trim();
    assert!(
        body.starts_with(r"(?x)\b("),
        "iban.structural must start at a word boundary: {body:?}"
    );
    assert!(
        !body.ends_with(r"\b"),
        "iban.structural must not end in a word boundary; the trailing boundary is decided in code \
         by gaze_types::word_run_extends_identifier: {body:?}"
    );
}
