//! Cue-anchored identifiers in structured text: tool-call JSON, `key=value` and `key: value` logs.
//!
//! Every cue rule was written for prose (`BSN: 111222333`). In a tool call the cue is a JSON key,
//! so a quote sits between the cue and the value (`"bsn": "111222333"`), keys are snake / camel /
//! kebab case (`steuer_id`, `steuerId`, `nhs_number`) or carry a prefix (`customer_ssn`), and log
//! lines use `bsn=`. None of those matched, so every value shipped raw on the agentic path;
//! gaze-proxy cleans `tool_calls[].function.arguments` as one text string. The key vocabulary
//! lives in the `core` rulepack patterns, not in Rust.
//!
//! Fixture values are synthetic, checksum-valid test numbers where a validator exists.

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
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    };
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(&chain()));
    gaze_assembly::build_pipeline(&policy, &context, &[rulepack], &locale_chain, None)
        .expect("pipeline")
}

fn pipeline_with_phone_locale(pack_name: &str, locale: LocaleTag) -> Pipeline {
    let packs = ["core", pack_name].map(|name| {
        Rulepack::load(RulepackSource::Embedded(
            embedded(name).expect("embedded rulepack"),
        ))
        .expect("rulepack loads")
    });
    let mut policy = gaze::Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let context = Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    };
    let active = LocaleChain::merge_cli_policy_rulepack_default(
        None,
        None,
        Some(&[locale, LocaleTag::Global]),
    );
    gaze_assembly::build_pipeline(&policy, &context, &packs, &active, None)
        .expect("phone locale pipeline")
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

#[test]
fn localized_phone_labels_keep_parser_failed_international_values() {
    // The digits remain in Ofcom's reserved 7700 900xxx range.
    let number = "+44 7/7/0/0/9/0/0/1/2/3";
    for (pack, locale, label) in [
        ("locale-de", "de-DE", "Telefon"),
        ("locale-de", "de-DE", "Handy"),
        ("locale-de", "de-DE", "Tel."),
        ("locale-fr", "fr-FR", "téléphone"),
        ("locale-nl", "nl-NL", "telefoon"),
        ("locale-br", "pt-BR", "telefone"),
    ] {
        let locale = LocaleTag::parse(locale).expect("locale");
        let pipeline = pipeline_with_phone_locale(pack, locale.clone());
        let session = Session::new(Scope::Ephemeral).expect("session");
        let input = format!("{label}: {number}; phoneModel={number}; order={number}");
        let (clean, _, _) = pipeline
            .clean_with_safety_net_detect_context(
                &session,
                RawDocument::Text(input.clone()),
                &[locale, LocaleTag::Global],
                &DictionaryBundle::default(),
            )
            .expect("clean");
        let CleanDocument::Text(cleaned) = clean else {
            panic!("expected text");
        };
        assert_eq!(
            cleaned.matches(number).count(),
            2,
            "{pack} {label}: {cleaned}"
        );
        assert_eq!(
            pipeline
                .restore_strict_text(&session, &cleaned)
                .expect("restore"),
            input
        );
    }
}

/// Structured shapes a key/value pair takes in agent traffic. `{k}` is the key, `{v}` the value.
const SHAPES: [&str; 7] = [
    r#"{"name":"lookup","arguments":{"{k}":"{v}"}}"#,
    r#"{"{k}": "{v}", "action": "lookup"}"#,
    r#"{'{k}': '{v}'}"#,
    r#"{\"{k}\":\"{v}\"}"#,
    "user=42 {k}={v} action=lookup",
    "{k}: {v}",
    "{k} = \"{v}\"",
];

/// Digit-only values also appear as bare JSON numbers.
const NUMBER_SHAPE: &str = r#"{"{k}":{v}}"#;

struct Family {
    class: &'static str,
    value: &'static str,
    keys: &'static [&'static str],
}

const FAMILIES: &[Family] = &[
    Family {
        class: "bsn",
        value: "111222333",
        keys: &[
            "bsn",
            "BSN",
            "bsn_nummer",
            "burgerservicenummer",
            "user_bsn",
        ],
    },
    Family {
        class: "steuer_id",
        value: "86095742719",
        keys: &[
            "steuer_id",
            "steuerId",
            "steuer-id",
            "steuerid",
            "tax_id",
            "taxId",
            "taxIdentNr",
            "tax_ident_nr",
            "taxIdentNumber",
            "customer_steuer_id",
        ],
    },
    Family {
        class: "cpf",
        value: "111.444.777-35",
        keys: &["cpf", "CPF", "cpf_numero"],
    },
    Family {
        class: "cnpj",
        value: "11.222.333/0001-81",
        keys: &["cnpj", "cnpj_number"],
    },
    Family {
        class: "nhs_number",
        value: "9434765919",
        keys: &[
            "nhs_number",
            "nhsNumber",
            "nhs-number",
            "nhs",
            "patient_nhs_no",
        ],
    },
    Family {
        class: "ssn",
        value: "123-45-6789",
        keys: &[
            "ssn",
            "SSN",
            "customer_ssn",
            "ssn_number",
            "socialSecurityNumber",
        ],
    },
    Family {
        class: "ssn",
        value: "12345678901",
        keys: &["sv_nummer", "svNummer", "sozialversicherungsnummer"],
    },
    Family {
        class: "passport",
        value: "C01234567",
        keys: &[
            "passport",
            "passport_no",
            "passportNumber",
            "passport-number",
        ],
    },
    Family {
        class: "national_id",
        value: "AB1234567",
        keys: &["national_id", "nationalId", "id_number", "idNumber"],
    },
    Family {
        class: "driver_license",
        value: "B1234567",
        keys: &[
            "driver_license",
            "driverLicense",
            "drivers_license_number",
            "dl_number",
        ],
    },
    Family {
        class: "nino",
        value: "AB123456C",
        keys: &["nino", "ni_number", "national_insurance_number"],
    },
    Family {
        class: "pan",
        value: "ABCPD1234E",
        keys: &["pan", "pan_number", "panCard"],
    },
    Family {
        class: "aadhaar",
        value: "234123412346",
        keys: &["aadhaar", "aadhaar_number", "aadhaarNo"],
    },
    Family {
        class: "vat_id",
        value: "DE123456789",
        keys: &["vat_id", "ust_id", "ustIdNr"],
    },
];

#[test]
fn every_cue_family_is_tokenized_in_every_structured_shape_and_key_spelling() {
    let pipeline = pipeline();
    let mut failures = Vec::new();
    for family in FAMILIES {
        let digit_only = family.value.bytes().all(|b| b.is_ascii_digit());
        for key in family.keys {
            let shapes = SHAPES
                .iter()
                .chain(digit_only.then_some(&NUMBER_SHAPE))
                .copied();
            for shape in shapes {
                let text = shape.replace("{k}", key).replace("{v}", family.value);
                let cleaned = clean_and_restore(&pipeline, &text);
                let leaked = without_tokens(&cleaned).contains(family.value);
                let right_class = cleaned.contains(&format!(":Custom:{}_", family.class));
                if leaked || !right_class {
                    failures.push(format!("{text:?} -> {cleaned:?}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} structured cue fixtures leaked or took the wrong class:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn steuer_id_json_key_wins_the_whole_value_with_its_own_class() {
    // Pilot finding: `"steuer_id": "<valid>"` came back as two raw digits plus a `Custom:phone`
    // token. The whole Steuer-ID must be one `steuer_id` token.
    let pipeline = pipeline();
    let cleaned = clean_and_restore(&pipeline, r#"{"steuer_id": "86095742719"}"#);
    assert!(cleaned.contains(":Custom:steuer_id_1>"), "{cleaned:?}");
    assert!(!cleaned.contains(":Custom:phone_"), "{cleaned:?}");
    assert!(
        !without_tokens(&cleaned).chars().any(|c| c.is_ascii_digit()),
        "no digit of the Steuer-ID may survive: {cleaned:?}"
    );
}

#[test]
fn checksum_invalid_values_under_a_cue_key_follow_each_rule_policy() {
    let pipeline = pipeline();
    for (text, value, class) in [
        (r#"{"bsn": "111222334"}"#, "111222334", "bsn"),
        (
            r#"{"steuer_id": "86095742718"}"#,
            "86095742718",
            "steuer_id",
        ),
        (r#"{"cpf": "111.444.777-36"}"#, "111.444.777-36", "cpf"),
        ("bsn=111222334", "111222334", "bsn"),
    ] {
        let cleaned = clean_and_restore(&pipeline, text);
        assert!(
            cleaned.contains(&format!(":Custom:{class}_")),
            "{cleaned:?}"
        );
        assert!(!cleaned.contains(value), "{cleaned:?}");
    }
    let nhs = r#"{"nhs_number": "9434765918"}"#;
    let cleaned = clean_and_restore(&pipeline, nhs);
    assert!(cleaned.contains(":Custom:nhs_number_1>"), "{cleaned:?}");
    assert!(!without_tokens(&cleaned).chars().any(|c| c.is_ascii_digit()));
}

#[test]
fn same_shaped_values_under_unrelated_keys_stay_untouched() {
    let pipeline = pipeline();
    for text in [
        r#"{"order_id": "111222333"}"#,
        r#"{"user_id": "86095742719"}"#,
        r#"{"invoice": "9434765919"}"#,
        r#"{"bsnx": "111222333"}"#,
        r#"{"unbsn": "111222333"}"#,
        r#"{"sku": "C01234567"}"#,
        r##"{"color":"#D3D3D3"}"##,
        "id=111222333 status=ok",
        "japan=ABCPD1234E",
    ] {
        assert_eq!(clean_and_restore(&pipeline, text), text);
    }
}

#[test]
fn prose_cue_shapes_still_match() {
    // Superset check: the structured-key grammar must not lose the prose forms it extends.
    let pipeline = pipeline();
    for (text, value) in [
        ("BSN: 111222333", "111222333"),
        ("Steuer-ID 86095742719", "86095742719"),
        (
            "Her social security number is 564-23-7890 today.",
            "564-23-7890",
        ),
        ("NHS number 943 476 5919", "943 476 5919"),
        ("Passport: C01234567", "C01234567"),
    ] {
        let cleaned = clean_and_restore(&pipeline, text);
        assert!(!without_tokens(&cleaned).contains(value), "{cleaned:?}");
    }
}

#[test]
fn cued_nhs_and_tax_identification_values_have_no_raw_digit_suffix() {
    let pipeline = pipeline();
    for (key, value, class) in [
        ("nhsNumber", "943 476 5918", "nhs_number"),
        ("NHS_NO", "943-476-5918", "nhs_number"),
        ("taxIdentNr", "86 095 742 719", "steuer_id"),
        ("tax_ident_nr", "86-095-742-718", "steuer_id"),
        ("taxIdentNumber", "86095742718", "steuer_id"),
    ] {
        for shape in SHAPES {
            let input = shape.replace("{k}", key).replace("{v}", value);
            let cleaned = clean_and_restore(&pipeline, &input);
            let expected = format!(":Custom:{class}_1>");
            assert!(cleaned.contains(&expected), "{input:?} -> {cleaned:?}");
            assert_eq!(cleaned.matches(&expected).count(), 1);
            // Digits in unrelated fields are allowed; none from the value may remain.
            let raw = without_tokens(&cleaned);
            assert_eq!(raw, input.replace(value, "\0"), "{input:?} -> {cleaned:?}");
        }
    }
}

#[test]
fn zero_nhs_and_tax_identification_non_cues_stay_raw() {
    let pipeline = pipeline();
    for input in [
        "NHS number: 0000000000",
        "NHS no: 000 000 0000",
        "taxIdentifierProduct: 86095742719",
        "taxIdentificationModel: 86095742719",
        "taxIdentNrSuffix: 86095742719",
    ] {
        assert_eq!(clean_and_restore(&pipeline, input), input);
    }
}

#[test]
fn full_generic_tax_identification_field_preserves_repeated_value_protection() {
    let pipeline = pipeline();
    let value = "00 000 000 001";
    let input = format!("taxIdentificationNumber: {value}; repeat: {value}");
    let cleaned = clean_and_restore(&pipeline, &input);
    assert_eq!(without_tokens(&cleaned), input.replace(value, "\0"));
    assert_eq!(cleaned.matches(":Custom:tax_number_1>").count(), 2);
}
