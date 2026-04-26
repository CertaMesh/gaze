use super::*;
use crate::template::lower_pattern_template;
use gaze::{
    Action, CleanDocument, DetectorKind, LocaleTag, PiiClass, PolicyError, RawDocument,
    RulepackError, Scope, Session, SessionPolicy, SessionScope,
};

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
    let dictionaries = gaze::DictionaryBundle::from_context(&context);
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
rulepack_version = "0.4.5"
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
