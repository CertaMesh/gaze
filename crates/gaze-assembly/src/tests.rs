use super::*;
use crate::{detector_wiring::derive_source_short_label, template::lower_pattern_template};
use gaze::{
    Action, CleanDocument, ConflictTier, DetectorKind, LocaleTag, PiiClass, PolicyError,
    RawDocument, RedactionEntry, RedactionLogger, RulepackError, Scope, Session, SessionPolicy,
    SessionScope,
};
use gaze_recognizers::{
    AnchoredBoundary, AnchoredMatchRecognizer, CuePosition, NameShape, RegexDetector,
};
use std::sync::{Arc, Mutex};

fn policy() -> gaze::Policy {
    gaze::Policy {
        session: SessionPolicy {
            scope: SessionScope::Ephemeral,
            ttl_secs: None,
        },
        detectors: Vec::new(),
        dictionaries: Vec::new(),
        rules: vec![
            RuleSpec::Class {
                class: PiiClass::Name,
                action: Action::Tokenize,
            },
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
        ner: None,
        rulepacks: gaze::RulepackPolicy {
            bundled: Vec::new(),
            paths: Vec::new(),
        },
        locale: Some(vec![LocaleTag::DeDe]),
    }
}

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

fn embedded_rulepack(name: &str) -> Rulepack {
    Rulepack::load(gaze::RulepackSource::Embedded(
        gaze_recognizers::embedded(name).expect("embedded rulepack"),
    ))
    .expect("rulepack")
}

fn clean_with_rulepacks(rulepacks: &[Rulepack], locales: &[LocaleTag], input: &str) -> String {
    let mut policy = policy();
    policy.locale = Some(locales.to_vec());
    clean_with_policy_and_rulepacks(&policy, rulepacks, input)
}

fn clean_with_policy_and_rulepacks(
    policy: &gaze::Policy,
    rulepacks: &[Rulepack],
    input: &str,
) -> String {
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(policy, &empty_context(), rulepacks, &active_locales, None)
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(&session, RawDocument::Text(input.to_string()))
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    text
}

fn name_email_policy(locales: Vec<LocaleTag>) -> gaze::Policy {
    let mut policy = policy();
    policy.locale = Some(locales);
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::Name,
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: PiiClass::Email,
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy
}

#[derive(Clone, Default)]
struct MemoryLogger {
    entries: Arc<Mutex<Vec<RedactionEntry>>>,
}

impl MemoryLogger {
    fn entries(&self) -> Vec<RedactionEntry> {
        self.entries.lock().expect("entries lock").clone()
    }
}

impl RedactionLogger for MemoryLogger {
    fn log(&self, entry: &RedactionEntry) -> gaze::Result<()> {
        self.entries
            .lock()
            .expect("entries lock")
            .push(entry.clone());
        Ok(())
    }
}

fn policy_with_registered_dictionary(rules: Vec<RuleSpec>) -> gaze::Policy {
    gaze::Policy {
        session: SessionPolicy {
            scope: SessionScope::Ephemeral,
            ttl_secs: None,
        },
        detectors: vec![gaze::DetectorSpec {
            kind: DetectorKind::Dictionary,
            name: "alpha".to_string(),
            pattern: None,
            class: PiiClass::custom("foo"),
            dictionary_name: Some("dict_alpha".to_string()),
            case_sensitive: true,
            token_family: "counter".to_string(),
        }],
        dictionaries: Vec::new(),
        rules,
        ner: None,
        rulepacks: gaze::RulepackPolicy {
            bundled: Vec::new(),
            paths: Vec::new(),
        },
        locale: Some(vec![LocaleTag::Global]),
    }
}

fn context_with_alpha_override() -> Context {
    Context {
        dictionaries: std::collections::HashMap::from([(
            "dict_alpha".to_string(),
            gaze::ContextDictionary {
                terms: vec!["context-song-123".to_string()],
                case_sensitive: true,
            },
        )]),
        class_map: std::collections::HashMap::from([(
            "dict_alpha".to_string(),
            PiiClass::custom("bar"),
        )]),
        fields: serde_json::Map::new(),
    }
}

#[test]
fn t20_context_class_map_overrides_policy_dict_class() {
    let policy = policy_with_registered_dictionary(vec![
        RuleSpec::Class {
            class: PiiClass::custom("bar"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ]);
    let context = context_with_alpha_override();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(&policy, &context, &[], &active_locales, None).expect("pipeline");
    let dictionaries = gaze::dictionary_bundle_from_context(&context);
    let fields = serde_json::Map::new();
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact_with_detect_context(
            &session,
            RawDocument::Text("track context-song-123".to_string()),
            active_locales.as_slice(),
            &dictionaries,
            &fields,
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(regex::Regex::new(r"^track <[0-9a-f]{8}:Custom:bar_\d+>$")
        .unwrap()
        .is_match(&text));
}

#[test]
fn t20a_class_map_override_fails_closed_when_action_rule_uncovered() {
    let policy = policy_with_registered_dictionary(vec![
        RuleSpec::Class {
            class: PiiClass::custom("foo"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ]);
    let context = context_with_alpha_override();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(&policy, &context, &[], &active_locales, None) {
        Ok(_) => panic!("uncovered class_map override must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::Rulepack(RulepackError::ClassMapOverrideClash {
            dict,
            old_class,
            new_class,
            ..
        }) if dict == "dict_alpha"
            && old_class == PiiClass::custom("foo")
            && new_class == PiiClass::custom("bar")
    ));
}

#[test]
fn t20b_rulepack_context_dict_override_fails_closed_when_uncovered() {
    let policy = gaze::Policy {
        session: SessionPolicy {
            scope: SessionScope::Ephemeral,
            ttl_secs: None,
        },
        detectors: Vec::new(),
        dictionaries: Vec::new(),
        rules: vec![
            RuleSpec::Class {
                class: PiiClass::custom("foo"),
                action: Action::Tokenize,
            },
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
        ner: None,
        rulepacks: gaze::RulepackPolicy {
            bundled: Vec::new(),
            paths: Vec::new(),
        },
        locale: Some(vec![LocaleTag::Global]),
    };
    let context = context_with_alpha_override();
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "tenant-rulepack"
rulepack_version = "0.4.6"
default_locales = ["global"]

[[recognizers]]
id = "tenant.alpha"
class = "custom:foo"
enabled = true

[recognizers.match]
kind = "dictionary"
terms_from_context = "dict_alpha"
case_sensitive = true
"#,
    )
    .expect("rulepack");
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(&policy, &context, &[rulepack], &active_locales, None) {
        Ok(_) => panic!("rulepack context dictionary override must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::Rulepack(RulepackError::ClassMapOverrideClash {
            dict,
            old_class,
            new_class,
            ..
        }) if dict == "dict_alpha"
            && old_class == PiiClass::custom("foo")
            && new_class == PiiClass::custom("bar")
    ));
}

#[test]
fn pattern_template_lowers_correctly_under_locale_chain_de() {
    let core = Rulepack::load(gaze::RulepackSource::Embedded(
        gaze_recognizers::embedded("core").expect("core"),
    ))
    .expect("core");
    let de = Rulepack::load(gaze::RulepackSource::Embedded(
        gaze_recognizers::embedded("locale-de").expect("locale-de"),
    ))
    .expect("de");
    let policy = policy();
    let active_locales =
        LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), Some(&[LocaleTag::DeDe]));
    let pipeline = build_pipeline(
        &policy,
        &empty_context(),
        &[core, de],
        &active_locales,
        None,
    )
    .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("Von: Dana Weber <user@example.invalid>".into()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(
        regex::Regex::new(r"^Von: <[0-9a-f]{8}:Name_\d+> <[a-z0-9._%+\-]+@example\.invalid>$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn merged_vocab_for_de_de_loads_only_locale_de_buckets() {
    let core = embedded_rulepack("core");
    let de = embedded_rulepack("locale-de");
    let en = embedded_rulepack("locale-en");
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::DeDe]), None);
    let vocab = merged_locale_vocab(&[core, de, en], &active_locales);

    let forward_markers = vocab
        .get("forward_markers")
        .expect("locale-de forward markers");
    assert_eq!(
        forward_markers,
        &vec![
            "Weitergeleitete Nachricht von".to_string(),
            "Forwarded message from".to_string(),
            "Begin forwarded message from".to_string(),
            "----- Forwarded message".to_string(),
        ]
    );
    assert!(
        !forward_markers.contains(&"Begin forwarded message".to_string()),
        "de-DE chain must not merge locale-en-only buckets; English safety cues are duplicated into locale-de deliberately"
    );
    assert_eq!(
        vocab
            .get("agent_recipient_cues")
            .expect("agent recipient cues"),
        &vec![
            "antwortest".to_string(),
            "antworte".to_string(),
            "schreibst".to_string(),
            "reply to".to_string(),
            "respond to".to_string(),
            "draft for".to_string(),
            "draft to".to_string(),
        ]
    );
}

#[test]
fn merged_vocab_for_en_us_loads_only_locale_en_buckets() {
    let core = embedded_rulepack("core");
    let de = embedded_rulepack("locale-de");
    let en = embedded_rulepack("locale-en");
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);
    let vocab = merged_locale_vocab(&[core, de, en], &active_locales);

    assert_eq!(
        vocab
            .get("forward_markers")
            .expect("locale-en forward markers"),
        &vec![
            "Forwarded message from".to_string(),
            "Begin forwarded message".to_string(),
            "----- Forwarded message".to_string(),
        ]
    );
    assert!(
        !vocab
            .get("agent_recipient_cues")
            .expect("agent recipient cues")
            .contains(&"antwortest".to_string()),
        "en-US chain must not merge locale-de cues"
    );
}

#[test]
fn markus_default_pipeline_repro_locks_in_v0_6_closure() {
    let policy = name_email_policy(vec![LocaleTag::DeDe]);
    let fixtures = [
        (
            "Von: Alice Example <alice@example.invalid>",
            r"^Von: <[0-9a-f]{8}:Name_\d+> <<[0-9a-f]{8}:Email_\d+>>$",
        ),
        (
            "From: alice@example.invalid (Alice Example)",
            r"^From: <[0-9a-f]{8}:Email_\d+> \(<[0-9a-f]{8}:Name_\d+>\)$",
        ),
        (
            "Forwarded message from Alice Example:",
            r"^Forwarded message from <[0-9a-f]{8}:Name_\d+>:$",
        ),
        (
            "Du antwortest als Artistfy-Support an Alice Example.",
            r"^Du antwortest als Artistfy-Support an <[0-9a-f]{8}:Name_\d+>\.$",
        ),
    ];
    for rulepacks in [
        vec![embedded_rulepack("core"), embedded_rulepack("locale-de")],
        vec![
            embedded_rulepack("core"),
            embedded_rulepack("locale-de"),
            embedded_rulepack("locale-en"),
        ],
    ] {
        for (input, expected) in fixtures {
            let text = clean_with_policy_and_rulepacks(&policy, &rulepacks, input);
            assert!(
                regex::Regex::new(expected).unwrap().is_match(&text),
                "{input} produced {text}"
            );
        }
    }
}

#[test]
fn paren_display_name_from_header_tokenizes_single_address() {
    // Matrix row 5 / R2-Patch-9: paren display name beside a header email.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
        &[LocaleTag::EnUs],
        "From: alice@example.invalid (Alice Example)",
    );

    assert!(
        regex::Regex::new(r"^From: alice@example\.invalid \(<[0-9a-f]{8}:Name_\d+>\)$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn paren_display_name_von_header_tokenizes_under_de_locale_chain() {
    // Matrix row 5 / R2-Patch-9: DE locale bucket lowers `Von`.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-de")],
        &[LocaleTag::DeDe],
        "Von: alice@example.invalid (Alice Example)",
    );

    assert!(
        regex::Regex::new(r"^Von: alice@example\.invalid \(<[0-9a-f]{8}:Name_\d+>\)$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn paren_display_name_from_header_tokenizes_multi_address_line() {
    // Matrix row 5 / R2-Patch-9: regex.rs uses captures_iter, so after the
    // first header-anchored address the comma arm walks the second paren form.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
        &[LocaleTag::EnUs],
        "From: bob@example.invalid (Bob B), alice@example.invalid (Alice A)",
    );

    assert!(
        regex::Regex::new(
            r"^From: bob@example\.invalid \(<[0-9a-f]{8}:Name_\d+>\), alice@example\.invalid \(<[0-9a-f]{8}:Name_\d+>\)$"
        )
        .unwrap()
        .is_match(&text)
    );
    assert_eq!(text.matches(":Name_").count(), 2);
}

#[test]
fn anchored_footer_catches_sender_name_without_org_suffix() {
    // Matrix row 3 / R2-Patch-8: max-greedy captures `Alice Example`;
    // single-component `Mailgun` and organization-shaped `Acme Corp` stay out.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
        &[LocaleTag::EnUs],
        "Sent by Alice Example via Mailgun on behalf of Acme Corp",
    );

    assert!(regex::Regex::new(
        r"^Sent by <[0-9a-f]{8}:Name_\d+> via Mailgun on behalf of Acme Corp$"
    )
    .unwrap()
    .is_match(&text));
    assert_eq!(text.matches(":Name_").count(), 1);
}

#[test]
fn anchored_agent_recipient_catches_german_prompt_preamble() {
    // Matrix row 4 / R2-Patch-11: cue is literal `antwortest`; the 48-char
    // window walks past `als Artistfy-Support an `, while the 2-component
    // minimum skips the product-role fragment and captures `Alice Example`.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-de")],
        &[LocaleTag::DeDe],
        "Du antwortest als Artistfy-Support an Alice Example.",
    );

    assert!(
        regex::Regex::new(r"^Du antwortest als Artistfy-Support an <[0-9a-f]{8}:Name_\d+>\.$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn anchored_forward_marker_catches_single_path() {
    // Matrix row 6 / R2-Patch-13: fixture is covered by anchored_match only.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
        &[LocaleTag::EnUs],
        "Forwarded message from Alice Example:",
    );

    assert!(
        regex::Regex::new(r"^Forwarded message from <[0-9a-f]{8}:Name_\d+>:$")
            .unwrap()
            .is_match(&text)
    );
    assert_eq!(text.matches(":Name_").count(), 1);
}

#[test]
fn deferred_and_not_caught_rows_do_not_emit_name_tokens() {
    for (locale, input) in [
        (LocaleTag::EnUs, "Re: ticket update from Alice Example"),
        (
            LocaleTag::EnUs,
            "Cc: bob@example.invalid, alice@example.invalid",
        ),
        (LocaleTag::EnUs, "Schedule a call with Alice next Tuesday"),
    ] {
        let text = clean_with_rulepacks(
            &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
            &[locale],
            input,
        );
        assert!(!text.contains(":Name_"), "{input} produced {text}");
    }
}

#[test]
fn same_span_structural_name_overlap_logs_loser() {
    let logger = MemoryLogger::default();
    let pipeline = gaze::Pipeline::builder()
        .recognizer(
            RegexDetector::with_rulepack_fields(
                r"reply to alice@example\.invalid \(([^)]+)\)",
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
            .expect("regex recognizer"),
        )
        .recognizer(AnchoredMatchRecognizer::new(
            "name.agent_recipient".to_string(),
            vec!["reply to alice@example.invalid".to_string()],
            AnchoredBoundary::Punctuation,
            48,
            NameShape::PersonName,
            CuePosition::Before,
            "agent_recipient".to_string(),
            2,
            0.88,
            110,
        ))
        .rule(gaze::ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(gaze::DefaultRule::new(Action::Preserve))
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("reply to alice@example.invalid (Alice Example)".to_string()),
        )
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };

    assert!(text.contains(":Name_"));
    let entries = logger.entries();
    assert_eq!(entries.len(), 2, "{entries:?}");
    let loser = entries
        .iter()
        .find(|entry| entry.conflict_loser)
        .expect("loser");
    assert_eq!(loser.source, "email.header.name.paren");
    assert_eq!(loser.decided_by, ConflictTier::Merged);
}

#[test]
fn pattern_template_preserves_regex_quantifiers() {
    let locale_vocab =
        std::collections::HashMap::from([("email_headers".to_string(), vec!["From".to_string()])]);
    let pattern = lower_pattern_template(
        "email.header.name",
        r"^(?:{locale_email_headers}): ([A-Z][a-z]+(?:\s+[A-Z][a-z]+){0,3})$",
        &locale_vocab,
    )
    .expect("lowered pattern");

    assert!(pattern.contains(r"{0,3}"));
    let regex = regex::Regex::new(&pattern).expect("compiled regex");
    let captures = regex
        .captures("From: Alice Example")
        .expect("email header captures");
    assert_eq!(captures.get(1).map(|m| m.as_str()), Some("Alice Example"));
}

#[test]
fn locale_email_headers_placeholder_is_non_capturing() {
    let locale_vocab =
        std::collections::HashMap::from([("email_headers".to_string(), vec!["Von".to_string()])]);
    let pattern = lower_pattern_template(
        "email.header.name",
        r#"^(?:{locale_email_headers}):\s*(?:"([^"]+)"|([A-Z][a-z]+(?:\s+[A-Z][a-z]+){0,3}))\s+<[^>]+>"#,
        &locale_vocab,
    )
    .expect("lowered pattern");
    let regex = regex::Regex::new(&pattern).expect("compiled regex");

    let quoted = regex
        .captures(r#"Von: "Doe, Jane" <jane@example.invalid>"#)
        .expect("quoted capture");
    assert_eq!(quoted.get(1).map(|m| m.as_str()), Some("Doe, Jane"));
    assert!(quoted.get(2).is_none());

    let bare = regex
        .captures("Von: Alice Example <alice@example.invalid>")
        .expect("bare capture");
    assert!(bare.get(1).is_none());
    assert_eq!(bare.get(2).map(|m| m.as_str()), Some("Alice Example"));
}

#[test]
fn locale_email_headers_legacy_alias_matches_bucket_syntax() {
    let locale_vocab = std::collections::HashMap::from([(
        "email_headers".to_string(),
        vec!["From".to_string(), "Reply-To".to_string()],
    )]);

    let legacy = lower_pattern_template(
        "email.header.name",
        r"^(?:{locale_email_headers}):\s+(.+)$",
        &locale_vocab,
    )
    .expect("legacy placeholder");
    let bucket = lower_pattern_template(
        "email.header.name",
        r"^(?:{locale.email_headers}):\s+(.+)$",
        &locale_vocab,
    )
    .expect("bucket placeholder");

    assert_eq!(legacy, bucket);
}

#[test]
fn locale_bucket_placeholder_lowers_neutral_bucket() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "neutral-template"
rulepack_version = "0.4.2"
default_locales = ["global"]

[locale.salutations]
names = ["Mx", "Dr"]

[[recognizers]]
id = "neutral.salutation.name"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern_template = '''(?m)^(?:{locale.salutations}):\s+([A-Z][a-z]+)$'''
capture_groups = [1]
"#,
    )
    .expect("parse");

    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    )
    .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(&session, RawDocument::Text("Mx: Schmidt".to_string()))
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(regex::Regex::new(r"^Mx: <[0-9a-f]{8}:Name_\d+>$")
        .unwrap()
        .is_match(&text));
}

#[test]
fn locale_bucket_placeholder_unknown_bucket_fails_closed() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "bad-locale-bucket"
rulepack_version = "0.4.2"
default_locales = ["global"]

[[recognizers]]
id = "bad.locale.bucket"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern_template = '''{locale.missing_bucket}: (.+)'''
"#,
    )
    .expect("parse");

    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("unknown locale bucket must fail"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::Policy(PolicyError::UnknownLocaleBucket { name })
            if name == "missing_bucket"
    ));
}

#[test]
fn pattern_template_unknown_placeholder_fails_closed() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "bad-template"
rulepack_version = "0.4.1"
default_locales = ["global"]

[[recognizers]]
id = "bad.template"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern_template = '''{unknown_placeholder}: (.+)'''
"#,
    )
    .expect("parse");

    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("unknown placeholder must fail"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        BuildError::Rulepack(RulepackError::UnknownPatternTemplatePlaceholder {
            placeholder,
            ..
        }) if placeholder == "unknown_placeholder"
    ));
}

#[test]
fn anchored_match_missing_bucket_fails_closed_with_recognizer_id() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "bad-anchored"
rulepack_version = "0.6.0"
default_locales = ["global"]

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "missing_bucket"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"
"#,
    )
    .expect("parse");
    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("missing anchored_match bucket must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::UnknownLocaleBucket {
            recognizer_id,
            bucket,
        } if recognizer_id == "name.forward_marker" && bucket == "missing_bucket"
    ));
}

#[test]
fn anchored_match_empty_bucket_fails_closed_with_recognizer_id() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "bad-anchored"
rulepack_version = "0.6.0"
default_locales = ["global"]

[locale.empty_bucket]
names = []

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "empty_bucket"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"
"#,
    )
    .expect("parse");
    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("empty anchored_match bucket must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::UnknownLocaleBucket {
            recognizer_id,
            bucket,
        } if recognizer_id == "name.forward_marker" && bucket == "empty_bucket"
    ));
}

#[test]
fn anchored_match_builds_with_constructor_owned_cues() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "anchored"
rulepack_version = "0.6.0"
default_locales = ["global"]

[locale.forward_markers]
names = ["Forwarded message from"]

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "forward_markers"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"

[recognizers.scoring]
base = 0.95
priority = 60
"#,
    )
    .expect("parse");
    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    )
    .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("Forwarded message from Alice Example:".to_string()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(
        regex::Regex::new(r"^Forwarded message from <[0-9a-f]{8}:Name_\d+>:$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn source_short_label_derivation_handles_builtin_and_adopter_shapes() {
    assert_eq!(
        derive_source_short_label("name.agent_recipient"),
        "agent_recipient"
    );
    assert_eq!(
        derive_source_short_label("name.forward_marker"),
        "forward_marker"
    );
    assert_eq!(derive_source_short_label("name.auto_footer"), "auto_footer");
    assert_eq!(
        derive_source_short_label("name.x.team_handoff"),
        "team_handoff"
    );
}
