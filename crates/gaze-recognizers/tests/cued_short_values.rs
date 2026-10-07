//! Values that only their wording makes personal: `age.phrase`, `birth_date.answer`,
//! `card.cued_short` and `postal.cued_short`.
//!
//! On main these shipped raw: `I just turned 47`, `retired at the age of 66`, `i'm 64 y/o`,
//! `a 28 year old female`, `date of birth? It's 11/3/1995`, a 12-digit Maestro number after
//! `card`, `ZIP: 481 22`, `PLZ 53-320`. Each positive below is a distinct probe of one regex
//! branch, so dropping a branch turns red only its own rows. The negatives are the same wording
//! and digit shapes where the text says they are not personal; each one pins a refusal.

use gaze::{DictionaryBundle, LocaleTag, SafetyNetPolicy, Scope, Session};
use gaze_assembly::{CorePipeline, CorePipelineConfig};
use proptest::prelude::*;

const SOURCES: [&str; 5] = [
    "age.phrase",
    "birth_date.answer",
    "card.cued_short",
    "postal.cued_short",
    "postal.cued_four_digit",
];

fn core() -> CorePipeline {
    CorePipelineConfig::new().build().expect("core pipeline")
}

/// Clean `raw`, check restore is byte-exact, and return each protected value with its sources.
fn protected(core: &CorePipeline, raw: &str) -> Vec<(String, Vec<String>)> {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, spans, _, trace) = core
        .pipeline()
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .expect("clean");
    let gaze::CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(
        core.pipeline()
            .restore_strict_text(&session, &clean)
            .expect("restore"),
        raw,
        "restore must be byte-exact"
    );
    assert_eq!(spans.len(), trace.len());
    spans
        .iter()
        .zip(&trace)
        .map(|(span, entry)| {
            (
                raw[span.raw_span.clone()].to_string(),
                entry.source_ids().to_vec(),
            )
        })
        .collect()
}

fn assert_protected_by(core: &CorePipeline, raw: &str, value: &str, source: &str) {
    let found = protected(core, raw);
    assert!(
        found
            .iter()
            .any(|(text, sources)| text == value && sources.iter().any(|s| s == source)),
        "{source} did not protect exactly {value:?} in {raw:?}: {found:?}"
    );
}

fn assert_untouched(core: &CorePipeline, raw: &str) {
    let found = protected(core, raw);
    assert!(
        found
            .iter()
            .all(|(_, sources)| sources.iter().all(|s| !SOURCES.contains(&s.as_str()))),
        "false match in {raw:?}: {found:?}"
    );
}

#[test]
fn age_phrase_branches_each_have_a_distinct_probe() {
    let core = core();
    for (raw, age) in [
        // year old female / male
        ("This 28 year old female reports knee pain.", "28"),
        ("A 45-year-old male presented with a cough.", "45"),
        // turned, with a clause boundary or a time word after it
        ("I just turned 47 and booked a trip.", "47"),
        ("My mother turned 61 last week.", "61"),
        ("We both turned 40 last year.", "40"),
        ("We turned 40 in March.", "40"),
        ("We turn 40 next year.", "40"),
        ("we turn 40 in June", "40"),
        ("We turn 30 in two weeks!", "30"),
        ("We turn 65 in April and retire.", "65"),
        // Collective personal ages stay protected even when wording could be an anniversary.
        ("We turn 10 in March!", "10"),
        ("We turn 10 in March! Our birthdays are in May.", "10"),
        (
            "We turn 10 in March; celebrate our studio anniversary.",
            "10",
        ),
        (
            r#"{"note":"We turn 10 in March","reason":"birthday party"}"#,
            "10",
        ),
        (
            "We turn 60 in May, both of us are celebrating our birthdays.",
            "60",
        ),
        (
            "We turn 42 in October; our birthdays fall on the same day.",
            "42",
        ),
        (
            "We turn 31 in July, both celebrating another year of life.",
            "31",
        ),
        ("We turn 22 next autumn and want a birthday party.", "22"),
        (
            "We turn 47 in December, then we will be 47 years old.",
            "47",
        ),
        ("note=\"customer turned 33\"", "33"),
        ("I'll turn 40 next month.", "40"),
        // geworden
        ("Meine Tochter ist letzte Woche 12 geworden.", "12"),
        ("Ich bin im Mai 35 Jahre alt geworden.", "35"),
        // at the age of / im Alter von
        ("He retired at the age of 66 after the merger.", "66"),
        ("{\"note\":\"patient quit smoking at the age of 52\"}", "52"),
        (
            "Meine Oma hat im Alter von 70 Jahren Spanisch gelernt.",
            "70",
        ),
        // y/o after a person copula, or before a person noun
        ("hi all, i'm 64 y/o and new here", "64"),
        ("Patient is a 34 y/o with chest pain.", "34"),
        ("3,56 y/o male with fever", "56"),
    ] {
        assert_protected_by(&core, raw, age, "age.phrase");
    }
}

#[test]
fn age_phrase_refuses_objects_animals_units_and_missing_persons() {
    let core = core();
    for raw in [
        "The old bridge turned 47 this spring.",
        "The company turned 40 in May.",
        "She turned 90 degrees to face the door.",
        "He turned 45 degrees and walked back.",
        "Die Firma ist dieses Jahr 40 geworden.",
        "The oak was felled at the age of 88.",
        "The firm, at the age of 30, was sold to a rival.",
        "Die Eiche wurde im Alter von 80 Jahren gefällt.",
        "My 12 y/o laptop still boots.",
        "We are replacing a 12 year old codebase.",
        "The shelter has a 3 year old female cat for adoption.",
        "They sold the 9-year-old male horse last week.",
        "I turned 130 last week.",
        // A decimal is a distance or a quantity, never an age: `.` before a digit is no boundary.
        "He turned 3.5 km into the run.",
        "She turned 2,5 Runden.",
    ] {
        assert_untouched(&core, raw);
    }
}

#[test]
fn birth_date_answer_needs_the_copula_right_after_the_break() {
    let core = core();
    for (raw, date) in [
        (
            "Can you confirm your date of birth? It's 3/11/1987.",
            "3/11/1987",
        ),
        (
            "You asked for my date of birth. It is 8/15/1960.",
            "8/15/1960",
        ),
        ("DOB? That's 1987-03-11.", "1987-03-11"),
        (
            "Sie fragten nach meinem Geburtsdatum. Es ist der 11.03.1987.",
            "11.03.1987",
        ),
        ("Mein Geburtsdatum? Das ist der 01.03.1969.", "01.03.1969"),
        ("your date of birth?\nIt's 12 March 1987", "12 March 1987"),
        (
            "utterance=\"my date of birth? it is 1946-01-24\"",
            "1946-01-24",
        ),
    ] {
        assert_protected_by(&core, raw, date, "birth_date.answer");
    }
    for raw in [
        "Date of birth is optional. The form closes 5/19/2026.",
        "Please update your date of birth. Last login was 10/26/2026.",
        "We could not verify the date of birth. Retry after 10/26/2026.",
        "Date of birth. It's required by 5/19/2026.",
    ] {
        assert_untouched(&core, raw);
    }
}

#[test]
fn card_cued_short_takes_maestro_lengths_right_after_a_card_label() {
    let core = core();
    for (raw, card) in [
        ("What is the limit for card 504718263954?", "504718263954"),
        (
            "Please block my debit card number 674139008231 today.",
            "674139008231",
        ),
        ("Meine Kartennummer lautet 5047182639541.", "5047182639541"),
        ("Maestro 679751128852 wurde gesperrt.", "679751128852"),
        (
            "card_number=580000000000004 reason=limit",
            "580000000000004",
        ),
        ("card_no=\"674139008231\" result=ok", "674139008231"),
        (
            "{\"payment\":{\"cardNumber\":\"504718263954\"}}",
            "504718263954",
        ),
        (
            "{\"wallet\":{\"card\":{\"number\":\"504718263954\"}}}",
            "504718263954",
        ),
        (
            "My card number is 5825 6151 9692, can you check it?",
            "5825 6151 9692",
        ),
    ] {
        assert_protected_by(&core, raw, card, "card.cued_short");
    }
    for raw in [
        "Order number 504718263954 shipped this morning.",
        "Card reader serial 5047 1826 3954 needs a firmware update.",
        "The card terminal ID is 504718263954; restart it tonight.",
        "card number 404718263954",
        "card number 5047182639541234567890",
        "level=info event=job.done ts_ms=1737966651671 status=ok",
    ] {
        assert_untouched(&core, raw);
    }
}

#[test]
fn postal_cued_short_takes_short_and_foreign_codes_right_after_a_postal_label() {
    let core = core();
    for (raw, code) in [
        ("Ship it to the office, ZIP: 481 22.", "481 22"),
        (
            "Our postcode is 508\u{a0}80 if the courier asks.",
            "508\u{a0}80",
        ),
        ("Die PLZ lautet 53-320.", "53-320"),
        ("Postleitzahl: 65-572, bitte eintragen.", "65-572"),
        ("{\"shipping\":{\"postalCode\":\"768949\"}}", "768949"),
        ("cep=\"75534-030\" status=ok", "75534-030"),
        ("zip_code=70928-012 result=hit", "70928-012"),
        ("My zip code is 195.", "195"),
        ("Postal code: 614. Thanks!", "614"),
        ("My zip is 560001.", "560001"),
        ("My zip is 195.", "195"),
        ("ZIP 560001", "560001"),
        ("zip 560001", "560001"),
        ("zip\t560001", "560001"),
        ("Zip - 560001", "560001"),
        ("zip 110001, Delhi", "110001"),
        ("(zip 560001)", "560001"),
        ("customer.zip=560001", "560001"),
        (r#"order.shipping.zip = "481 22""#, "481 22"),
        ("zip: 614", "614"),
        ("{\"zip\":\"560001\"}", "560001"),
        ("ZIP 481 22", "481 22"),
        ("zip 195 b", "195"),
        ("zip 195 B-block", "195"),
        ("zip 195 Block", "195"),
        ("zip 195 B5", "195"),
        ("zip 560001 lines", "560001"),
        ("backup.zip 560001 B", "560001"),
        ("I'll zip 560001", "560001"),
        ("we zip 560001", "560001"),
        ("to zip 560001", "560001"),
        ("please zip 560001", "560001"),
        ("zip 560001 b", "560001"),
        ("zip 560001 B-block", "560001"),
        ("Ship to zip 560001.", "560001"),
        ("Please ship to zip 560001", "560001"),
        ("send it to zip 195", "195"),
        ("Change to zip 481 22.", "481 22"),
        ("I zip 560001", "560001"),
        ("we zip 110001", "110001"),
        ("zip 560001 B", "560001"),
        ("zip 560001 b-block", "560001"),
        ("ZIP 560001 Lines Road", "560001"),
        ("Ship to ZIP 560001.", "560001"),
        ("{\"zip\": \"560001\"}", "560001"),
    ] {
        assert_protected_by(&core, raw, code, "postal.cued_short");
    }
    for raw in [
        "Batch 481 22 passed QA.",
        "Postcode lookup failed for batch 481 22.",
        "The zip code service rejected ticket 900 00.",
        "Postleitzahl fehlt, Fehlercode 86-169.",
        "level=warn event=zip.validation.failed order=600686",
        "ZIP upload finished in 614 seconds.",
        "The zip archive holds 869 files.",
        "ZIP: 1234567",
        // No German, Austrian or Swiss code has three digits (`postal_at_ch.rs` pins it too).
        "PLZ: 123",
        "PLZ 123 fehlt noch",
        // Size units and plural counts identify archive operations.
        "Uploaded backup.zip 120 KB",
        "archive.zip: 345 MB",
        "zip 250 B",
        "I'll zip 250 photos tonight.",
        "zip 123456 rows",
        "zip 560001 files",
        "zip 560001 photos",
        "zip 560001 items",
        "zip 560001 records",
        "zip 560001 images",
        "zip 560001 documents",
        "backup.zip 560001 gb",
        "backup.zip 560001 TB",
        "backup.zip 560001 bytes",
    ] {
        assert_untouched(&core, raw);
    }
}

#[test]
fn postal_cued_four_digit_preserves_fields_and_refuses_archive_sizes() {
    let core = core();
    assert_protected_by(&core, "zip code 1200", "1200", "postal.cued_four_digit");
    assert_protected_by(&core, "address.zip: 8001", "8001", "postal.cued_four_digit");
    assert_protected_by(
        &core,
        "Deliver to zip 8001 please",
        "8001",
        "postal.cued_four_digit",
    );
    for raw in [
        "Uploaded backup.zip 1200 KB",
        "archive.zip: 3450 MB",
        "backup.zip 1200 KB",
    ] {
        assert_untouched(&core, raw);
    }
}

fn luhn_check_digit(payload: &str) -> char {
    let sum: u32 = payload
        .chars()
        .rev()
        .enumerate()
        .map(|(index, ch)| {
            let digit = ch.to_digit(10).expect("digit");
            if index % 2 == 0 {
                let doubled = digit * 2;
                if doubled > 9 {
                    doubled - 9
                } else {
                    doubled
                }
            } else {
                digit
            }
        })
        .sum();
    char::from_digit((10 - sum % 10) % 10, 10).expect("digit")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn any_person_age_after_turned_is_one_restorable_age_token(
        age in 1u32..=122,
        person in prop::sample::select(vec!["I", "She", "My son", "The patient", "Her husband"]),
        tail in prop::sample::select(vec![".", " last week.", " in May.", " and celebrated.", ""]),
    ) {
        let raw = format!("{person} turned {age}{tail}");
        assert_protected_by(&core(), &raw, &age.to_string(), "age.phrase");
    }

    #[test]
    fn any_cued_maestro_length_card_is_one_restorable_token(
        prefix in prop::sample::select(vec!["50", "56", "58", "63", "67", "69"]),
        length in 12usize..=15,
        body in "[0-9]{13}",
        valid in any::<bool>(),
        cue in prop::sample::select(vec!["card ", "Card number: ", "Maestro ", "card_no=", "Kartennummer lautet "]),
    ) {
        let payload = format!("{prefix}{}", &body[..length - 1 - prefix.len()]);
        let check = luhn_check_digit(&payload);
        let last = if valid { check } else { char::from_digit((check.to_digit(10).unwrap() + 1) % 10, 10).unwrap() };
        let card = format!("{payload}{last}");
        let raw = format!("{cue}{card} was blocked");
        assert_protected_by(&core(), &raw, &card, "card.cued_short");
    }

    #[test]
    fn a_short_code_after_a_benign_label_is_never_a_postcode(
        label in prop::sample::select(vec!["Batch", "Room", "Error", "Invoice", "Build"]),
        shape in 0usize..5,
        digits in "[1-9][0-9]{7}",
    ) {
        let code = match shape {
            0 => format!("{} {}", &digits[..3], &digits[3..5]),
            1 => format!("{}-{}", &digits[..2], &digits[2..5]),
            2 => format!("{}-{}", &digits[..5], &digits[5..8]),
            3 => digits[..6].to_string(),
            _ => digits[..3].to_string(),
        };
        assert_untouched(&core(), &format!("{label} {code} is ready."));
    }
}
