//! Independently authored synthetic field records, derived from the declared grammar.
//! No evaluation rows, models, or copied production regexes are used here.

use gaze::{DictionaryBundle, LocaleTag, SafetyNetPolicy, Scope, Session};
use gaze_assembly::CorePipelineConfig;

#[test]
fn actual_core_captures_declared_values_and_restores_original_bytes() {
    let core = CorePipelineConfig::new().build().unwrap();
    for (raw, value, source) in [
        ("DOB: 1990-02-03", "1990-02-03", "birth_date.cue"),
        ("password: x", "x", "password.field"),
        (
            "Benutzername: \"Test Nutzer\"",
            "Test Nutzer",
            "username.field",
        ),
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, spans, _, trace) = core
            .pipeline()
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                raw,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        assert_eq!(spans.len(), 1, "missing full field: {raw:?}");
        assert_eq!(trace.len(), 1);
        let span = &spans[0];
        assert_eq!(&raw[span.raw_span.clone()], value);
        assert_eq!(trace[0].source_ids(), &[source.to_string()]);
        let gaze::CleanDocument::Text(clean) = clean else {
            panic!("text")
        };
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .unwrap(),
            raw
        );
        assert_eq!(
            clean,
            format!(
                "{}{}{}",
                &raw[..span.raw_span.start],
                &clean[span.clean_span.clone()],
                &raw[span.raw_span.end..]
            )
        );
    }
}

fn field_detector(id: &str) -> gaze_recognizers::RegexDetector {
    let pack = gaze::Rulepack::load(gaze::RulepackSource::Embedded(
        gaze_recognizers::embedded("core").unwrap(),
    ))
    .unwrap();
    let spec = pack.recognizers.into_iter().find(|r| r.id == id).unwrap();
    let gaze::RawMatch::Regex {
        pattern: Some(pattern),
        capture_groups,
        ..
    } = spec.matcher
    else {
        panic!("literal regex required")
    };
    assert!(spec.validator.is_none());
    assert!(spec.normalizer.is_none());
    gaze_recognizers::RegexDetector::with_rulepack_fields(
        &pattern,
        spec.class,
        &spec.id,
        spec.locales,
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups,
        vec![],
        None,
        None,
    )
    .unwrap()
}

fn candidates(detector: &gaze_recognizers::RegexDetector, raw: &str) -> Vec<gaze::Candidate> {
    use gaze::Recognizer;
    detector
        .detect(
            raw,
            &gaze::DetectContext::new(&[LocaleTag::Global], &DictionaryBundle::default()),
        )
        .unwrap()
}

#[test]
fn exact_cues_case_quotes_and_record_boundaries() {
    for (id, cues, value) in [
        (
            "password.field",
            &["password", "passphrase", "passwort", "kennwort"][..],
            "x!42,;",
        ),
        (
            "username.field",
            &[
                "username",
                "user name",
                "login name",
                "benutzername",
                "nutzername",
                "Anmeldename",
            ][..],
            "Üser.試験",
        ),
        (
            "birth_date.cue",
            &[
                "date of birth",
                "birth date",
                "birthdate",
                "DOB",
                "Geburtsdatum",
            ][..],
            "31.02.1990",
        ),
    ] {
        let detector = field_detector(id);
        for cue in cues {
            for cue in [cue.to_lowercase(), cue.to_uppercase(), cue.to_string()] {
                for delimiter in [":", "="] {
                    for quote in ["", "'", "\""] {
                        for end in ["", "\n", "\r\n"] {
                            let raw =
                                format!(" \t{cue}\t{delimiter} {quote}{value}{quote}\t {end}");
                            let found = candidates(&detector, &raw);
                            assert_eq!(found.len(), 1, "{id}: {raw:?}");
                            assert_eq!(&raw[found[0].span.clone()], value);
                            assert_eq!(found[0].source, id);
                            assert_eq!(found[0].token_family, "counter");
                        }
                    }
                }
            }
        }
        for newline in ["\n", "\r\n"] {
            let raw = format!(
                "{}: {value}{newline}{}: {value}{newline}{}: {value}",
                cues[0], cues[0], cues[0]
            );
            assert_eq!(candidates(&detector, &raw).len(), 3, "adjacent {id}");
        }
    }
}

#[test]
fn dates_require_complete_shapes_and_suffixes() {
    let detector = field_detector("birth_date.cue");
    for value in [
        "1990-02-03",
        "3.2.1990",
        "03.02.1990",
        "31.02.1990",
        "2/3/1990",
        "12/31/1990",
        "31/12/1990",
    ] {
        for raw in [
            format!("DOB: {value}"),
            format!("Dr. Schmidt was born on {value}."),
            format!("Dr. Schmidt wurde geboren am {value}. Weiter."),
        ] {
            let found = candidates(&detector, &raw);
            assert_eq!(found.len(), 1, "{raw:?}");
            assert_eq!(&raw[found[0].span.clone()], value);
        }
    }
    for value in [
        "1990-00-03",
        "1990-13-03",
        "1990-02-32",
        "0.2.1990",
        "32.2.1990",
        "13/31/1990",
        "31/13/1990",
        "1990-02",
        "03.02.90",
        "February 3 1990",
        "1990-02-03X",
        "1990-02-03-04",
        "1990-02-03.4",
        "1990-02-03/4",
        "1990-02-030",
        "1990-02-03_foo",
    ] {
        for raw in [
            format!("DOB: {value}"),
            format!("born on {value}"),
            format!("geboren am {value}"),
        ] {
            assert!(
                candidates(&detector, &raw).is_empty(),
                "partial date: {raw:?}"
            );
        }
    }
}

#[test]
fn malformed_or_unrelated_records_add_no_field_candidate() {
    let detectors = [
        field_detector("password.field"),
        field_detector("username.field"),
        field_detector("birth_date.cue"),
    ];
    for raw in [
        "The software version is 1990-02-03.",
        "Invoice date: 03.02.1990",
        "Build: 2026-09-15",
        "The password must be long.",
        "password_policy: minimum 12 characters",
        "password: must be at least twelve characters",
        "username_pattern: [a-z]+",
        "account: 123456",
        "user count: 12",
        "Geburtsdatum format: TT.MM.JJJJ",
        "Geburtsdatum des Kunden: 03.02.1990",
        "anmeld name: synthetic",
        "user: synthetic",
        "key: synthetic",
        "pass: synthetic",
        "password:",
        "password: \"\"",
        "username: ''",
        "password: \"synthetic'",
        "password: 'synthetic\"",
        "password: \"a\nb\"",
        "password: \"a\rb\"",
        "password: \"a\"b\"",
        "password: \"a\\q\"",
        "password: \"a\\",
        "password: a\\b",
        "password: a'b",
        "prefixpassword: x",
        "reborn on 1990-02-03",
        "born on\n1990-02-03",
    ] {
        for detector in &detectors {
            assert!(
                candidates(detector, raw).is_empty(),
                "new field candidate: {raw:?}"
            );
        }
    }
    assert_eq!(candidates(&detectors[0], "password: required").len(), 1);
}

#[test]
fn grammar_units_bound_full_values_without_prefix_fallback() {
    for id in ["password.field", "username.field"] {
        let detector = field_detector(id);
        let cue = id.split('.').next().unwrap();
        for units in [255, 256, 257] {
            for quote in ['\'', '"'] {
                for parts in [
                    vec!["x".to_string()],
                    vec!["試".to_string()],
                    vec![format!("\\{quote}")],
                    vec!["\\\\".to_string()],
                    vec!["é".to_string(), format!("\\{quote}"), "\\\\".to_string()],
                ] {
                    let value: String = (0..units)
                        .map(|i| parts[i % parts.len()].as_str())
                        .collect();
                    let raw = format!("{cue}: {quote}{value}{quote}");
                    let found = candidates(&detector, &raw);
                    assert_eq!(
                        found.len(),
                        usize::from(units <= 256),
                        "{id}, {units}, {parts:?}"
                    );
                    if let Some(candidate) = found.first() {
                        assert_eq!(&raw[candidate.span.clone()], value);
                        assert!(value.chars().count() <= 512);
                    }
                }
            }
            for atom in ["x", "試"] {
                let value = atom.repeat(units);
                let raw = format!("{cue}: {value}");
                let found = candidates(&detector, &raw);
                assert_eq!(found.len(), usize::from(units <= 256));
                if let Some(candidate) = found.first() {
                    assert_eq!(&raw[candidate.span.clone()], value);
                }
            }
        }
    }
}
