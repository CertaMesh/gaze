//! Nym-small safety net behaviour on real model output.
//!
//! `fixtures/nym_pieces.json` holds the tokenizer character offsets and per-piece scores the
//! pinned model produced for a handful of synthetic sentences (provenance inside the file). The
//! `captured_*` tests replay them through the production decoder, so the default test run checks
//! real model behaviour without the 150 MB bundle. The `live_*` tests are ignored by default and
//! need `GAZE_NYM_MODEL_DIR` pointing at a verified bundle; `cargo run -p xtask --
//! safety-net-sanity` runs them when that variable is set, and one of them proves the fixture is
//! still what the model produces.

use std::sync::OnceLock;

use gaze_recognizers::safety_net::nym::test_support::{
    capture, decode_captured, model_spans, DecodedSpan, PieceScore,
};
use gaze_recognizers::safety_net::nym::{NymConfig, NymLabel, NymOperatingPoint, NymSafetyNet};
use gaze_types::{DocumentKind, LeakKind, LocaleTag, Manifest, SafetyNet, SafetyNetContext};
use serde_json::{json, Value};

const FIXTURE: &str = include_str!("fixtures/nym_pieces.json");
const FIXTURE_PATH: &str = "tests/fixtures/nym_pieces.json";

/// The synthetic sentences the fixture was captured from, by case id.
fn case_texts() -> Vec<(&'static str, String)> {
    let filler =
        "Wir bestätigen den Eingang Ihrer Unterlagen und melden uns in den nächsten Tagen \
                  mit einer Rückmeldung zum weiteren Vorgehen. ";
    vec![
        (
            "plate-in-prose",
            "Das Fahrzeug mit dem Kennzeichen M-AB 1234 wurde abgeschleppt.".to_string(),
        ),
        (
            "username-en",
            "Please log in as jdoe_1977 and reset the password.".to_string(),
        ),
        (
            "dob-and-building-number",
            "Herr Beispiel, geb. 12.03.1985, wohnt in der Musterstraße 12a.".to_string(),
        ),
        (
            "salutation-4192",
            "Sehr geehrte Frau Müller, ich möchte Ihnen mitteilen, dass Ihr \
             Sicherheitszugangstoken X1E2-7KQ9-PLM4 gültig ist."
                .to_string(),
        ),
        (
            "room-number-known-gap",
            "Das Meeting findet in Raum 204 im 3. OG statt.".to_string(),
        ),
        (
            "multibyte",
            "🙂👍🏽 Grüße aus Überlingen: Kennzeichen B-XY 99E, Benutzer mu\u{308}ller_x9\u{a0}ok"
                .to_string(),
        ),
        (
            "long-document-tail",
            format!("{}Kennzeichen HH-XY 4711", filler.repeat(24)),
        ),
    ]
}

struct Case {
    text: String,
    offsets: Vec<(usize, usize)>,
    scores: Vec<PieceScore>,
}

fn fixture_case(id: &str) -> Case {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture is JSON");
    let case = fixture["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("fixture case {id}"));
    let text = case_texts()
        .into_iter()
        .find(|(case_id, _)| *case_id == id)
        .map(|(_, text)| text)
        .expect("case text");
    assert_eq!(
        case["text"].as_str(),
        Some(text.as_str()),
        "{id}: fixture text drifted"
    );
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
    Case {
        text,
        offsets,
        scores,
    }
}

/// Decodes a captured case: `(text, label, score)` per span, each checked to be word-aligned.
fn decode_case(id: &str, op: &NymOperatingPoint) -> Vec<(String, NymLabel, f32)> {
    let case = fixture_case(id);
    decode_captured(&case.text, &case.offsets, &case.scores, op)
        .expect("captured output decodes")
        .into_iter()
        .map(|(range, label, score)| {
            assert!(
                !gaze_types::is_inside_word(&case.text, range.start),
                "{id}: span starts inside a word"
            );
            assert!(
                !gaze_types::is_inside_word(&case.text, range.end),
                "{id}: span ends inside a word"
            );
            (case.text[range].to_string(), label, score)
        })
        .collect()
}

/// Spans under the shipped default operating point.
fn spans(id: &str) -> Vec<(String, NymLabel)> {
    decode_case(id, &NymOperatingPoint::default())
        .into_iter()
        .map(|(text, label, _)| (text, label))
        .collect()
}

fn owned(expected: &[(&str, NymLabel)]) -> Vec<(String, NymLabel)> {
    expected
        .iter()
        .map(|(text, label)| (text.to_string(), *label))
        .collect()
}

#[test]
fn captured_plate_in_prose_is_one_suspect() {
    assert_eq!(
        spans("plate-in-prose"),
        owned(&[("M-AB 1234", NymLabel::LicensePlate)])
    );
}

#[test]
fn captured_username_and_dob_shapes_are_suspects() {
    assert_eq!(
        spans("username-en"),
        owned(&[("jdoe_1977", NymLabel::Username)])
    );
    let decoded = decode_case("dob-and-building-number", &NymOperatingPoint::default());
    assert_eq!(
        decoded
            .iter()
            .map(|(text, label, _)| (text.as_str(), *label))
            .collect::<Vec<_>>(),
        vec![
            ("12.03.1985", NymLabel::DateOfBirth),
            ("12a", NymLabel::BuildingNumber)
        ]
    );
    // DATE_OF_BIRTH clears its 0.9 threshold; the Python probe measured 0.952 here.
    assert!((decoded[0].2 - 0.952).abs() < 0.001, "{}", decoded[0].2);
}

/// The 4192 sentence: a salutation, a surname and a credential. The model does see the surname
/// and the token (SURNAME and SSN pieces at >= 0.9); the default enables neither, so nothing is
/// flagged.
#[test]
fn captured_salutation_sentence_has_no_suspects() {
    let case = fixture_case("salutation-4192");
    let strongest = |label: NymLabel| {
        case.scores
            .iter()
            .filter(|score| score.label == label)
            .map(|score| score.mass)
            .fold(0.0f32, f32::max)
    };
    assert!(
        strongest(NymLabel::Surname) >= 0.9,
        "fixture no longer shows the surname"
    );
    assert!(
        strongest(NymLabel::Ssn) >= 0.9,
        "fixture no longer shows the token"
    );
    assert_eq!(spans("salutation-4192"), vec![]);
}

/// Known out-of-corpus false positive (probe §4): a room number reads as BUILDING_NUMBER. Pinned
/// so an address-context guard has to flip this assertion on purpose.
#[test]
fn captured_room_number_is_a_known_false_positive() {
    assert_eq!(
        spans("room-number-known-gap"),
        owned(&[("204", NymLabel::BuildingNumber)])
    );
}

#[test]
fn captured_multibyte_offsets_land_on_whole_words() {
    assert_eq!(
        spans("multibyte"),
        owned(&[
            ("B-XY 99E", NymLabel::LicensePlate),
            ("mu\u{308}ller_x9", NymLabel::Username),
        ])
    );
}

#[test]
fn captured_long_document_flags_the_tail() {
    let case = fixture_case("long-document-tail");
    assert!(
        case.offsets.len() > 512,
        "the case must span more than one model window"
    );
    assert_eq!(
        spans("long-document-tail"),
        owned(&[("HH-XY 4711", NymLabel::LicensePlate)])
    );
}

// ---- live: real pinned bundle ----

fn live_net() -> &'static NymSafetyNet {
    static NET: OnceLock<NymSafetyNet> = OnceLock::new();
    NET.get_or_init(|| {
        let net = NymSafetyNet::new(
            NymConfig::from_env().expect("set GAZE_NYM_MODEL_DIR to a verified nym bundle"),
        );
        net.preload().expect("pinned nym bundle loads");
        net
    })
}

fn live_check(text: &str) -> Vec<gaze_types::LeakSuspect> {
    let manifest = Manifest::default();
    let context = SafetyNetContext::new(
        &manifest,
        &[LocaleTag::Global],
        DocumentKind::Text,
        None,
        None,
    );
    live_net().check(text, context).expect("nym check")
}

fn case_text(id: &str) -> String {
    case_texts()
        .into_iter()
        .find(|(case_id, _)| *case_id == id)
        .map(|(_, text)| text)
        .expect("case text")
}

#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by xtask safety-net-sanity when set)"]
fn live_plate_in_prose_is_a_suspect() {
    let text = case_text("plate-in-prose");
    let suspects = live_check(&text);
    assert_eq!(suspects.len(), 1, "{suspects:?}");
    let suspect = &suspects[0];
    assert_eq!(&text[suspect.span.clone()], "M-AB 1234");
    assert_eq!(suspect.safety_net_id, "nym-small-int8");
    assert_eq!(suspect.raw_label, "LICENSE_PLATE>=0.5");
    assert_eq!(suspect.kind, LeakKind::Uncovered);
    assert!(suspect.score.is_some_and(|score| score >= 0.5));
}

#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by xtask safety-net-sanity when set)"]
fn live_salutation_sentence_has_no_suspects() {
    assert_eq!(live_check(&case_text("salutation-4192")), vec![]);
}

#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by xtask safety-net-sanity when set)"]
fn live_long_document_is_scanned_to_the_end() {
    let text = case_text("long-document-tail");
    let suspects = live_check(&text);
    assert_eq!(
        suspects
            .iter()
            .map(|suspect| &text[suspect.span.clone()])
            .collect::<Vec<_>>(),
        vec!["HH-XY 4711"]
    );
}

/// Re-captures every case and compares it with the committed fixture, so the `captured_*` tests
/// keep testing what the pinned model really outputs. With `GAZE_NYM_WRITE_FIXTURE` set it
/// rewrites the fixture instead.
#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by xtask safety-net-sanity when set)"]
fn live_capture_matches_the_committed_fixture() {
    let fresh = capture_all();
    if std::env::var_os("GAZE_NYM_WRITE_FIXTURE").is_some() {
        std::fs::write(FIXTURE_PATH, serde_json::to_string(&fresh).unwrap() + "\n").unwrap();
        return;
    }
    let committed: Value = serde_json::from_str(FIXTURE).unwrap();
    let fresh_cases = fresh["cases"].as_array().unwrap();
    let committed_cases = committed["cases"].as_array().unwrap();
    assert_eq!(fresh_cases.len(), committed_cases.len());
    for (fresh, committed) in fresh_cases.iter().zip(committed_cases) {
        let id = &committed["id"];
        assert_eq!(fresh["text"], committed["text"], "{id}");
        assert_eq!(fresh["offsets"], committed["offsets"], "{id}: offsets");
        let fresh_scores = fresh["scores"].as_array().unwrap();
        let committed_scores = committed["scores"].as_array().unwrap();
        assert_eq!(fresh_scores.len(), committed_scores.len(), "{id}");
        for (a, b) in fresh_scores.iter().zip(committed_scores) {
            assert_eq!(a[0], b[0], "{id}: label");
            assert_eq!(a[2], b[2], "{id}: begin");
            assert!(
                (a[1].as_f64().unwrap() - b[1].as_f64().unwrap()).abs() < 1e-4,
                "{id}: mass {a} vs {b}"
            );
        }
    }
}

fn capture_all() -> Value {
    let cases = case_texts()
        .into_iter()
        .map(|(id, text)| {
            let (offsets, scores) = capture(live_net(), &text).expect("capture");
            json!({
                "id": id,
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
            "generator": "GAZE_NYM_WRITE_FIXTURE=1 cargo test -p gaze-recognizers --features safety-net-nym,test-support --test nym_safety_net -- --ignored live_capture_matches_the_committed_fixture",
            "offsets": "tokenizer character offsets (encode_char_offsets, no special tokens)",
            "scores": "[label with the largest B+I mass, that mass, P(B) >= P(I)] per piece",
            "texts": "synthetic, fictional"
        },
        "cases": cases,
    })
}

/// Todo 3681 on the real model, on the model's own spans (before manifest correlation, which
/// would drop a same-class flag inside a token and hide whether the model read it). Unmasked, the
/// pinned model flags Gaze's own token text because the class name spells a Nym label; with the
/// tokens in the manifest they are masked before inference and no span overlaps them. The first
/// half keeps the fixture honest: if the model stopped flagging token text, the second half would
/// pass without proving anything.
#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by xtask safety-net-sanity when set)"]
fn live_token_text_is_masked_before_the_model_reads_it() {
    let tokens = [
        "<e19efc64:Custom:building_number_1>",
        "<e19efc64:Custom:license_plate_1>",
    ];
    let text = format!(
        "Die Lieferung geht an die Musterstraße {} in Berlin, Fahrzeug {} steht im Hof.",
        tokens[0], tokens[1]
    );
    let token_spans = tokens
        .iter()
        .map(|token| {
            let start = text.find(token).unwrap();
            start..start + token.len()
        })
        .collect::<Vec<_>>();
    let overlapping = |spans: &[DecodedSpan]| {
        spans
            .iter()
            .filter(|(span, _, _)| {
                token_spans
                    .iter()
                    .any(|token| span.start < token.end && token.start < span.end)
            })
            .count()
    };

    let unmasked = model_spans(live_net(), &text, &Manifest::default()).expect("nym spans");
    assert!(
        overlapping(&unmasked) > 0,
        "fixture no longer exercises token text: {unmasked:?}"
    );

    let manifest = Manifest::from_spans(
        token_spans
            .iter()
            .zip(["building_number", "license_plate"])
            .map(|(span, class)| {
                gaze_types::EmittedTokenSpan::new(
                    span.clone(),
                    0..1,
                    gaze_types::PiiClass::custom(class).unwrap(),
                )
            })
            .collect(),
    );
    let masked = model_spans(live_net(), &text, &manifest).expect("nym spans");
    assert_eq!(overlapping(&masked), 0, "{masked:?}");
}
