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
    let clean = support::clean(
        pipeline(),
        &session,
        &input,
        &[LocaleTag::EnUs, LocaleTag::Global],
    );
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
    // IBAN's own syntax permits spaces, not hyphens; folding to '-' must not
    // silently widen that recognizer's contract.
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
fn gap_neighbours_do_not_hide_or_extend_validated_values() {
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
                assert_exact_token(&prefix, value, &format!("{sep}9 ok"));
            }
        }
        assert_exact_token("Phone: ", "+44 7700 900123", &format!("{sep}2024 ok"));
    }
}

#[test]
fn split_digit_runs_do_not_manufacture_bare_postal_codes() {
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
        assert!(
            !session
                .snapshot_entries()
                .iter()
                .any(|entry| entry.class == gaze::PiiClass::custom("postal_code").unwrap()),
            "{input:?} -> {clean:?}"
        );
        assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
    }
}
