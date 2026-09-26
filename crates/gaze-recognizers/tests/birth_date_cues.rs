//! Cue-anchored dates of birth in prose, tool-call JSON and `key=value` logs (solo todo #3651).
//!
//! `birth_date.cue` used to accept only a line-start field record (`DOB: 1990-02-03`) and
//! `born on` / `geboren am`, with three numeric date shapes. On v0.15.1 these shipped raw through
//! `gaze clean` and gaze-proxy alike: `Geburtsdatum 30.05.1971`, `{"dob": "30.05.1971"}`,
//! `née le 02/11/1992`, month-name dates, two-digit years.
//!
//! A bare date is never tokenized: invoices, logs and release notes are full of dates, so the
//! birth cue is the whole precision story. Every cue alternative in the rulepack has a test of
//! its own, and no alternative is shadowed by another, so dropping one cue turns red only the tests
//! that use it (`born` is shared by `born` and `born on`).

use gaze::Context;
use gaze::{
    Action, CleanDocument, DictionaryBundle, LocaleChain, LocaleTag, Pipeline, RawDocument,
    RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

fn chain() -> Vec<LocaleTag> {
    [
        "de-DE", "en-US", "en-GB", "nl-NL", "pt-BR", "fr-FR", "en-IN", "es-ES",
    ]
    .iter()
    .map(|tag| LocaleTag::parse(tag).expect("tag"))
    .chain(std::iter::once(LocaleTag::Global))
    .collect()
}

fn pipeline() -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    policy.rulepacks.bundled = vec!["core".to_string()];
    let context = Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    };
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(&chain()));
    gaze_assembly::build_pipeline(&policy, &context, &[rulepack], &locale_chain, None)
        .expect("pipeline")
}

fn clean_and_restore(pipeline: &Pipeline, text: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            &chain(),
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let CleanDocument::Text(cleaned) = clean else {
        panic!("expected text");
    };
    let restored = pipeline
        .restore_strict_text(&session, &cleaned)
        .expect("restore");
    assert_eq!(restored, text, "restore must be byte-exact");
    cleaned
}

/// The date is gone, it went into one `birth_date` token, and restore is exact.
fn assert_birth_date(text: &str, date: &str) {
    let cleaned = clean_and_restore(&pipeline(), text);
    assert!(
        !without_tokens(&cleaned).contains(date),
        "date of birth leaked: {text:?} -> {cleaned:?}"
    );
    assert!(
        cleaned.contains(":Custom:birth_date_"),
        "date of birth took the wrong class: {text:?} -> {cleaned:?}"
    );
}

macro_rules! birth_date_cases {
    ($($name:ident: $text:expr => $date:expr;)+) => {
        $(
            #[test]
            fn $name() {
                assert_birth_date($text, $date);
            }
        )+
    };
}

// One test per cue. The date shape is held fixed so each test pins its cue alone.
birth_date_cases! {
    cue_en_dob: "DOB 12.03.1987" => "12.03.1987";
    cue_en_dotted_dob: "D.O.B. 12.03.1987" => "12.03.1987";
    cue_en_date_of_birth: "Her date of birth is 12.03.1987." => "12.03.1987";
    cue_en_birth_date: "Birth date: 12.03.1987" => "12.03.1987";
    cue_en_birthdate: "birthdate 12.03.1987" => "12.03.1987";
    cue_en_birthday: "My birthday is 12.03.1987." => "12.03.1987";
    cue_en_born: "user jweber, born 12.03.1987, asked" => "12.03.1987";
    cue_en_born_on: "I was born on 12.03.1987 in Leeds." => "12.03.1987";
    cue_de_geburtsdatum: "Geburtsdatum 30.05.1971" => "30.05.1971";
    cue_de_geburtsdatum_ist_der: "Mein Geburtsdatum ist der 30.05.1971." => "30.05.1971";
    cue_de_geb_dot: "Anna Weber, geb. 30.05.1971" => "30.05.1971";
    cue_de_geb_datum: "Geb.-Datum: 30.05.1971" => "30.05.1971";
    cue_de_geburtstag: "Sein Geburtstag ist am 30.05.1971." => "30.05.1971";
    cue_de_geboren_am: "Frau Müller, geboren am 30.05.1971, wohnt" => "30.05.1971";
    cue_de_trailing_geboren: "Ich wurde am 30.05.1971 geboren." => "30.05.1971";
    cue_de_trailing_geboren_with_place: "Ich wurde am 30.05.1971 in Hamburg geboren." => "30.05.1971";
    cue_fr_nee_le: "Marie Dupont, née le 02/11/1992, habite" => "02/11/1992";
    cue_fr_ne_le: "Pierre Dubois, né le 02/11/1992." => "02/11/1992";
    cue_fr_date_de_naissance: "Ma date de naissance est le 02/11/1992." => "02/11/1992";
    cue_nl_geboren_op: "Jan de Vries, geboren op 02-11-1992, woont" => "02-11-1992";
    cue_nl_geboortedatum: "Mijn geboortedatum is 02-11-1992." => "02-11-1992";
    cue_da_fodt_den: "Lars Jensen, født den 02.11.1992, bor" => "02.11.1992";
    cue_da_fodselsdato: "Min fødselsdato er 02.11.1992." => "02.11.1992";
    cue_da_fodselsdag: "Min fødselsdag er 02.11.1992." => "02.11.1992";
    cue_es_nacido_el: "Carlos Martínez, nacido el 02/11/1992, vive" => "02/11/1992";
    cue_es_nacio: "Carlos nació el 02/11/1992 en Madrid." => "02/11/1992";
    cue_es_fecha_de_nacimiento: "Mi fecha de nacimiento es el 02/11/1992." => "02/11/1992";
}

// One test per date shape, under a fixed cue.
birth_date_cases! {
    shape_dd_mm_yyyy_dots: "Date of birth: 14.03.1987" => "14.03.1987";
    shape_d_m_yyyy_dots: "Date of birth: 4.3.1987" => "4.3.1987";
    shape_dd_mm_yyyy_slashes: "Date of birth: 14/03/1987" => "14/03/1987";
    shape_mm_dd_yyyy_slashes: "Date of birth: 11/25/1990" => "11/25/1990";
    shape_iso: "Date of birth: 1990-07-21" => "1990-07-21";
    shape_yyyy_mm_dd_slashes: "Date of birth: 1984/03/12" => "1984/03/12";
    shape_compact_yyyymmdd: "Date of birth: 19840312" => "19840312";
    shape_dd_mm_yyyy_dashes: "Date of birth: 14-03-1987" => "14-03-1987";
    shape_two_digit_year: "Date of birth: 14.03.87" => "14.03.87";
    shape_day_month_name_year: "Date of birth: 14 March 1987" => "14 March 1987";
    shape_month_name_day_year: "Date of birth: March 14, 1987" => "March 14, 1987";
    shape_german_month_name: "Date of birth: 14. März 1987" => "14. März 1987";
    shape_french_month_name: "Date of birth: 12 mars 1984" => "12 mars 1984";
    shape_french_premier: "Date of birth: 1er mars 1984" => "1er mars 1984";
    shape_spanish_de_month_de: "Date of birth: 12 de marzo de 1987" => "12 de marzo de 1987";
    shape_abbrev_month_dashes: "Date of birth: 12-Mar-1984" => "12-Mar-1984";
    shape_nbsp_separators: "Date of birth:\u{a0}14\u{a0}March\u{a0}1987" => "14\u{a0}March\u{a0}1987";
    shape_narrow_nbsp_separators: "Date of birth:\u{202f}14\u{202f}March\u{202f}1987" => "14\u{202f}March\u{202f}1987";
}

/// Structured shapes a key/value pair takes in agent traffic. `{k}` is the key, `{v}` the value.
/// Same set as `structured_cue_shapes.rs` (#647).
const SHAPES: [&str; 7] = [
    r#"{"name":"lookup","arguments":{"{k}":"{v}"}}"#,
    r#"{"{k}": "{v}", "action": "lookup"}"#,
    r#"{'{k}': '{v}'}"#,
    r#"{\"{k}\":\"{v}\"}"#,
    "user=42 {k}={v} action=lookup",
    "{k}: {v}",
    "{k} = \"{v}\"",
];

const KEYS: [&str; 14] = [
    "dob",
    "DOB",
    "customer_dob",
    "date_of_birth",
    "dateOfBirth",
    "date-of-birth",
    "birthDate",
    "birth_date",
    "birthdate",
    "user_birth_date",
    "birthday",
    "geburtsdatum",
    "geb_datum",
    "Geburtsdatum",
];

#[test]
fn every_birth_key_is_tokenized_in_every_structured_shape() {
    let pipeline = pipeline();
    let mut failures = Vec::new();
    for key in KEYS {
        for value in ["30.05.1971", "1971-05-30", "05/30/1971"] {
            for shape in SHAPES {
                let text = shape.replace("{k}", key).replace("{v}", value);
                let cleaned = clean_and_restore(&pipeline, &text);
                if without_tokens(&cleaned).contains(value)
                    || !cleaned.contains(":Custom:birth_date_")
                {
                    failures.push(format!("{text:?} -> {cleaned:?}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} structured birth-date fixtures leaked or took the wrong class:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn birth_date_in_a_tool_result_with_other_fields() {
    let text = r#"{"note": "born 03/12/1984", "dob": "30.05.1971", "since": "2019-04-01"}"#;
    let cleaned = clean_and_restore(&pipeline(), text);
    let raw = without_tokens(&cleaned);
    assert!(!raw.contains("03/12/1984"), "{cleaned:?}");
    assert!(!raw.contains("30.05.1971"), "{cleaned:?}");
    // `since` is not a birth cue: a membership date stays as it was.
    assert!(raw.contains("2019-04-01"), "{cleaned:?}");
}

#[test]
fn dates_without_a_birth_cue_stay_untouched() {
    let pipeline = pipeline();
    for text in [
        "Invoice date: 12.03.2024",
        "Payment due 2024-03-12, thanks.",
        r#"{"created_at": "2024-03-12", "due": "12/03/2024"}"#,
        "Version 2.4.1 released on 12.03.2024.",
        "2024-03-12T10:00:00Z INFO request served",
        "[12/03/2024 10:00:01] worker started",
        "Termin am 12.03.2024 um 10 Uhr",
        "Rechnung vom 14. März 2024",
        "Livraison le 02/11/2024",
        "Born to Run (1975) is an album.",
        "She was born in 1975.",
        "Adobe 12.03.1987 build",
        r#"{"dobby": "12.03.1987"}"#,
        r#"{"undob": "12.03.1987"}"#,
        r#"{"birthday_party": "12.03.2024"}"#,
        "Firmengeburtstag 12.03.2024",
        "reborn on 12.03.2024",
        "je ne le 12.03.2024",
        "DOB: pending",
        "Geb. Müller",
        "born 12.03.1987x",
        "born 12.03.1987_2",
    ] {
        assert_eq!(clean_and_restore(&pipeline, text), text, "{text:?}");
    }
}

#[test]
fn only_the_date_is_tokenized_not_the_cue_or_the_following_text() {
    let text = "Geboren am 30.05.1971 in Köln.";
    let cleaned = clean_and_restore(&pipeline(), text);
    assert!(cleaned.starts_with("Geboren am "), "{cleaned:?}");
    assert!(cleaned.ends_with(" in Köln."), "{cleaned:?}");
}

/// The rule this recognizer replaced, verbatim. Every value it captured must still be captured
/// with the same span: the re-anchoring is a widening, never a trade.
const RETIRED_FIELD_RECORD_PATTERN: &str = r#"(?im)(?:^[ \t]*(?:date of birth|birth date|birthdate|DOB|Geburtsdatum)[ \t]*[:=][ \t]*(?:"((?:[0-9]{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12][0-9]|3[01])|(?:0?[1-9]|[12][0-9]|3[01])\.(?:0?[1-9]|1[0-2])\.[0-9]{4}|(?:(?:0?[1-9]|1[0-2])/(?:0?[1-9]|[12][0-9]|3[01])|(?:0?[1-9]|[12][0-9]|3[01])/(?:0?[1-9]|1[0-2]))/[0-9]{4}))"|'((?:[0-9]{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12][0-9]|3[01])|(?:0?[1-9]|[12][0-9]|3[01])\.(?:0?[1-9]|1[0-2])\.[0-9]{4}|(?:(?:0?[1-9]|1[0-2])/(?:0?[1-9]|[12][0-9]|3[01])|(?:0?[1-9]|[12][0-9]|3[01])/(?:0?[1-9]|1[0-2]))/[0-9]{4}))'|((?:[0-9]{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12][0-9]|3[01])|(?:0?[1-9]|[12][0-9]|3[01])\.(?:0?[1-9]|1[0-2])\.[0-9]{4}|(?:(?:0?[1-9]|1[0-2])/(?:0?[1-9]|[12][0-9]|3[01])|(?:0?[1-9]|[12][0-9]|3[01])/(?:0?[1-9]|1[0-2]))/[0-9]{4})))[ \t]*(?:\r?\n|\z)|\b(?:born on|geboren am)[ \t]+((?:[0-9]{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12][0-9]|3[01])|(?:0?[1-9]|[12][0-9]|3[01])\.(?:0?[1-9]|1[0-2])\.[0-9]{4}|(?:(?:0?[1-9]|1[0-2])/(?:0?[1-9]|[12][0-9]|3[01])|(?:0?[1-9]|[12][0-9]|3[01])/(?:0?[1-9]|1[0-2]))/[0-9]{4}))(?:[^\w./-]|\.(?:[^\w./-]|\z)|\z))"#;

fn captured_spans(regex: &regex::Regex, groups: &[usize], text: &str) -> Vec<(usize, usize)> {
    regex
        .captures_iter(text)
        .filter_map(|caps| {
            groups
                .iter()
                .filter_map(|group| caps.get(*group))
                .find(|m| !m.as_str().is_empty())
                .map(|m| (m.start(), m.end()))
        })
        .collect()
}

#[test]
fn rule_captures_every_value_the_retired_field_record_rule_captured() {
    let spec = Rulepack::load(RulepackSource::Embedded(embedded("core").unwrap()))
        .unwrap()
        .recognizers
        .into_iter()
        .find(|r| r.id == "birth_date.cue")
        .expect("birth_date.cue");
    let gaze::RawMatch::Regex {
        pattern: Some(pattern),
        capture_groups,
        ..
    } = spec.matcher
    else {
        panic!("literal regex required")
    };
    let current = regex::Regex::new(&pattern).unwrap();
    let current_groups: Vec<usize> = capture_groups
        .expect("capture groups")
        .into_iter()
        .map(|g| g as usize)
        .collect();
    let retired = regex::Regex::new(RETIRED_FIELD_RECORD_PATTERN).unwrap();

    let mut checked = 0usize;
    let mut regressions = Vec::new();
    let cues = [
        "date of birth",
        "Date Of Birth",
        "birth date",
        "birthdate",
        "DOB",
        "dob",
        "Geburtsdatum",
        "born on",
        "Born On",
        "geboren am",
    ];
    let leads = ["", "  ", "\t", "x\n", "Name: A\r\n"];
    let separators = [
        ":",
        "=",
        " : ",
        "\t=\t",
        ":   ",
        " ",
        "  ",
        "\t",
        ":          ",
    ];
    let dates = [
        "1990-02-03",
        "1990-12-31",
        "3.2.1990",
        "03.02.1990",
        "31.12.1990",
        "2/3/1990",
        "12/31/1990",
        "31/12/1990",
        "02/03/1990",
    ];
    let quotes = [("", ""), ("\"", "\""), ("'", "'")];
    let tails = [
        "",
        "\n",
        "\r\n",
        "  \n",
        ".",
        ". Next",
        ", then",
        " ok",
        "\nnext line",
    ];
    for cue in cues {
        for lead in leads {
            for sep in separators {
                for date in dates {
                    for (open, close) in quotes {
                        for tail in tails {
                            let text = format!("{lead}{cue}{sep}{open}{date}{close}{tail}");
                            let old = captured_spans(&retired, &[1, 2, 3, 4], &text);
                            if old.is_empty() {
                                continue;
                            }
                            checked += 1;
                            let new = captured_spans(&current, &current_groups, &text);
                            for span in old {
                                if !new.contains(&span) {
                                    regressions.push(format!("{text:?}: {span:?} not in {new:?}"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        checked > 1000,
        "enumeration exercised too few retired matches: {checked}"
    );
    assert!(
        regressions.is_empty(),
        "{} of {checked} retired captures are lost:\n{}",
        regressions.len(),
        regressions.join("\n")
    );
}
