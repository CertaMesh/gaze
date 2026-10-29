mod support;

use gaze::{Action, LocaleTag, Pipeline, RuleSpec, Scope, Session};
use std::sync::OnceLock;

fn pipeline() -> &'static Pipeline {
    static PIPELINE: OnceLock<Pipeline> = OnceLock::new();
    PIPELINE.get_or_init(|| {
        support::pipeline_for(
            &[LocaleTag::EnUs, LocaleTag::Global],
            &["core", "locale-en"],
            vec![RuleSpec::Default {
                action: Action::Tokenize,
            }],
            true,
        )
    })
}

fn assert_exact_token(prefix: &str, value: &str, suffix: &str) {
    let input = format!("{prefix}{value}{suffix}");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let de_pipeline;
    let (selected, locales) = if value.starts_with("+49") {
        de_pipeline = support::pipeline_for(
            &[LocaleTag::DeDe, LocaleTag::Global],
            &["core", "locale-de"],
            vec![RuleSpec::Default {
                action: Action::Tokenize,
            }],
            true,
        );
        (&de_pipeline, vec![LocaleTag::DeDe, LocaleTag::Global])
    } else {
        (pipeline(), vec![LocaleTag::EnUs, LocaleTag::Global])
    };
    let clean = support::clean(selected, &session, &input, &locales);
    let blank = gaze::token_shape::pattern().replace_all(&clean, "\0");
    assert_eq!(blank, format!("{prefix}\0{suffix}"), "{input:?}");
    let entries = session.snapshot_entries();
    assert_eq!(entries.len(), 1, "{input:?}");
    assert_eq!(
        entries[0].raw, value,
        "manifest must retain exact source bytes"
    );
    assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
}

fn assert_grouped(separator: char) {
    for (cue, groups) in [
        ("Tax ID: ", &["123", "456", "789"][..]),
        (
            "IBAN: ",
            &["GB82", "WEST", "1234", "5698", "7654", "32"][..],
        ),
        ("Phone: ", &["+1", "202", "555", "0100"][..]),
        ("Card: ", &["4111", "1111", "1111", "1111"][..]),
    ] {
        let value = groups.join(&separator.to_string());
        // Multi-byte neighbours and dropped characters outside the value test both ends
        // of the normalized-to-source map, independently of strict restore.
        assert_exact_token(&format!("é\u{200b} {cue}"), &value, " ; 終\u{2060}");
    }
}

#[test]
fn zwsp_identifiers_and_email_keep_exact_source_spans() {
    assert_grouped('\u{200b}');
    assert_exact_token("é Mail: ", "al\u{200b}ice@example.invalid", " ; 終");
}

#[test]
fn word_joiner_identifiers_keep_exact_source_spans() {
    assert_grouped('\u{2060}');
    assert_exact_token("Mail: ", "al\u{2060}ice@example.invalid", "!");
}

#[test]
fn soft_hyphen_identifiers_keep_exact_source_spans() {
    assert_grouped('\u{00ad}');
    assert_exact_token("Mail: ", "al\u{00ad}ice@example.invalid", "!");
}

#[test]
fn nonbreaking_hyphen_identifiers_keep_exact_source_spans() {
    // Nonbreaking hyphens retain the same group semantics as visible hyphens.
    for (cue, groups) in [
        ("Tax ID: ", &["123", "456", "7890"][..]),
        ("Phone: ", &["+1", "202", "555", "0100"][..]),
        ("Card: ", &["4111", "1111", "1111", "1111"][..]),
    ] {
        assert_exact_token(cue, &groups.join("\u{2011}"), " ; 終");
    }
    assert_exact_token("Mail: ", "al\u{2011}ice@example.invalid", "!");
}

#[test]
fn joiners_and_bom_identifiers_keep_exact_source_spans() {
    for separator in ['\u{200c}', '\u{200d}', '\u{feff}'] {
        assert_grouped(separator);
    }
}

#[test]
fn unicode_spaces_keep_exact_identifier_source_spans() {
    for separator in ['\u{00a0}', '\u{202f}', '\u{2007}', '\u{2009}'] {
        for (cue, groups) in [
            ("Tax ID: ", &["123", "456", "7890"][..]),
            (
                "IBAN: ",
                &["GB82", "WEST", "1234", "5698", "7654", "32"][..],
            ),
            ("Phone: ", &["+1", "202", "555", "0100"][..]),
        ] {
            assert_exact_token(cue, &groups.join(&separator.to_string()), " ; 終");
        }
    }
}

#[test]
fn joined_overlong_labelled_runs_keep_every_source_byte_protected() {
    for separator in [
        '\u{200b}', '\u{2060}', '\u{00ad}', '\u{feff}', '\u{200d}', '\u{200c}',
    ] {
        for cue in ["ID card: ", "Tax number: ", "Driver's licence: "] {
            assert_exact_token(
                cue,
                &format!("AB12 CD3456{separator}XYZ123456"),
                "; public text",
            );
        }
    }
}

#[test]
fn legitimate_joiners_and_code_bom_stay_byte_exact_without_tokens() {
    for input in [
        "Family: 👩\u{200d}👩\u{200d}👧\u{200d}👦; scientist: 👩\u{200d}🔬.",
        "Persian: می\u{200c}روم and خانه\u{200c}ها.",
        "Arabic joining: ب\u{200d}ب and ب\u{200c}ب.",
        "Devanagari: क्\u{200d}ष and क्\u{200c}ष.",
        "Malayalam: ന്\u{200d} and ന്\u{200c}.",
        "\u{feff}const answer = 42;\n",
        "const label = \"hello\u{feff}world\";\n",
        "#!/usr/bin/env node\n\u{feff}console.log(42);\n",
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = support::clean(
            pipeline(),
            &session,
            input,
            &[LocaleTag::EnUs, LocaleTag::Global],
        );
        assert_eq!(clean, input);
        assert!(session.snapshot_entries().is_empty());
    }
}

#[test]
fn joined_public_references_do_not_become_labelled_identifiers() {
    for separator in [
        '\u{200b}', '\u{2060}', '\u{00ad}', '\u{feff}', '\u{200d}', '\u{200c}',
    ] {
        for prefix in ["Public build reference: ", "ID card documentation: "] {
            let input = format!("{prefix}AB12 CD3456{separator}XYZ123456");
            let session = Session::new(Scope::Ephemeral).unwrap();
            assert_eq!(
                support::clean(
                    pipeline(),
                    &session,
                    &input,
                    &[LocaleTag::EnUs, LocaleTag::Global]
                ),
                input
            );
            assert!(session.snapshot_entries().is_empty());
        }
    }
}

#[test]
fn invisible_prose_and_public_fragments_stay_byte_exact_without_tokens() {
    for input in [
        "A co\u{00ad}operative team writes inter\u{00ad}national prose.",
        "Keep these words to\u{2060}gether and preserve the source.",
        "Build AB\u{200b}CD completed; color #aa\u{2060}bbcc.",
        "See /public/soft\u{00ad}ware for the manual.",
        "Release v2\u{2011}4\u{2011}6 is ready.",
        "SKU: AB\u{200b}12\u{2060}CD\u{00ad}34",
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = support::clean(
            pipeline(),
            &session,
            input,
            &[LocaleTag::EnUs, LocaleTag::Global],
        );
        assert_eq!(clean, input);
        assert!(session.snapshot_entries().is_empty());
    }
}

#[test]
fn normalized_aliases_keep_independent_original_spellings_across_turns() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let originals = [
        "Mail: al\u{200b}ice@example.invalid",
        "Mail: al\u{2060}ice@example.invalid",
        "Mail: al\u{00ad}ice@example.invalid",
    ];
    for input in originals.iter().chain(originals.iter().rev()) {
        let clean = support::clean(
            pipeline(),
            &session,
            input,
            &[LocaleTag::EnUs, LocaleTag::Global],
        );
        assert_eq!(
            gaze::token_shape::pattern().replace_all(&clean, "\0"),
            "Mail: \0"
        );
        assert_eq!(session.restore_strict_text(&clean).unwrap(), *input);
    }
    assert_eq!(session.snapshot_entries().len(), 3);
}

#[test]
fn preservation_policy_keeps_original_formatting_and_emits_no_tokens() {
    let preserving = support::pipeline_for(
        &[LocaleTag::EnUs, LocaleTag::Global],
        &["core", "locale-en"],
        vec![RuleSpec::Default {
            action: Action::Preserve,
        }],
        true,
    );
    for input in [
        "Mail: al\u{200b}ice@example.invalid",
        "Mail: al\u{2060}ice@example.invalid",
        "Mail: al\u{00ad}ice@example.invalid",
        "Mail: al\u{2011}ice@example.invalid",
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        assert_eq!(
            support::clean(
                &preserving,
                &session,
                input,
                &[LocaleTag::EnUs, LocaleTag::Global]
            ),
            input
        );
        assert!(session.snapshot_entries().is_empty());
    }
}

#[test]
fn gap_neighbours_never_hide_validated_values() {
    for sep in [
        '\u{ad}',
        '\u{34f}',
        '\u{61c}',
        '\u{115f}',
        '\u{17b4}',
        '\u{180e}',
        '\u{200b}',
        '\u{200c}',
        '\u{200d}',
        '\u{202a}',
        '\u{2060}',
        '\u{3164}',
        '\u{fe0f}',
        '\u{feff}',
        '\u{ffa0}',
        '\u{fff0}',
        '\u{1bca0}',
        '\u{1d173}',
        '\u{e0067}',
    ] {
        for (cue, value) in [
            ("", "+44 7700 900123"),
            ("Phone: ", "+44 7700 900123"),
            ("Phone: ", "+49 171 3920000"),
            ("Card: ", "4111111111111111"),
            ("IBAN: ", "GB82WEST12345698765432"),
            ("ip ", "192.168.10.20"),
        ] {
            for prefix in [cue.to_owned(), format!("{cue}9{sep}")] {
                assert_value_protected(&prefix, value, &format!("{sep}9"), " ok");
            }
        }
        assert_value_protected("Phone: ", "+44 7700 900123", &format!("{sep}2024"), " ok");
    }
}

/// The value is one exact token. A glued neighbour joins it only when the joined value
/// validates by itself: those are over-redacted FP bytes, never a raw value byte.
fn assert_value_protected(prefix: &str, value: &str, glued: &str, suffix: &str) {
    let input = format!("{prefix}{value}{glued}{suffix}");
    let locales = if value.starts_with("+49") {
        vec![LocaleTag::DeDe, LocaleTag::Global]
    } else {
        vec![LocaleTag::EnUs, LocaleTag::Global]
    };
    let de_pipeline;
    let selected = if value.starts_with("+49") {
        de_pipeline = support::pipeline_for(
            &locales,
            &["core", "locale-de"],
            vec![RuleSpec::Default {
                action: Action::Tokenize,
            }],
            true,
        );
        &de_pipeline
    } else {
        pipeline()
    };
    let session = Session::new(Scope::Ephemeral).unwrap();
    let clean = support::clean(selected, &session, &input, &locales);
    let observed = (
        gaze::token_shape::pattern()
            .replace_all(&clean, "\0")
            .into_owned(),
        session
            .snapshot_entries()
            .into_iter()
            .map(|entry| entry.raw)
            .collect::<Vec<_>>(),
    );
    let alone = (format!("{prefix}\0{glued}{suffix}"), vec![value.to_owned()]);
    let joined = (
        format!("{prefix}\0{suffix}"),
        vec![format!("{value}{glued}")],
    );
    assert!(
        observed == alone || observed == joined,
        "{input:?}: {observed:?}"
    );
    assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
}

#[test]
fn split_digit_runs_record_known_bare_postal_false_positives() {
    for sep in ['\u{200b}', '\u{2060}', '\u{ad}', '\u{3164}'] {
        let input = "1234567890123456"
            .chars()
            .map(|ch| ch.to_string())
            .collect::<Vec<_>>()
            .join(&sep.to_string());
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = support::clean(
            pipeline(),
            &session,
            &input,
            &[LocaleTag::EnUs, LocaleTag::Global],
        );
        // Known precision limit: mixed inner skips and outer boundaries split a digit run.
        assert_eq!(
            gaze::token_shape::pattern().replace_all(&clean, "\0"),
            format!("\0{sep}\0{sep}\0{sep}6")
        );
        let entries = session.snapshot_entries();
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .all(|entry| entry.class == gaze::PiiClass::custom("postal_code").unwrap()));
        assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
    }
}

#[test]
fn valid_joined_values_never_ship_split_remainders_raw() {
    let de = support::pipeline_for(
        &[LocaleTag::DeDe, LocaleTag::Global],
        &["core", "locale-de"],
        vec![RuleSpec::Default {
            action: Action::Tokenize,
        }],
        true,
    );
    let de_locales = [LocaleTag::DeDe, LocaleTag::Global];
    let en_locales = [LocaleTag::EnUs, LocaleTag::Global];
    for sep in ['\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{ad}'] {
        for (selected, locales, cue, value) in [
            (&de, &de_locales, "Ref: Tel. ", "+49 89 99998123"),
            (&de, &de_locales, "Tel. ", "+49 30 23125123"),
            (&de, &de_locales, "Tel. ", "030 23125456"),
            (pipeline(), &en_locales, "Tel ", "+39 06 1234 5678"),
            (pipeline(), &en_locales, "Tel ", "+1 202 555 0100"),
            (pipeline(), &en_locales, "IP ", "192.168.10.20"),
        ] {
            for (split, _) in value.char_indices().skip(1) {
                let raw = format!("{}{sep}{}", &value[..split], &value[split..]);
                let input = format!("{cue}{raw} ok");
                let session = Session::new(Scope::Ephemeral).unwrap();
                let clean = support::clean(selected, &session, &input, locales);
                let blank = gaze::token_shape::pattern().replace_all(&clean, "\0");
                let middle = blank
                    .strip_prefix(cue)
                    .and_then(|rest| rest.strip_suffix(" ok"))
                    .unwrap_or_else(|| panic!("{input:?}: {blank:?}"));
                // Only tokens and the separator layout may remain: no value byte.
                assert!(
                    middle.contains('\0')
                        && middle
                            .chars()
                            .all(|ch| ch == '\0' || ch == sep || ch == ' '),
                    "{input:?}: {blank:?}"
                );
                assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
            }
        }
    }
}

#[test]
fn uncued_invalid_four_by_four_cards_do_not_expose_phone_substrings() {
    let locales = [LocaleTag::EnUs, LocaleTag::DeDe, LocaleTag::Global];
    let selected = support::pipeline_for(
        &locales,
        &["core", "locale-en", "locale-de"],
        vec![RuleSpec::Default {
            action: Action::Tokenize,
        }],
        true,
    );
    for separator in [' ', '-', '\u{200b}', '\u{2060}', '\u{feff}', '\u{ad}'] {
        let value = ["5234", "7762", "0771", "4620"].join(&separator.to_string());
        let input = format!("Public format fixture = {value}. Not assigned.");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = support::clean(&selected, &session, &input, &locales);
        assert_eq!(clean, input, "{input:?}");
        assert!(session.snapshot_entries().is_empty());

        // The same failed checksum after a card cue remains sensitive.
        let input = format!("Card: {value}");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = support::clean(&selected, &session, &input, &locales);
        assert_eq!(
            gaze::token_shape::pattern().replace_all(&clean, "\0"),
            "Card: \0",
            "{input:?}"
        );
        assert_eq!(session.snapshot_entries()[0].raw, value);
        assert_eq!(session.restore_strict_text(&clean).unwrap(), input);

        let input = format!("Phone: {value}");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = support::clean(&selected, &session, &input, &locales);
        let blank = gaze::token_shape::pattern().replace_all(&clean, "\0");
        assert!(
            blank
                .strip_prefix("Phone: ")
                .is_some_and(|value| value.chars().all(|ch| ch == '\0' || ch == separator)),
            "{input:?}: {clean}"
        );
        assert_eq!(session.restore_strict_text(&clean).unwrap(), input);

        // Nonuniform grouping is outside the precision exception.
        let value = ["523", "47762", "0771", "4620"].join(&separator.to_string());
        let input = format!("Public format fixture = {value}. Not assigned.");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = support::clean(&selected, &session, &input, &locales);
        assert!(
            session
                .snapshot_entries()
                .iter()
                .any(|entry| entry.class == gaze::PiiClass::custom("phone").unwrap()),
            "{input:?}: {clean}"
        );
        assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
    }
}

#[test]
fn valid_four_by_four_cards_remain_fully_protected() {
    for separator in [' ', '-', '\u{200b}', '\u{2060}', '\u{feff}', '\u{ad}'] {
        let value = ["4111", "1111", "1111", "1111"].join(&separator.to_string());
        assert_exact_token("Public format fixture = ", &value, ". Not assigned.");
    }
}

#[test]
fn invisible_code_fragments_do_not_create_a_postcode_prefix() {
    for separator in ['\u{200b}', '\u{2060}', '\u{feff}', '\u{ad}'] {
        for groups in [["XY", "56", "ZT", "78"], ["VR", "23", "QP", "91"]] {
            let value = groups.join(&separator.to_string());
            let input = format!("Documentation only = {value}. Done.");
            let session = Session::new(Scope::Ephemeral).unwrap();
            assert_eq!(
                support::clean(
                    pipeline(),
                    &session,
                    &input,
                    &[LocaleTag::EnUs, LocaleTag::Global]
                ),
                input
            );
            assert!(session.snapshot_entries().is_empty());
        }
        // Interior formatting in a complete postcode is still protected.
        assert_exact_token(
            "Postcode: ",
            &format!("XY{separator}56{separator}ZT"),
            ". Done.",
        );
    }
}

#[test]
fn invalid_international_phone_prefix_does_not_license_a_national_suffix() {
    for separator in ['-', '\u{2011}'] {
        let value = ["+0", "212", "555", "0122"].join(&separator.to_string());
        let input = format!("Public format fixture = {value}. Not assigned.");
        let session = Session::new(Scope::Ephemeral).unwrap();
        assert_eq!(
            support::clean(
                pipeline(),
                &session,
                &input,
                &[LocaleTag::EnUs, LocaleTag::Global]
            ),
            input
        );
        assert!(session.snapshot_entries().is_empty());
        assert_exact_token(
            "Phone: ",
            &["+1", "212", "555", "0122"].join(&separator.to_string()),
            ". Done.",
        );
        assert_exact_token(
            "Phone: ",
            &["212", "555", "0122"].join(&separator.to_string()),
            ". Done.",
        );
    }
}

#[test]
fn hyphen_grouped_ibans_keep_the_complete_value_protected() {
    for separator in ['-', '\u{2011}'] {
        let value = ["NL82", "ABNA", "8266", "3896", "21"].join(&separator.to_string());
        assert_exact_token("{\"result\":{\"IBAN number\":\"", &value, "\"}}");
        let value = ["GB82", "WEST", "1234", "5698", "7654", "32"].join(&separator.to_string());
        assert_exact_token("IBAN: ", &value, "; end.");
    }
}
