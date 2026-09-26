//! Independently authored synthetic field records, derived from the declared grammar.
//! No evaluation rows, models, or copied production regexes are used here.
//! `password.field` ships in the opt-in `secrets` bundle, so every pipeline here loads
//! `core` plus `secrets` explicitly.

use gaze::{DictionaryBundle, LocaleTag, SafetyNetPolicy, Scope, Session};
use gaze_assembly::CorePipelineConfig;

#[test]
fn actual_core_captures_declared_values_and_restores_original_bytes() {
    let core = core_with_secrets().unwrap();
    for (raw, value, source) in [
        ("DOB: 1990-02-03", "1990-02-03", "birth_date.cue"),
        ("password: x", "x", "password.field"),
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

fn core_with_secrets() -> Result<gaze_assembly::CorePipeline, gaze_assembly::BuildError> {
    CorePipelineConfig::new()
        .with_bundled_rulepack("secrets")
        .build()
}

fn field_detector(id: &str) -> gaze_recognizers::RegexDetector {
    let spec = ["core", "secrets"]
        .into_iter()
        .flat_map(|bundle| {
            gaze::Rulepack::load(gaze::RulepackSource::Embedded(
                gaze_recognizers::embedded(bundle).unwrap(),
            ))
            .unwrap()
            .recognizers
        })
        .find(|r| r.id == id)
        .unwrap();
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
        // Two-digit years and month names joined the grammar with #3651.
        "03.02.90",
        "February 3 1990",
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
        "password: x\r",
        "password: x\rpassword: y",
        "password: a\\b",
        "password: a'b",
        "prefixpassword: x",
        "reborn on 1990-02-03",
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
    for id in ["password.field"] {
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

fn assert_source_capture(pipeline: &gaze::Pipeline, raw: &str, captured: &str, source: &str) {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    let gaze::CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(spans.len(), 1, "{raw:?}: {spans:?}");
    let span = &spans[0];
    assert_eq!(&raw[span.raw_span.clone()], captured, "{raw:?}");
    assert_eq!(
        session.restore(&clean[span.clean_span.clone()]).as_deref(),
        Some(captured)
    );
    assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
    assert_eq!(&clean[..span.clean_span.start], &raw[..span.raw_span.start]);
    assert_eq!(&clean[span.clean_span.end..], &raw[span.raw_span.end..]);
    assert_eq!(trace.len(), 1);
    assert!(trace[0].source_ids().contains(&source.to_string()));
    let expected_class = if source == "email.global" {
        gaze::PiiClass::Email
    } else {
        gaze::PiiClass::custom(source.split('.').next().unwrap()).unwrap()
    };
    assert_eq!(span.class, expected_class);
    assert_eq!(trace[0].class(), &span.class);
    assert_eq!(trace[0].raw_start()..trace[0].raw_end(), span.raw_span);
    assert_eq!(trace[0].action(), "tokenize");
}

#[test]
fn actual_assembly_normalization_exposes_source_boundaries_and_raw_size_limitations() {
    let core = core_with_secrets().unwrap();
    for (raw, captured) in [
        ("password: \"\u{200d}a\"", "a"),
        ("password: \"a\u{200c}\"", "a"),
        ("password: \"a\u{200d}\u{200c}b\"", "a\u{200d}\u{200c}b"),
        ("password: \"e\u{301}\"", "e\u{301}"),
        ("ｐａｓｓｗｏｒｄ： ＂ａ＼＂ｂ＼＼ｃ＂", "ａ＼＂ｂ＼＼ｃ"),
        ("password: 'a\\'b\\\\c'", "a\\'b\\\\c"),
        ("password: \"a\\\"b\\\\c\"", "a\\\"b\\\\c"),
        (
            "password: \"nur ein erfundenes Kennwort!\"\r\n",
            "nur ein erfundenes Kennwort!",
        ),
        ("password: required", "required"),
    ] {
        assert_source_capture(core.pipeline(), raw, captured, "password.field");
    }
    // Removed interior scalars have no raw-size ceiling, despite two grammar units.
    let captured = format!("a{}b", "\u{200d}".repeat(1024));
    assert_source_capture(
        core.pipeline(),
        &format!("password: \"{captured}\""),
        &captured,
        "password.field",
    );
    let raw = "password: \"\u{200c}\u{200d}\"";
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _) = core
        .pipeline()
        .clean_with_safety_net(
            &session,
            gaze::RawDocument::Text(raw.into()),
            &[LocaleTag::Global],
        )
        .unwrap();
    assert!(spans.is_empty());
    let gaze::CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(clean, raw);

    for units in [255, 256, 257] {
        for value in [
            "試".repeat(units),
            "\\\"".repeat(units),
            (0..units)
                .map(|i| if i % 2 == 0 { "é" } else { "\\\\" })
                .collect::<String>(),
        ] {
            let raw = format!("password: \"{value}\"");
            if units <= 256 {
                assert_source_capture(core.pipeline(), &raw, &value, "password.field");
            } else {
                let session = Session::new(Scope::Ephemeral).unwrap();
                let (clean, spans, _) = core
                    .pipeline()
                    .clean_with_safety_net(
                        &session,
                        gaze::RawDocument::Text(raw.clone()),
                        &[LocaleTag::Global],
                    )
                    .unwrap();
                assert!(spans.is_empty(), "no 257-unit prefix");
                let gaze::CleanDocument::Text(clean) = clean else {
                    panic!("text")
                };
                assert_eq!(clean, raw);
            }
        }
    }
}

fn assembled(
    rules: Vec<gaze::RuleSpec>,
    competitors: &[(&str, gaze::PiiClass, i32)],
) -> gaze::Pipeline {
    let mut pack = gaze::Rulepack::load(gaze::RulepackSource::Embedded(
        gaze_recognizers::embedded("core").unwrap(),
    ))
    .unwrap();
    pack.recognizers.extend(
        gaze::Rulepack::load(gaze::RulepackSource::Embedded(
            gaze_recognizers::embedded("secrets").unwrap(),
        ))
        .unwrap()
        .recognizers,
    );
    // Competing synthetic candidates exercise real assembly/arbitration without a model.
    for (index, (pattern, class, priority)) in competitors.iter().enumerate() {
        let mut spec = pack
            .recognizers
            .iter()
            .find(|r| r.id == "password.field")
            .unwrap()
            .clone();
        spec.id = format!("synthetic.competitor.{index}");
        spec.class = class.clone();
        spec.matcher = gaze::RawMatch::Regex {
            pattern: Some(pattern.to_string()),
            pattern_template: None,
            capture_groups: None,
        };
        spec.scoring.priority = *priority;
        pack.recognizers.push(spec);
    }
    let mut policy = gaze::Policy::default();
    policy.rules = rules;
    let context = gaze::Context {
        dictionaries: Default::default(),
        class_map: Default::default(),
        fields: Default::default(),
    };
    gaze_assembly::build_pipeline(
        &policy,
        &context,
        &[pack],
        &gaze::LocaleChain::from(&[LocaleTag::Global][..]),
        None,
    )
    .unwrap()
}

fn tokenize_rules() -> Vec<gaze::RuleSpec> {
    vec![gaze::RuleSpec::Default {
        action: gaze::Action::Tokenize,
    }]
}

#[test]
fn actual_assembly_nested_builtin_and_custom_fragments_keep_full_field_source() {
    use gaze::PiiClass;
    let core = core_with_secrets().unwrap();
    for value in [
        "prefix alice@example.invalid suffix",
        "prefix +49 1555 0112233 suffix",
        "prefix 192.0.2.1 suffix",
        "prefix 2001:db8::1 suffix",
        "prefix secret: SYNTHETICabcdef012345 suffix",
    ] {
        assert_source_capture(
            core.pipeline(),
            &format!("password: \"{value}\""),
            value,
            "password.field",
        );
    }
    for class in [
        PiiClass::Name,
        PiiClass::Location,
        PiiClass::Email,
        PiiClass::custom("phone").unwrap(),
        PiiClass::custom("ip").unwrap(),
        PiiClass::custom("security_token").unwrap(),
        PiiClass::custom("username").unwrap(),
    ] {
        for reverse in [false, true] {
            let mut competitors = vec![
                ("inside", class.clone(), 87),
                ("side", PiiClass::custom("fragment").unwrap(), 80),
            ];
            if reverse {
                competitors.reverse();
            }
            let pipeline = assembled(tokenize_rules(), &competitors);
            assert_source_capture(
                &pipeline,
                "password: \"left inside right\"",
                "left inside right",
                "password.field",
            );
        }
    }
}

#[test]
fn same_span_email_winner_selects_email_policy_instead_of_password_policy() {
    use gaze::{Action, PiiClass, RuleSpec};
    let raw = "password: alice@example.invalid";
    let rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("password").unwrap(),
            action: Action::Preserve,
        },
        RuleSpec::Class {
            class: PiiClass::Email,
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    let pipeline = assembled(rules, &[]);
    assert_source_capture(&pipeline, raw, "alice@example.invalid", "email.global");
    let reverse_rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("password").unwrap(),
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: PiiClass::Email,
            action: Action::Preserve,
        },
        RuleSpec::Default {
            action: Action::Tokenize,
        },
    ];
    // The email still wins the same-span overlap and selects the email
    // policy (`preserve`), but the bytes are also a password-field value the
    // policy tokenizes: protection beats preservation, so the value leaves as
    // one `custom:password` residual fragment inside the preserved winner
    // (todo #3740). Before that rule the preserved email shipped the value raw.
    let pipeline = assembled(reverse_rules, &[]);
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _) = pipeline
        .clean_with_safety_net(
            &session,
            gaze::RawDocument::Text(raw.into()),
            &[LocaleTag::Global],
        )
        .unwrap();
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert!(spans[0].origin.is_residual_fragment());
    assert_eq!(spans[0].class, PiiClass::custom("password").unwrap());
    assert_eq!(&raw[spans[0].raw_span.clone()], "alice@example.invalid");
    let gaze::CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert!(clean.starts_with("password: <"), "{clean}");
    assert!(clean.contains(":Custom:password_"), "{clean}");
    assert!(!clean.contains("alice"), "{clean}");
    assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
}

#[derive(Clone)]
struct CapturedLogs(std::sync::Arc<std::sync::Mutex<Vec<gaze::RedactionEntry>>>);
impl gaze::RedactionLogger for CapturedLogs {
    fn log(&self, entry: &gaze::RedactionEntry) -> Result<(), gaze::RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

#[test]
fn validator_veto_belongs_to_card_or_phone_not_declared_password() {
    for (value, veto_id) in [
        ("4111-1111-1111-1112", "card.structural"),
        ("+99999999", "phone"),
    ] {
        let logs = CapturedLogs(Default::default());
        let core = core_with_secrets()
            .unwrap()
            .into_pipeline()
            .with_redaction_logger(logs.clone());
        assert_source_capture(
            &core,
            &format!("password: \"{value}\""),
            value,
            "password.field",
        );
        let rows = logs.0.lock().unwrap();
        assert!(rows
            .iter()
            .any(|r| r.recognizer_id.as_deref() == Some("password.field")
                && r.validator_fail_reason.is_none()
                && !r.conflict_loser));
        assert!(
            rows.iter().any(|r| r
                .recognizer_id
                .as_deref()
                .is_some_and(|id| id.contains(veto_id))
                && r.validator_fail_reason.is_some()),
            "own veto missing: {rows:?}"
        );
    }
    for value in ["4111-1111-1111-1111", "+49 1555 0112233"] {
        let core = core_with_secrets().unwrap();
        assert_source_capture(
            core.pipeline(),
            &format!("password: \"prefix {value} suffix\""),
            &format!("prefix {value} suffix"),
            "password.field",
        );
    }
}

#[test]
fn builtin_container_and_custom_competitor_order_preserve_existing_arbitration() {
    for reverse in [false, true] {
        let mut rivals = vec![
            (r#"password: "left right""#, gaze::PiiClass::Name, 0),
            ("left", gaze::PiiClass::custom("username").unwrap(), 87),
        ];
        if reverse {
            rivals.reverse();
        }
        let pipeline = assembled(tokenize_rules(), &rivals);
        let raw = "password: \"left right\"";
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, spans, _, trace) = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                raw,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        let gaze::CleanDocument::Text(clean) = clean else {
            panic!("text")
        };
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].raw_span, 0..raw.len());
        assert_eq!(spans[0].class, gaze::PiiClass::Name);
        assert!(trace[0].source_ids().contains(&"password.field".into()));
        assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
    }
}

#[derive(Clone, Copy)]
enum NetResponse {
    Owned,
    Residual,
    Error,
}
struct FieldNet {
    response: NetResponse,
    seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}
impl gaze::SafetyNet for FieldNet {
    fn id(&self) -> &str {
        "synthetic.field.net"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        ctx: gaze::SafetyNetContext<'_>,
    ) -> Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
        self.seen.lock().unwrap().push(text.into());
        let (span, class) = match self.response {
            NetResponse::Error => {
                return Err(gaze::SafetyNetError::Runtime {
                    message: "synthetic failure".into(),
                })
            }
            NetResponse::Owned => {
                if let Some(span) = ctx.manifest.spans.first() {
                    (span.clean_span.clone(), span.class.clone())
                } else {
                    let start = text.find("synthetic").unwrap();
                    (
                        start..start + 9,
                        gaze::PiiClass::custom("password").unwrap(),
                    )
                }
            }
            NetResponse::Residual => (0..8, gaze::PiiClass::Name),
        };
        Ok(vec![gaze::LeakSuspect::new(
            span,
            class,
            self.id(),
            None,
            gaze::LeakKind::Uncovered,
            "synthetic",
            None,
        )])
    }
}

#[test]
fn repeated_fields_live_staged_trace_and_owned_replay_restore_exact_bytes() {
    let pipeline = core_with_secrets().unwrap().into_pipeline();
    for newline in ["\n", "\r\n"] {
        let raw =
            format!("password: synthetic{newline}password: synthetic{newline}DOB: 1990-02-03");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let (staged, spans, _) = pipeline
            .clean_transaction_with_safety_net_policy_detect_context(
                &mut tx,
                gaze::RawDocument::Text(raw.clone()),
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        let gaze::CleanDocument::Text(staged) = staged else {
            panic!("text")
        };
        assert_eq!(spans.len(), 3);
        assert_eq!(
            &staged[spans[0].clean_span.clone()],
            &staged[spans[1].clean_span.clone()]
        );
        assert_eq!(tx.tokens().len(), 2);
        assert!(session.tokens().is_empty());
        assert_eq!(tx.restore_strict_text(&staged).unwrap(), raw);
        tx.commit().unwrap();
        let (live, live_spans, _, trace) = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                &raw,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        let gaze::CleanDocument::Text(live) = live else {
            panic!("text")
        };
        assert_eq!(live, staged);
        assert_eq!(live_spans, spans);
        assert_eq!(trace.len(), 3);
        for (span, trace) in spans.iter().zip(trace) {
            assert_eq!(trace.raw_start()..trace.raw_end(), span.raw_span);
            assert_eq!(
                session.restore(&live[span.clean_span.clone()]).unwrap(),
                raw[span.raw_span.clone()]
            );
        }
        let mut replay = session.begin_transaction();
        let clean = pipeline
            .protect_text_transaction(
                &mut replay,
                &staged,
                gaze::ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default()),
            )
            .unwrap();
        assert_eq!(clean, staged);
        assert_eq!(replay.restore_strict_text(&clean).unwrap(), raw);
        assert_eq!(replay.tokens().len(), 2);
    }
}

#[test]
fn configured_fake_net_sees_final_fields_and_stage_errors_publish_nothing() {
    for response in [
        NetResponse::Owned,
        NetResponse::Residual,
        NetResponse::Error,
    ] {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let pipeline = core_with_secrets()
            .unwrap()
            .into_pipeline()
            .with_safety_net(FieldNet {
                response,
                seen: seen.clone(),
            });
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let result = pipeline.protect_text_transaction(
            &mut tx,
            "password: synthetic",
            gaze::ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default()),
        );
        assert!(session.tokens().is_empty());
        assert!(!tx.tokens().is_empty());
        match response {
            NetResponse::Owned => {
                let clean = result.unwrap();
                assert_eq!(
                    tx.restore_strict_text(&clean).unwrap(),
                    "password: synthetic"
                );
                let token = tx.tokens().into_iter().next().unwrap();
                use sha2::{Digest, Sha256};
                let canonical = token.replacen(&token[1..9], "00000000", 1);
                let mut hasher = Sha256::new();
                hasher.update(b"gaze-safety-net-token-v3\0");
                hasher.update(canonical.as_bytes());
                let prefix = hex::encode(&hasher.finalize()[..4]);
                let stable_token = token.replacen(&token[1..9], &prefix, 1);
                let expected_scan = clean.replacen(&token, &stable_token, 1);
                assert_eq!(seen.lock().unwrap().as_slice(), &[expected_scan]);
            }
            NetResponse::Residual => {
                assert!(matches!(result, Err(gaze::ProtectionError::Residual)))
            }
            NetResponse::Error => assert!(matches!(result, Err(gaze::ProtectionError::SafetyNet))),
        }
        drop(tx);
        assert!(session.tokens().is_empty());
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}

#[test]
fn caller_actions_remain_authoritative_and_strict_does_not_promise_no_new_denials() {
    use gaze::{Action, PiiClass, RuleSpec};
    let raw = "password: synthetic";
    for action in [
        Action::Preserve,
        Action::Redact,
        Action::Generalize,
        Action::FormatPreserve,
    ] {
        let pipeline = assembled(
            vec![
                RuleSpec::Class {
                    class: PiiClass::custom("password").unwrap(),
                    action,
                },
                RuleSpec::Default {
                    action: Action::Tokenize,
                },
            ],
            &[],
        );
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, spans, _) = pipeline
            .clean_with_safety_net(
                &session,
                gaze::RawDocument::Text(raw.into()),
                &[LocaleTag::Global],
            )
            .unwrap();
        let gaze::CleanDocument::Text(clean) = clean else {
            panic!("text")
        };
        match action {
            Action::Preserve => {
                assert_eq!(clean, raw);
                assert!(spans.is_empty());
            }
            Action::Redact => {
                assert_eq!(clean, "password: [REDACTED]");
                assert!(session.tokens().is_empty());
            }
            Action::Generalize => {
                assert!(!clean.contains("synthetic"));
                assert!(session.tokens().is_empty());
            }
            Action::FormatPreserve => {
                assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
                assert!(session.contains_token(&clean[spans[0].clean_span.clone()]));
            }
            _ => unreachable!(),
        }
        let trace = pipeline.clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        );
        assert_eq!(trace.is_ok(), action == Action::Preserve);
        let pipeline = pipeline.with_safety_net(FieldNet {
            response: NetResponse::Owned,
            seen: Default::default(),
        });
        let mut tx = session.begin_transaction();
        let strict = pipeline.protect_text_transaction(
            &mut tx,
            raw,
            gaze::ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default()),
        );
        match action {
            Action::Preserve => assert!(matches!(strict, Err(gaze::ProtectionError::Residual))),
            Action::Redact | Action::Generalize => {
                assert!(matches!(strict, Err(gaze::ProtectionError::Provenance)))
            }
            Action::FormatPreserve => assert!(strict.is_ok()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn foreign_token_spelling_is_not_owned_field_protection() {
    let pipeline = core_with_secrets().unwrap().into_pipeline();
    let foreign = Session::new(Scope::Ephemeral).unwrap();
    let token = foreign
        .tokenize(&gaze::PiiClass::custom("password").unwrap(), "synthetic")
        .unwrap();
    let raw = format!("password: {token}");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    assert!(!tx.contains_token(&token));
    let result = pipeline.protect_text_transaction(
        &mut tx,
        &raw,
        gaze::ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default()),
    );
    let clean = result.unwrap();
    assert!(!clean.contains(&token));
    assert!(!tx.contains_token(&token));
    assert_eq!(tx.restore_strict_text(&clean).unwrap(), raw);
    assert_eq!(tx.tokens().len(), 1);
    let local = tx.tokens().pop().unwrap();
    assert_eq!(tx.restore(&local).as_deref(), Some(token.as_str()));
    drop(tx);
    assert!(session.tokens().is_empty());
}

#[test]
fn staged_clean_net_error_requires_discard_and_never_publishes_mappings() {
    let seen = Default::default();
    let pipeline = core_with_secrets()
        .unwrap()
        .into_pipeline()
        .with_safety_net(FieldNet {
            response: NetResponse::Error,
            seen,
        });
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    let result = pipeline.clean_transaction_with_safety_net_policy_detect_context(
        &mut tx,
        gaze::RawDocument::Text("password: synthetic".into()),
        &[LocaleTag::Global],
        &DictionaryBundle::default(),
        SafetyNetPolicy::default(),
    );
    assert!(result.is_err());
    assert_eq!(tx.tokens().len(), 1);
    assert!(session.tokens().is_empty());
    drop(tx);
    assert!(session.tokens().is_empty());
}
