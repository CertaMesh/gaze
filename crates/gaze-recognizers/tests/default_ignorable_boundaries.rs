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

// Unicode 17.0 DerivedCoreProperties.txt, Default_Ignorable_Code_Point.
// Independent of the implementation predicate; every removed scalar is exercised.
fn all_separators() -> impl Iterator<Item = char> {
    [
        (0xAD, 0xAD),
        (0x34F, 0x34F),
        (0x61C, 0x61C),
        (0x115F, 0x1160),
        (0x17B4, 0x17B5),
        (0x180B, 0x180F),
        (0x200B, 0x200F),
        (0x202A, 0x202E),
        (0x2060, 0x206F),
        (0x3164, 0x3164),
        (0xFE00, 0xFE0F),
        (0xFEFF, 0xFEFF),
        (0xFFA0, 0xFFA0),
        (0xFFF0, 0xFFF8),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0000, 0xE0FFF),
    ]
    .into_iter()
    .flat_map(|(start, end)| start..=end)
    .filter_map(char::from_u32)
}

fn separators() -> Vec<char> {
    let all = all_separators().collect::<Vec<_>>();
    match std::env::var("GAZE_SEPARATOR_PROBE") {
        Ok(codepoint) => {
            let ch = char::from_u32(u32::from_str_radix(&codepoint, 16).unwrap()).unwrap();
            assert!(all.contains(&ch));
            vec![ch]
        }
        Err(_) => all,
    }
}

#[test]
fn unicode_default_ignorable_inventory_has_expected_size() {
    assert_eq!(all_separators().count(), 4174);
}

const VALUES: [&str; 5] = [
    "4111111111111111",
    "GB82WEST12345698765432",
    "0x0000000000000000000000000000000000000000",
    "cedar@example.invalid",
    "+12025550100",
];

fn assert_protected(input: &str, value: &str) {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let clean = support::clean(
        pipeline(),
        &session,
        input,
        &[LocaleTag::EnUs, LocaleTag::Global],
    );
    assert!(
        session
            .snapshot_entries()
            .iter()
            .any(|entry| entry.raw.contains(value)),
        "value bytes escaped full protection: {input:?} -> {clean:?}"
    );
    assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
}

#[test]
fn every_default_ignorable_preserves_value_boundaries() {
    for sep in separators() {
        for value in VALUES {
            assert_protected(&format!("word{sep}{value}; done"), value);
            assert_protected(&format!("Value: {value}{sep}word; done"), value);
        }
    }
}

#[test]
fn every_default_ignorable_inside_values_keeps_all_source_bytes() {
    for sep in separators() {
        for (cue, value, at) in [
            ("Card: ", VALUES[0], 4),
            ("IBAN: ", VALUES[1], 8),
            ("Wallet: ", VALUES[2], 12),
            ("Mail: ", VALUES[3], 3),
            ("Phone: ", VALUES[4], 5),
        ] {
            let value = format!("{}{sep}{}", &value[..at], &value[at..]);
            assert_protected(&format!("{cue}{value}; done"), &value);
        }
    }
}

#[test]
fn legitimate_default_ignorables_remain_byte_exact_without_tokens() {
    for input in [
        "Emoji: ❤️; family 👩\u{200d}👩\u{200d}👧\u{200d}👦; scientist 👩\u{200d}🔬.",
        "Flag: 🏴\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}.",
        "Persian: می\u{200c}روم and خانه\u{200c}ها.",
        "Arabic: ب\u{200d}ب and ب\u{200c}ب.",
        "Indic: क्\u{200d}ष and क्\u{200c}ष; ന്\u{200d}.",
        "RTL: \u{200f}مرحبا\u{200e}; \u{202b}שלום\u{202c}; \u{2067}سلام\u{2069}.",
        "const label = \"co\u{34f}operative\";\n",
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
fn mixed_outer_and_inner_separators_keep_complete_values_protected() {
    for sep in ['\u{200b}', '\u{034f}', '\u{2067}'] {
        for (value, at) in VALUES.into_iter().zip([4, 8, 12, 3, 5]) {
            let value = format!("{}{sep}{}", &value[..at], &value[at..]);
            assert_protected(&format!("word{sep}{value}{sep}word; done"), &value);
        }
    }
}
