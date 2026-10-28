mod support;

use gaze::{Action, DefaultRule, LocaleTag, Pipeline, Scope, Session};
use gaze_recognizers::RegexDetector;

#[test]
fn legacy_regex_detectors_keep_glued_and_internal_source_boundaries() {
    let pipeline = Pipeline::builder()
        .detector(
            RegexDetector::new(
                r"\b[0-9]{16}\b",
                gaze::PiiClass::custom("credit_card").unwrap(),
            )
            .unwrap(),
        )
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    for sep in [
        '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{00ad}', '\u{feff}', '\u{034f}',
    ] {
        for value in [
            "4111111111111111".to_owned(),
            format!("4111{sep}111111111111"),
        ] {
            let prefix = format!("word{sep}");
            let suffix = format!("{sep}word;");
            let raw = format!("{prefix}{value}{suffix}");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let clean = support::clean(&pipeline, &session, &raw, &[LocaleTag::Global]);
            assert_eq!(
                gaze::token_shape::pattern().replace_all(&clean, "\0"),
                format!("{prefix}\0{suffix}"),
                "{raw:?}"
            );
            assert_eq!(session.snapshot_entries().len(), 1);
            assert_eq!(session.snapshot_entries()[0].raw, value);
            assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
        }
    }
}

#[test]
fn adopter_ignorable_literals_and_class_members_keep_matching() {
    for sep in ['\u{200b}', '\u{ad}', '\u{fe0f}', '\u{e0067}'] {
        for pattern in [
            format!(r"ID{sep}\d{{6}}\b"),
            format!(r"ID[{sep}_]\d{{6}}\b"),
            format!(r"ID[\u{{{:x}}}_]\d{{6}}\b", sep as u32),
            format!(
                r"ID[\u{{{:x}}}-\u{{{:x}}}]\d{{6}}\b",
                sep as u32 - 1,
                sep as u32 + 1
            ),
            format!(r"ID[[{sep}_]&&[^{0}]]\d{{6}}\b", '_'),
        ] {
            let pipeline = Pipeline::builder()
                .detector(
                    RegexDetector::new(&pattern, gaze::PiiClass::custom("reference").unwrap())
                        .unwrap(),
                )
                .rule(DefaultRule::new(Action::Tokenize))
                .build()
                .unwrap();
            let input = format!("ID{sep}123456");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let clean = support::clean(&pipeline, &session, &input, &[LocaleTag::Global]);
            assert_eq!(
                session.snapshot_entries().len(),
                1,
                "{pattern:?}: {clean:?}"
            );
            assert_eq!(session.snapshot_entries()[0].raw, input);
            assert_eq!(session.restore_strict_text(&clean).unwrap(), input);
        }
    }
}

#[test]
fn adopter_ignorable_edge_literals_match_without_interior_gaps() {
    for sep in ['\u{200b}', '\u{ad}', '\u{fe0f}', '\u{e0067}'] {
        for (pattern, prefix, suffix) in [
            (format!(r"^{sep}ID\d{{6}}$"), sep.to_string(), String::new()),
            (format!(r"^ID\d{{6}}{sep}$"), String::new(), sep.to_string()),
            (
                format!(r"^[{sep}_]ID\d{{6}}[{sep}_]$"),
                sep.to_string(),
                sep.to_string(),
            ),
        ] {
            let pipeline = Pipeline::builder()
                .detector(
                    RegexDetector::new(&pattern, gaze::PiiClass::custom("reference").unwrap())
                        .unwrap(),
                )
                .rule(DefaultRule::new(Action::Tokenize))
                .build()
                .unwrap();
            let raw = format!("{prefix}ID123456{suffix}");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let clean = support::clean(&pipeline, &session, &raw, &[LocaleTag::Global]);
            assert_eq!(
                gaze::token_shape::pattern().replace_all(&clean, "\0"),
                format!("{prefix}\0{suffix}"),
                "{pattern:?}"
            );
            assert_eq!(session.snapshot_entries()[0].raw, "ID123456");
            assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
        }
    }
}

#[test]
fn hard_boundary_marker_only_captures_do_not_hide_real_values() {
    let detector = RegexDetector::with_rulepack_fields(
        r#"field: "([^"]+)""#,
        gaze::PiiClass::custom("reference").unwrap(),
        "reference.field",
        vec![LocaleTag::Global],
        0.9,
        50,
        "reference",
        Some(vec![1]),
        Vec::new(),
        None,
        None,
    )
    .unwrap();
    let pipeline = Pipeline::builder()
        .detector(detector)
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    for sep in [
        '\u{ad}', '\u{34f}', '\u{61c}', '\u{115f}', '\u{17b4}', '\u{180e}', '\u{200b}',
        '\u{200c}', '\u{200d}', '\u{202a}', '\u{2060}', '\u{3164}', '\u{fe0f}', '\u{feff}',
        '\u{ffa0}', '\u{fff0}', '\u{1bca0}', '\u{1d173}', '\u{e0067}',
    ] {
        let empty = format!("field: \"{sep}{sep}\"");
        let value = format!("cedar{sep}birch");
        for tail in [String::new(), format!("; field: \"{value}\"")] {
            let raw = format!("{empty}{tail}");
            let session = Session::new(Scope::Ephemeral).unwrap();
            let clean = support::clean(&pipeline, &session, &raw, &[LocaleTag::Global]);
            if tail.is_empty() {
                assert_eq!(clean, raw);
                assert!(session.snapshot_entries().is_empty());
            } else {
                assert_eq!(
                    gaze::token_shape::pattern().replace_all(&clean, "\0"),
                    format!("{empty}; field: \"\0\"")
                );
                assert_eq!(session.snapshot_entries().len(), 1);
                assert_eq!(session.snapshot_entries()[0].raw, value);
            }
            assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
        }
    }
}
