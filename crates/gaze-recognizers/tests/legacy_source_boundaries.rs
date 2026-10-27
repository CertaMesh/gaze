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
