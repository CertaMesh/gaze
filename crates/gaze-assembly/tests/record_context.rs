use gaze::{
    Action, CleanDocument, Context, LocaleChain, PiiClass, Policy, RawDocument, RuleSpec, Scope,
    Session,
};
use gaze_assembly::{build_pipeline, BuildError};

fn context() -> Context {
    Context::from_json_str(
        r#"{"record":{"name":"Alice Smith","email":"alice@example.invalid"},"field_map":{"/name":"Name","/email":"Email"},"record_match_kinds":{"name_multi":["exact","case_folded"],"email":["exact"]}}"#,
    )
    .unwrap()
}

fn policy(action: Action) -> Policy {
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default { action }];
    policy
}

#[test]
fn valid_de_iban_record_survives_short_name_and_restores() {
    let iban = "DE36000000000000000000";
    let context =
        Context::from_json_str(&format!(r#"{{"record":{{"iban":"{iban}","name":"A"}}}}"#)).unwrap();
    assert_eq!(context.record_value_rejections.len(), 1);
    assert_eq!(context.record_value_rejections[0].path, "/name");
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None)
        .expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = format!("IBAN: {iban}");
    let bundle = gaze::dictionary_bundle_from_context(&context);
    let CleanDocument::Text(clean) = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.clone()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap()
    else {
        panic!("expected text")
    };
    assert!(!clean.contains(iban));
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
        r#"{"record":{"name":"Émilie Müller"},"field_map":{"/name":"Name"},"record_match_kinds":{"name_multi":["exact","case_folded"]}}"#,
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
        r#"{"record":{"name":"JÖRG STRASSE"},"field_map":{"/name":"Name"},"record_match_kinds":{"name_multi":["exact","case_folded"]}}"#,
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
fn record_whitespace_variants_restore_original_bytes() {
    let context =
        Context::from_json_str(r#"{"record":{"customer":{"full_name":" Maren\u00a0Okafor "}},"record_match_kinds":{"name_multi":["exact","whitespace_flexible","whitespace_case_folded"]}}"#)
            .unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline =
        build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "Maren  Okafor met Maren Okafor and Maren\u{202f}Okafor.";
    let bundle = gaze::dictionary_bundle_from_context(&context);
    let CleanDocument::Text(clean) = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.into()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap()
    else {
        panic!("expected text")
    };
    assert!(
        !clean.contains("Maren"),
        "all spacing variants must be protected"
    );
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
fn record_whitespace_flex_has_a_bounded_gap() {
    let context = Context::from_json_str(r#"{"record":{"full_name":"Maren Okafor"},"record_match_kinds":{"name_multi":["whitespace_flexible"]}}"#).unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline =
        build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
    let within = format!("Maren{}Okafor", "\u{a0}".repeat(32));
    let beyond = format!("Maren{}Okafor", "\u{a0}".repeat(33));
    let bundle = gaze::dictionary_bundle_from_context(&context);
    for (raw, protected) in [(within, true), (beyond, false)] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .pseudonymize_with_detect_context(
                &session,
                RawDocument::Text(raw.clone()),
                locales.as_slice(),
                &bundle,
            )
            .unwrap()
        else {
            panic!("expected text")
        };
        assert_eq!(!clean.contains(&raw), protected);
        assert_eq!(
            pipeline
                .restore_with_telemetry(&session, &clean)
                .unwrap()
                .0
                .text,
            raw
        );
    }
}

#[test]
fn single_token_record_names_need_corroboration() {
    let context = Context::from_json_str(
        r#"{"record":{"first_name":"Will","last_name":"Smith","full_name":"Will Smith"},"record_match_kinds":{"name_single":["exact","case_folded","corroborated_single"],"name_multi":["exact"]}}"#,
    )
    .unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline =
        build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "Will you send it? Will Smith called. Hi Will,";
    let bundle = gaze::dictionary_bundle_from_context(&context);
    let CleanDocument::Text(clean) = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.into()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap()
    else {
        panic!("expected text")
    };
    assert!(clean.contains("Will you send it?"));
    assert!(!clean.contains("Will Smith"));
    assert!(!clean.contains("Hi Will,"));
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
fn common_may_stays_raw_and_unlisted_maren_tokenizes() {
    let context =
        Context::from_json_str(r#"{"record":{"first_name":"Maren","last_name":"May"}}"#).unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline =
        build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "May 2026. Maren called.";
    let bundle = gaze::dictionary_bundle_from_context(&context);
    let CleanDocument::Text(clean) = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.into()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap()
    else {
        panic!("expected text")
    };
    assert!(clean.contains("May 2026"));
    assert!(!clean.contains("Maren"));
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
fn unmeasured_record_class_is_off_until_adopter_enables_it() {
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let raw = "Synthetic reference ZXCVB12345.";
    for (override_kinds, expected_protected) in [
        ("", false),
        (r#", "record_match_kinds":{"custom:tag":["exact"]}"#, true),
    ] {
        let context = Context::from_json_str(&format!(
            r#"{{"record":{{"tag":"ZXCVB12345"}},"field_map":{{"/tag":"custom:tag"}}{override_kinds}}}"#
        ))
        .unwrap();
        let pipeline =
            build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
        let bundle = gaze::dictionary_bundle_from_context(&context);
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .pseudonymize_with_detect_context(
                &session,
                RawDocument::Text(raw.into()),
                locales.as_slice(),
                &bundle,
            )
            .unwrap()
        else {
            panic!("expected text")
        };
        assert_eq!(!clean.contains("ZXCVB12345"), expected_protected);
        assert_eq!(
            pipeline
                .restore_with_telemetry(&session, &clean)
                .unwrap()
                .0
                .text,
            raw
        );
    }
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
