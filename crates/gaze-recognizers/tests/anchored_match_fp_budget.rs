use std::{cell::Cell, collections::HashMap};

use gaze_types::{DetectContext, DictionaryBundle, LocaleTag, PiiClass, Recognizer};

use gaze_recognizers::{
    AnchoredBoundary, AnchoredMatchRecognizer, CuePosition, NameShape, RegexDetector,
};

fn ctx() -> DetectContext<'static> {
    let fields = Box::leak(Box::new(()));
    let dictionaries = Box::leak(Box::new(DictionaryBundle::from_entries(HashMap::new())));
    let locale_chain = Box::leak(Box::new([
        LocaleTag::DeDe,
        LocaleTag::EnUs,
        LocaleTag::Global,
    ]));
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
                "Begin forwarded message".to_string(),
                "----- Forwarded message".to_string(),
            ],
            AnchoredBoundary::Punctuation,
            64,
            NameShape::PersonName,
            CuePosition::Before,
            "forward_marker".to_string(),
            1,
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
            2,
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
            2,
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

fn name_count(input: &str) -> usize {
    let ctx = ctx();
    let anchored = anchored_recognizers()
        .iter()
        .flat_map(|recognizer| recognizer.detect(input, &ctx))
        .count();
    anchored + paren_header_name_recognizer().detect(input, &ctx).len()
}

#[test]
fn p6_anchored_match_false_positive_budget_stays_within_limit() {
    let fixtures = [
        // R2-Patch-10: German prose negatives stress open cue fragments.
        ("Ich denke an Alice gestern.", 0),
        ("Wir gehen an Bob vorbei.", 0),
        ("Er schreibst seinen Bericht.", 0),
        ("Sie antworten freundlich.", 0),
        // R2-Patch-10: English prose negatives stress forward and recipient cues.
        ("Hannah forwarded message details to me.", 0),
        ("Reply to your message tomorrow.", 0),
        ("Sent by Mailgun via Outlook.", 0),
        ("Forwarded message arrived.", 0),
        // R2-Patch-10 / R2-Patch-15: acknowledged v0.6 code-fence limit.
        ("```\nFrom: alice@example.invalid (Alice Example)\n```", 1),
        ("```\nCc: bob@example.invalid (Bob B)\n```", 1),
        // R2-Patch-10: brand-shaped names have no preceding cue.
        ("Acme Corp announced earnings.", 0),
        ("Microsoft Teams crashed today.", 0),
    ];

    let total_name_detections = fixtures
        .iter()
        .map(|(fixture, expected)| {
            let actual = name_count(fixture);
            assert_eq!(actual, *expected, "{fixture}");
            actual
        })
        .sum::<usize>();

    assert!(
        total_name_detections <= 2,
        "FP budget exceeded: {total_name_detections} Name detections"
    );
}
