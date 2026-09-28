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
fn known_record_variants_tokenize_and_restore_exact_source_bytes() {
    let context = context();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None)
        .expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "SMITH ALICE wrote ALICE@EXAMPLE.INVALID";
    // Name order is conservative and case-sensitive; email ASCII casing is not.
    let raw = raw.replace("SMITH ALICE", "Smith Alice");
    let bundle = gaze::dictionary_bundle_from_context(&context);
    let clean = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.clone()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap();
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text")
    };
    assert!(!clean.contains("Smith Alice"));
    assert!(!clean.contains("ALICE@EXAMPLE.INVALID"));
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
