use std::{cell::Cell, collections::HashMap};

use gaze_types::{DetectContext, DictionaryBundle, LocaleTag, PiiClass, Recognizer};
use tempfile::tempdir;

use gaze_recognizers::{
    AnchoredBoundary, AnchoredMatchRecognizer, CuePosition, NameShape, NerOptions, NerRecognizer,
    RegexDetector,
};

fn ctx() -> DetectContext<'static> {
    let fields = Box::leak(Box::new(()));
    let dictionaries = Box::leak(Box::new(DictionaryBundle::from_entries(HashMap::new())));
    let locale_chain = Box::leak(Box::new([LocaleTag::DeDe, LocaleTag::Global]));
    DetectContext {
        locale_chain,
        dictionaries,
        fields,
        degraded: Cell::new(false),
    }
}

fn anchored_recognizers() -> Vec<AnchoredMatchRecognizer> {
    vec![
        AnchoredMatchRecognizer::new(
            "name.forward_marker".to_string(),
            vec![
                "Forwarded message from".to_string(),
                "Begin forwarded message from".to_string(),
                "----- Forwarded message".to_string(),
            ],
            AnchoredBoundary::Punctuation,
            64,
            NameShape::PersonName,
            CuePosition::Before,
            "forward_marker".to_string(),
            0.88,
            110,
        ),
        AnchoredMatchRecognizer::new(
            "name.agent_recipient".to_string(),
            vec![
                "antwortest".to_string(),
                "antworte".to_string(),
                "schreibst".to_string(),
                "reply to".to_string(),
                "respond to".to_string(),
                "draft for".to_string(),
                "draft to".to_string(),
            ],
            AnchoredBoundary::Punctuation,
            48,
            NameShape::PersonName,
            CuePosition::Before,
            "agent_recipient".to_string(),
            0.88,
            110,
        ),
        AnchoredMatchRecognizer::new(
            "name.auto_footer".to_string(),
            vec![
                "Gesendet von".to_string(),
                "Gesendet durch".to_string(),
                "Sent by".to_string(),
                "Sent via".to_string(),
                "On behalf of".to_string(),
            ],
            AnchoredBoundary::Whitespace,
            64,
            NameShape::PersonName,
            CuePosition::Before,
            "auto_footer".to_string(),
            0.88,
            110,
        ),
    ]
}

fn paren_header_name_recognizer() -> RegexDetector {
    RegexDetector::with_rulepack_fields(
        r"(?:(?:From|To|Cc|Bcc|Reply-To|Sender|Von|An|Antwort-An|Absender):|,)\s*\S+@\S+\s*\(([^)]+)\)",
        PiiClass::Name,
        "email.header.name.paren",
        vec![LocaleTag::Global],
        0.85,
        100,
        "name.counter",
        Some(vec![1]),
        Vec::new(),
        None,
        None,
    )
    .expect("paren header recognizer")
}

fn email_recognizer() -> RegexDetector {
    RegexDetector::with_rulepack_fields(
        r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b",
        PiiClass::Email,
        "email.global",
        vec![LocaleTag::Global],
        0.70,
        90,
        "email.counter",
        None,
        Vec::new(),
        None,
        None,
    )
    .expect("email recognizer")
}

fn anchored_name_count(input: &str) -> usize {
    let ctx = ctx();
    anchored_recognizers()
        .iter()
        .flat_map(|recognizer| recognizer.detect(input, &ctx))
        .count()
}

#[test]
fn p6_context_sensitivity_matrix_locks_expected_verdicts() {
    let ctx = ctx();

    // Synthesis matrix row 1 / R2-Patch-18: NER baseline remains able to catch
    // the natural-prose standalone-name fixture without anchored-cue help.
    let ner_dir = tempdir().unwrap();
    let model_dir = ner_dir.path().join("__gaze_test_fixed_ner");
    std::fs::create_dir(&model_dir).unwrap();
    let ner = NerRecognizer::load_with_options(
        &model_dir,
        NerOptions {
            threshold: 0.30,
            locale: Some("de".to_string()),
        },
    )
    .expect("test-support NER recognizer");
    let row1 = "Hallo, hier ist Alice. Wie geht's?";
    assert_eq!(ner.detect(row1, &ctx).len(), 1);
    assert_eq!(anchored_name_count(row1), 0);

    // Synthesis matrix row 2 / R2-Patch-18: no Subject/Re anchor in v0.6.
    assert_eq!(
        anchored_name_count("Re: Order 12345 - Status update from Alice Example"),
        0
    );

    // Synthesis matrix row 3 / R2-Patch-8: auto-footer cue catches the sender
    // while single-component and organization-shaped suffixes stay out.
    let row3 = anchored_recognizers()[2].detect(
        "Sent by Alice Example via Mailgun on behalf of Acme Corp",
        &ctx,
    );
    assert_eq!(row3.len(), 1);
    assert_eq!(row3[0].source, "structural.auto_footer");

    // Synthesis matrix row 4 / R2-Patch-11: GH#24 headline prompt preamble.
    let row4 = anchored_recognizers()[1]
        .detect("Du antwortest als Artistfy-Support an Alice Example.", &ctx);
    assert_eq!(row4.len(), 1);
    assert_eq!(row4[0].source, "structural.agent_recipient");

    // Synthesis matrix row 5 / R2-Patch-9: paren-form email display names,
    // including multi-address headers, emit one Name per parenthesized display.
    let paren = paren_header_name_recognizer();
    assert_eq!(
        paren
            .detect("From: alice@example.invalid (Alice Example)", &ctx)
            .len(),
        1
    );
    assert_eq!(
        paren
            .detect(
                "From: bob@example.invalid (Bob B), alice@example.invalid (Alice A)",
                &ctx
            )
            .len(),
        2
    );

    // Synthesis matrix row 6 / R2-Patch-13: single forward-marker path.
    let row6 = anchored_recognizers()[0].detect("Forwarded message from Alice Example:", &ctx);
    assert_eq!(row6.len(), 1);
    assert_eq!(row6[0].source, "structural.forward_marker");

    // Synthesis matrix row 7 / R2-Patch-18: local-parts are not fabricated
    // into Name candidates; only Email candidates are present.
    let row7 = "Cc: bob@example.invalid, alice@example.invalid";
    assert_eq!(paren.detect(row7, &ctx).len(), 0);
    assert_eq!(anchored_name_count(row7), 0);
    assert_eq!(email_recognizer().detect(row7, &ctx).len(), 2);

    // Synthesis matrix row 8 / R2-Patch-18: no structural cue in v0.6.
    assert_eq!(
        anchored_name_count("Schedule a call with Alice next Tuesday"),
        0
    );
}
