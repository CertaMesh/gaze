use gaze::{
    Action, CleanDocument, Context, LocaleChain, PiiClass, Policy, RawDocument, RuleSpec, Scope,
    Session,
};
use gaze_assembly::{build_pipeline, BuildError};

fn context() -> Context {
    Context::from_json_str(
        r#"{"record":{"name":"Alice Smith","email":"alice@example.invalid"},"field_map":{"/name":"Name","/email":"Email"}}"#,
    )
    .unwrap()
}

fn policy(action: Action) -> Policy {
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default { action }];
    policy
}

#[test]
fn record_name_casing_tokenizes_and_restores_exact_source_bytes() {
    let context = context();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None)
        .expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "CONTACT ALICE SMITH; alice smith wrote alice@example.invalid";
    let bundle = gaze::dictionary_bundle_from_context(&context);
    let clean = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.to_string()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap();
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text")
    };
    assert!(!clean.contains("ALICE SMITH"));
    assert!(!clean.contains("alice smith"));
    assert!(!clean.contains("alice@example.invalid"));
    assert_eq!(
        pipeline
            .restore_with_telemetry(&session, &clean)
            .unwrap()
            .0
            .text,
        raw
    );
}

#[test]
fn record_unicode_name_case_match_restores_source_bytes() {
    let context = Context::from_json_str(
        r#"{"record":{"name":"Émilie Müller"},"field_map":{"/name":"Name"}}"#,
    )
    .unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None)
        .expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "ÉMILIE MÜLLER met émilie müller.";
    let bundle = gaze::dictionary_bundle_from_context(&context);
    let clean = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.into()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap();
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text")
    };
    assert!(!clean.contains("ÉMILIE MÜLLER"));
    assert!(!clean.contains("émilie müller"));
    assert_eq!(
        pipeline
            .restore_with_telemetry(&session, &clean)
            .unwrap()
            .0
            .text,
        raw
    );
}

#[test]
fn record_full_unicode_fold_preserves_original_byte_span() {
    let context = Context::from_json_str(
        r#"{"record":{"name":"JÖRG STRASSE"},"field_map":{"/name":"Name"}}"#,
    )
    .unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None)
        .expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "Jörg Straße wrote to JÖRG STRASSE.";
    let bundle = gaze::dictionary_bundle_from_context(&context);
    let clean = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.into()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap();
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text")
    };
    assert!(!clean.contains("JÖRG STRASSE"));
    assert!(!clean.contains("Jörg Straße"));
    assert_eq!(
        pipeline
            .restore_with_telemetry(&session, &clean)
            .unwrap()
            .0
            .text,
        raw
    );
}

#[test]
fn record_mapping_requires_reversible_policy_action() {
    let context = context();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    for action in [Action::Preserve, Action::Redact, Action::Generalize] {
        assert!(matches!(
            build_pipeline(&policy(action), &context, &[], &locales, None),
            Err(BuildError::RecordPolicy)
        ));
    }
}

#[test]
fn record_refuses_column_preserve_before_default() {
    let context = context();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let mut policy = policy(Action::Tokenize);
    policy.rules.insert(
        0,
        RuleSpec::Column {
            column: "message".into(),
            action: Action::Preserve,
        },
    );
    assert!(matches!(
        build_pipeline(&policy, &context, &[], &locales, None),
        Err(BuildError::RecordPolicy)
    ));
}

#[test]
fn record_terms_keep_their_typed_classes() {
    let context = context();
    assert!(context
        .class_map
        .values()
        .any(|class| class == &PiiClass::Name));
    assert!(context
        .class_map
        .values()
        .any(|class| class == &PiiClass::Email));
}
