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
    capture, check_captured, decode_captured, PieceScore,
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
fn person_name_parts_decode_as_one_word_aligned_span_in_every_case_and_surface() {
    let op =
        NymOperatingPoint::new([(NymLabel::GivenName, 0.7), (NymLabel::Surname, 0.9)]).unwrap();
    for name in [
        "Nelia Quillmere",
        "nelia quillmere",
        "NELIA QUILLMERE",
        "Élia Liora Briselune",
        "élia\u{a0}liora\u{202f}briselune",
    ] {
        for text in [
            format!("Gru\u{308}ße 🙂 {name} requested an update."),
            format!("user: please contact {name} tomorrow."),
            format!(r#"{{"tool":"notify","arguments":{{"recipient_name":"{name}"}}}}"#),
        ] {
            let start = text.find(name).unwrap();
            let end = start + name.len();
            let offsets = (0..text.chars().count())
                .map(|index| (index, index + 1))
                .collect::<Vec<_>>();
            let mut scores = vec![
                PieceScore {
                    label: NymLabel::Age,
                    mass: 0.0,
                    is_begin: true,
                };
                offsets.len()
            ];
            let mut cursor = start;
            let parts = name.split_whitespace().collect::<Vec<_>>();
            for (index, part) in parts.iter().enumerate() {
                let word_start = cursor + text[cursor..end].find(part).unwrap();
                let word_end = word_start + part.len();
                // Only an interior character is labelled. The production word rule must
                // expand this model finding without cutting UTF-8 or a combining sequence.
                let piece = text[..word_start].chars().count() + 1;
                let surname = index + 1 == parts.len();
                scores[piece] = PieceScore {
                    label: if surname {
                        NymLabel::Surname
                    } else {
                        NymLabel::GivenName
                    },
                    mass: if surname { 0.96 } else { 0.97 },
                    is_begin: true,
                };
                cursor = word_end;
            }
            let decoded = decode_captured(&text, &offsets, &scores, &op).unwrap();
            assert_eq!(
                decoded,
                vec![(start..end, NymLabel::GivenName, 0.96)],
                "{text}"
            );
            assert!(!gaze_types::is_inside_word(&text, start));
            assert!(!gaze_types::is_inside_word(&text, end));
        }
    }
}

fn name_cases() -> Vec<(Value, Case)> {
    let inputs: Value =
        serde_json::from_str(include_str!("fixtures/nym_name_calibration.json")).unwrap();
    let captures: Value =
        serde_json::from_str(include_str!("fixtures/nym_name_pieces.json")).unwrap();
    inputs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|input| {
            let captured = captures["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == input["id"])
                .unwrap();
            (
                input.clone(),
                Case {
                    text: input["text"].as_str().unwrap().to_string(),
                    offsets: captured["offsets"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|pair| {
                            (
                                pair[0].as_u64().unwrap() as usize,
                                pair[1].as_u64().unwrap() as usize,
                            )
                        })
                        .collect(),
                    scores: captured["scores"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|row| PieceScore {
                            label: NymLabel::parse(row[0].as_str().unwrap()).unwrap(),
                            mass: row[1].as_f64().unwrap() as f32,
                            is_begin: row[2].as_bool().unwrap(),
                        })
                        .collect(),
                },
            )
        })
        .collect()
}

#[test]
fn person_name_join_respects_record_and_json_boundaries() {
    let op =
        NymOperatingPoint::new([(NymLabel::GivenName, 0.5), (NymLabel::Surname, 0.5)]).unwrap();
    for separator in [
        "\n",
        "\r\n",
        "\u{2028}",
        ", ",
        " and ",
        "\",\"",
        "<deadbeef:Name_1>",
    ] {
        let text = format!("nelia{separator}quillmere");
        let end = text.chars().count();
        let surname = end - "quillmere".len();
        let offsets = (0..end).map(|i| (i, i + 1)).collect::<Vec<_>>();
        let mut scores = vec![
            PieceScore {
                label: NymLabel::Age,
                mass: 0.0,
                is_begin: true
            };
            end
        ];
        scores[1] = PieceScore {
            label: NymLabel::GivenName,
            mass: 0.9,
            is_begin: true,
        };
        scores[surname + 1] = PieceScore {
            label: NymLabel::Surname,
            mass: 0.9,
            is_begin: true,
        };
        let spans = decode_captured(&text, &offsets, &scores, &op).unwrap();
        assert_eq!(spans.len(), 2, "{separator:?}: {spans:?}");
    }
}

/// Invented names with one scored piece per word exercise both BIO assembly and name joining.
fn record_separated_name_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for separator in [
        "\n", "\r", "\r\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}",
    ] {
        for (first_label, second_label, second_begin) in [
            (NymLabel::GivenName, NymLabel::GivenName, false),
            (NymLabel::Surname, NymLabel::Surname, false),
            (NymLabel::GivenName, NymLabel::GivenName, true),
            (NymLabel::GivenName, NymLabel::Surname, true),
            (NymLabel::Surname, NymLabel::GivenName, true),
        ] {
            let text = format!("Nelia{separator}Liora");
            let offsets = (0..text.chars().count())
                .map(|i| (i, i + 1))
                .collect::<Vec<_>>();
            let mut scores = vec![
                PieceScore {
                    label: NymLabel::Age,
                    mass: 0.0,
                    is_begin: true,
                };
                offsets.len()
            ];
            scores[1] = PieceScore {
                label: first_label,
                mass: 0.99,
                is_begin: true,
            };
            scores[6 + separator.chars().count()] = PieceScore {
                label: second_label,
                mass: 0.99,
                is_begin: second_begin,
            };
            cases.push(Case {
                text,
                offsets,
                scores,
            });
        }
    }
    cases
}

#[test]
fn person_name_bio_continuations_preserve_record_boundaries() {
    let mut crossings = Vec::new();
    for case in record_separated_name_cases() {
        let spans = decode_captured(
            &case.text,
            &case.offsets,
            &case.scores,
            &NymOperatingPoint::default(),
        )
        .unwrap();
        let ranges = spans.iter().map(|span| span.0.clone()).collect::<Vec<_>>();
        if ranges != [0..5, case.text.len() - 5..case.text.len()] {
            crossings.push(format!("{:?}: {spans:?}", case.text));
        }
    }
    assert!(crossings.is_empty(), "record crossings: {crossings:#?}");
}

#[test]
fn person_name_pipeline_preserves_record_boundaries_and_restores() {
    use gaze::{Action, CleanDocument, DefaultRule, Pipeline, RawDocument, Scope, Session};
    let mut crossings = Vec::new();
    for case in record_separated_name_cases() {
        let raw = case.text.clone();
        let separator = &raw[5..raw.len() - 5];
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(CapturedNameNet(case))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (CleanDocument::Text(clean), manifest, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw.clone()),
                &[LocaleTag::Global],
                &gaze::DictionaryBundle::default(),
                gaze::SafetyNetPolicy::default(),
            )
            .unwrap()
        else {
            panic!("text");
        };
        assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
        if !clean.contains(separator) || manifest.len() != 2 {
            crossings.push(format!(
                "raw={raw:?}, clean={clean:?}, manifest={manifest:?}"
            ));
        }
    }
    assert!(crossings.is_empty(), "record crossings: {crossings:#?}");
}

#[test]
fn captured_person_names_are_enabled_by_default_with_auditable_thresholds() {
    let cases = name_cases();
    for (id, expected, labels) in [
        (
            "nelia-title-prose",
            "Nelia Quillmere",
            "GIVEN_NAME>=0.5;SURNAME>=0.5",
        ),
        (
            "nelia-title-chat",
            "Nelia Quillmere",
            "GIVEN_NAME>=0.5;SURNAME>=0.5",
        ),
        (
            "nelia-title-json",
            "Nelia Quillmere",
            "GIVEN_NAME>=0.5;SURNAME>=0.5",
        ),
        (
            "nelia-upper-prose",
            "NELIA QUILLMERE",
            "GIVEN_NAME>=0.5;SURNAME>=0.5",
        ),
        ("élia-lower-prose", "élia", "GIVEN_NAME>=0.5"),
        ("élia-lower-chat", "élia", "GIVEN_NAME>=0.5"),
        ("élia-lower-json", "élia", "GIVEN_NAME>=0.5"),
    ] {
        let (_, case) = cases.iter().find(|(row, _)| row["id"] == id).unwrap();
        let manifest = Manifest::default();
        let ctx = SafetyNetContext::new(
            &manifest,
            &[LocaleTag::Global],
            DocumentKind::Text,
            None,
            None,
        );
        let suspects = check_captured(
            &case.text,
            &case.offsets,
            &case.scores,
            &NymOperatingPoint::default(),
            ctx,
        )
        .unwrap();
        let names = suspects
            .iter()
            .filter(|suspect| suspect.class == gaze_types::PiiClass::Name)
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 1, "{id}: {suspects:?}");
        assert_eq!(&case.text[names[0].span.clone()], expected, "{id}");
        assert_eq!(names[0].raw_label, labels, "{id}");
    }
}

struct CapturedNameNet(Case);

impl SafetyNet for CapturedNameNet {
    fn id(&self) -> &str {
        "nym-small-int8"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<gaze_types::LeakSuspect>, gaze_types::SafetyNetError> {
        // One captured view. Later passes have different text and require fresh model output;
        // the ignored live test checks those passes with the pinned model itself.
        if text != self.0.text {
            return Ok(Vec::new());
        }
        let op =
            NymOperatingPoint::new([(NymLabel::GivenName, 0.5), (NymLabel::Surname, 0.5)]).unwrap();
        check_captured(text, &self.0.offsets, &self.0.scores, &op, context)
    }
}

#[test]
fn captured_person_names_restore_exactly_and_benign_controls_stay_raw() {
    use gaze::{Action, CleanDocument, DefaultRule, Pipeline, RawDocument, Scope, Session};
    for (input, case) in name_cases() {
        let raw = case.text.clone();
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(CapturedNameNet(case))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, manifest, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw.clone()),
                &[LocaleTag::Global],
                &gaze::DictionaryBundle::default(),
                gaze::SafetyNetPolicy::default(),
            )
            .unwrap();
        let CleanDocument::Text(clean) = clean else {
            panic!("text expected");
        };
        assert_eq!(
            session.restore_strict_text(&clean).unwrap(),
            raw,
            "{}",
            input["id"]
        );
        if input["gold"].is_null() {
            assert_eq!(clean, raw, "{}", input["id"]);
            assert!(manifest.is_empty(), "{}", input["id"]);
        }
    }
}

struct NameWinner(gaze_types::EvidenceKind);

impl gaze_types::Detector for NameWinner {
    fn evidence(&self) -> gaze_types::EvidenceKind {
        self.0
    }
    fn detect(&self, input: &str) -> Vec<gaze_types::Detection> {
        vec![gaze_types::Detection::new(
            0..input.len(),
            gaze_types::PiiClass::Name,
            if self.0 == gaze_types::EvidenceKind::Rule {
                "fixture.name_rule"
            } else {
                "ner"
            },
        )]
    }
}

struct OwnedTokenNameNet;

impl SafetyNet for OwnedTokenNameNet {
    fn id(&self) -> &str {
        "nym-small-int8"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<gaze_types::LeakSuspect>, gaze_types::SafetyNetError> {
        // Deliberately hostile model output: label the entire earlier winner's token as a name.
        let offsets = (0..text.chars().count())
            .map(|i| (i, i + 1))
            .collect::<Vec<_>>();
        let scores = offsets
            .iter()
            .enumerate()
            .map(|(i, _)| PieceScore {
                label: NymLabel::GivenName,
                mass: 0.99,
                is_begin: i == 0,
            })
            .collect::<Vec<_>>();
        check_captured(
            text,
            &offsets,
            &scores,
            &NymOperatingPoint::default(),
            context,
        )
    }
}

#[test]
fn nym_name_preserves_rule_and_learned_name_winners() {
    use gaze::{
        Action, ClassRule, CleanDocument, DefaultRule, Pipeline, RawDocument, Scope, Session,
    };
    for evidence in [
        gaze_types::EvidenceKind::Rule,
        gaze_types::EvidenceKind::Learned,
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let build = || {
            Pipeline::builder()
                .detector(NameWinner(evidence))
                .rule(ClassRule::new(gaze_types::PiiClass::Name, Action::Tokenize))
                .rule(DefaultRule::new(Action::Preserve))
        };
        let raw = "Nelia Quillmere";
        let baseline = build()
            .build()
            .unwrap()
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw.into()),
                &[LocaleTag::Global],
                &gaze::DictionaryBundle::default(),
                gaze::SafetyNetPolicy::default(),
            )
            .unwrap();
        let protected = build()
            .register_safety_net(OwnedTokenNameNet)
            .build()
            .unwrap()
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw.into()),
                &[LocaleTag::Global],
                &gaze::DictionaryBundle::default(),
                gaze::SafetyNetPolicy::default(),
            )
            .unwrap();
        let (CleanDocument::Text(before), CleanDocument::Text(after)) = (baseline.0, protected.0)
        else {
            panic!("text");
        };
        assert_eq!(after, before);
        assert_eq!(protected.1, baseline.1);
        assert!(protected.2.suspects.is_empty());
        assert_eq!(session.restore_strict_text(&after).unwrap(), raw);
    }
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
            ("Beispiel", NymLabel::Surname),
            ("12.03.1985", NymLabel::DateOfBirth),
            ("12a", NymLabel::BuildingNumber)
        ]
    );
    // DATE_OF_BIRTH clears its 0.9 threshold; the Python probe measured 0.952 here.
    assert!((decoded[1].2 - 0.952).abs() < 0.001, "{}", decoded[1].2);
}

/// The 4192 sentence: a salutation, a surname and a credential. The model does see the surname
/// and the token (SURNAME and SSN pieces at >= 0.9); only the surname is supported.
#[test]
fn captured_salutation_protects_surname_but_not_unsupported_secret() {
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
    assert_eq!(
        spans("salutation-4192"),
        owned(&[("Müller", NymLabel::Surname)])
    );
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
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by CI test-support)"]
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
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by CI test-support)"]
fn live_salutation_protects_surname_but_not_unsupported_secret() {
    let text = case_text("salutation-4192");
    let suspects = live_check(&text);
    assert_eq!(suspects.len(), 1, "{suspects:?}");
    assert_eq!(&text[suspects[0].span.clone()], "Müller");
    assert_eq!(suspects[0].raw_label, "SURNAME>=0.5");
}

#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by CI test-support)"]
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
#[ignore = "needs GAZE_NYM_MODEL_DIR (run by CI test-support)"]
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

/// Offline calibration, deliberately separate from every benchmark corpus. Capture once, then
/// replay the same model rows at each declared threshold through the production word decoder.
/// The output also preserves the rows for model-free regression tests and review.
#[test]
#[ignore = "offline synthetic calibration; needs GAZE_NYM_MODEL_DIR and GAZE_NYM_NAME_CAPTURE"]
fn offline_capture_person_name_calibration() {
    let data: Value =
        serde_json::from_str(include_str!("fixtures/nym_name_calibration.json")).unwrap();
    let cases = data["cases"].as_array().unwrap();
    let captured = cases
        .iter()
        .map(|case| {
            let text = case["text"].as_str().unwrap();
            let (offsets, scores) = capture(live_net(), text).unwrap();
            (case, offsets, scores)
        })
        .collect::<Vec<_>>();
    let mut summary = Vec::new();
    for label in [NymLabel::GivenName, NymLabel::Surname] {
        for threshold in data["thresholds"].as_array().unwrap() {
            let threshold = threshold.as_f64().unwrap() as f32;
            let op = NymOperatingPoint::new([(label, threshold)]).unwrap();
            let mut recovered = 0;
            let mut false_positive = 0;
            let mut benign_bytes = 0;
            for (case, offsets, scores) in &captured {
                let text = case["text"].as_str().unwrap();
                let gold = case["gold"].as_array().map(|range| {
                    range[0].as_u64().unwrap() as usize..range[1].as_u64().unwrap() as usize
                });
                for (span, _, _) in decode_captured(text, offsets, scores, &op).unwrap() {
                    for byte in span {
                        if gold.as_ref().is_some_and(|range| range.contains(&byte)) {
                            recovered += 1;
                        } else {
                            false_positive += 1;
                            if gold.is_none() {
                                benign_bytes += 1;
                            }
                        }
                    }
                }
            }
            summary.push(json!({
                "label": label.as_str(), "threshold": threshold,
                "recovered_name_bytes": recovered, "false_positive_bytes": false_positive,
                "benign_false_positive_bytes": benign_bytes,
            }));
        }
    }
    let rows = captured
        .iter()
        .map(|(case, offsets, scores)| {
            let mut row = (*case).clone();
            row["offsets"] = json!(offsets);
            row["scores"] = json!(scores
                .iter()
                .map(|score| { (score.label.as_str(), score.mass, score.is_begin) })
                .collect::<Vec<_>>());
            row
        })
        .collect::<Vec<_>>();
    let output = json!({
        "provenance": {
            "bundle_sha256": gaze_recognizers::safety_net::nym::NYM_SMALL_INT8_BUNDLE_SHA256,
            "model": "Wismut/nym-pii-multilingual-small@4348999cd3c2e20c49615e9af7c6bbb45b64cd85 int8",
            "threads": 1, "cases": cases.len(), "benchmark_data_used": false,
        },
        "summary": summary, "cases": rows,
    });
    let path = std::env::var_os("GAZE_NYM_NAME_CAPTURE")
        .expect("set GAZE_NYM_NAME_CAPTURE to the offline capture output path");
    std::fs::write(path, serde_json::to_string_pretty(&output).unwrap() + "\n").unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&output["summary"]).unwrap()
    );
}

#[test]
fn covered_name_emits_no_suspect_before_pipeline_filtering() {
    let text = "<deadbeef:Name_1>";
    let manifest = Manifest::from_spans(vec![gaze_types::EmittedTokenSpan::new(
        0..text.len(),
        0.."Nelia Quillmere".len(),
        gaze_types::PiiClass::Name,
    )]);
    let context = SafetyNetContext::new(
        &manifest,
        &[LocaleTag::Global],
        DocumentKind::Text,
        None,
        None,
    );
    assert!(OwnedTokenNameNet.check(text, context).unwrap().is_empty());
}
