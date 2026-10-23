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
