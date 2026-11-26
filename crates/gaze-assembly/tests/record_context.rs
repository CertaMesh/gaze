use gaze::{
    Action, CleanDocument, Context, LocaleChain, PiiClass, Policy, RawDocument, RuleSpec, Scope,
    Session,
};
use gaze_assembly::{
    build_pipeline, build_pipeline_builder, pipeline_with_context, pipeline_with_private_context,
    resolve_policy_inputs, BuildError,
};

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
fn caller_value_too_short_refuses_all_library_assembly_paths_without_value() {
    let rejected = Context::from_json_str(r#"{"record":{"name":"Qx"}}"#).unwrap();
    let policy = policy(Action::Tokenize);
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let resident = build_pipeline(&policy, &context(), &[], &locales, None).unwrap();
    let results = [
        build_pipeline(&policy, &rejected, &[], &locales, None).map(|_| ()),
        build_pipeline_builder(&policy, &rejected, &[], &locales, None).map(|_| ()),
        pipeline_with_context(&resident, &policy, &rejected, &[], &locales).map(|_| ()),
        pipeline_with_private_context(&resident, &policy, &rejected, &[], &locales).map(|_| ()),
        resolve_policy_inputs(&policy, Some(&rejected), None, None).map(|_| ()),
    ];
    for result in results {
        let error = result.expect_err("rejected caller context must refuse assembly");
        assert!(matches!(
            error,
            BuildError::Untransformable(gaze::UntransformableReason::CallerValueTooShort)
        ));
        assert_eq!(error.to_string(), "untransformable: caller_value_too_short");
        for rendered in [error.to_string(), format!("{error:?}")] {
            assert!(!rendered.contains("Qx"));
            assert!(!rendered.contains("/name"));
        }
    }
}

#[test]
fn non_name_record_defaults_clean_and_restore_through_product_pipeline() {
    let context = Context::from_json_str(
        r#"{"record":{"card":"4111 1111 1111 1111","phone":"+1-212-555-0104","passport":"SYN-PASS-123456","national_id":"SYN-ID-12345","steuer_id":"12 345 678 901"},"field_map":{"/card":"custom:credit_card","/phone":"custom:phone","/passport":"custom:passport","/national_id":"custom:national_id","/steuer_id":"custom:steuer_id"}}"#,
    ).unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None)
        .expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "Card 4111 1111 1111 1111; phone +1-212-555-0104; passport SYN-PASS-123456; national ID SYN-ID-12345; Steuer-ID 12 345 678 901.";
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
    for value in [
        "4111 1111 1111 1111",
        "+1-212-555-0104",
        "SYN-PASS-123456",
        "SYN-ID-12345",
        "12 345 678 901",
    ] {
        assert!(
            !clean.contains(value),
            "record value stayed raw: {value}; clean={clean}"
        );
    }
    assert_eq!(
        pipeline
            .restore_with_telemetry(&session, &clean)
            .unwrap()
            .0
            .text,
        raw,
    );
}

#[test]
fn valid_de_iban_record_with_only_two_letters_restores() {
    let iban = "DE36000000000000000000";
    let context = Context::from_json_str(&format!(r#"{{"record":{{"iban":"{iban}"}}}}"#)).unwrap();
    assert!(context.record_value_rejections.is_empty());
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None)
        .expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = format!("IBAN: {iban}");
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
fn record_kind_after_normalization_matches_attribution_fixtures() {
    for (value, raw, expected_kind) in [
        ("Maren Okafor", "Maren\u{a0}Okafor", "exact"),
        ("Maren Okafor", "Maren\u{202f}Okafor", "exact"),
        ("Maren  Okafor", "Maren Okafor", "exact"),
        ("Maren  Okafor", "Maren\u{202f}Okafor", "exact"),
        ("Maren Okafor", "Maren  Okafor", "whitespace_flexible"),
        ("Maren Okafor", "Maren\u{a0} Okafor", "whitespace_flexible"),
    ] {
        for kind in ["exact", "whitespace_flexible"] {
            let input = serde_json::json!({
                "record": {"full_name": value},
                "record_match_kinds": {"name_multi": [kind]},
            });
            let context = Context::from_json_str(&input.to_string()).unwrap();
            let locales = LocaleChain::merge_policy_and_cli(None, None);
            let pipeline =
                build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
            // An exact occurrence also satisfies the whitespace-flexible kind.
            assert_eq!(
                !clean.contains(raw),
                kind == expected_kind || expected_kind == "exact",
                "record kind parity: {value:?}, {raw:?}, {kind}"
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
    }
}

#[test]
fn record_whitespace_flex_has_a_bounded_gap() {
    let context = Context::from_json_str(r#"{"record":{"full_name":"Maren Okafor"},"record_match_kinds":{"name_multi":["whitespace_flexible"]}}"#).unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline =
        build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
    let within = format!("Maren{}Okafor", "\u{a0}".repeat(32));
    let beyond = format!("Maren{}Okafor", "\u{a0}".repeat(33));
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
fn full_name_defaults_protect_same_case_whitespace_variants() {
    let context = Context::from_json_str(r#"{"record":{"full_name":"Maren Okafor"}}"#).unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline =
        build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
    for (raw, should_match) in [
        ("Maren Okafor", true),
        ("MAREN OKAFOR", true),
        ("MAREN  OKAFOR", true),
        ("Maren  Okafor", true),
    ] {
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
        assert_eq!(clean.contains(raw), !should_match, "{raw}");
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
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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

/// Cleans `raw` under `context_json`, checks byte-exact restore, and returns the clean text.
fn clean_with_context(context_json: &str, raw: &str) -> String {
    let context = Context::from_json_str(context_json).unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline =
        build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
    assert_eq!(
        pipeline
            .restore_with_telemetry(&session, &clean)
            .unwrap()
            .0
            .text,
        raw
    );
    clean
}

/// A spelling closer to the declared value is never less protected than a looser one:
/// letter case alone must not decide whether a declared value leaks. Each row is one
/// class group and spelling; benign rows keep near misses, identifier twins and common
/// words raw.
#[test]
fn record_spellings_closer_to_the_declared_value_are_never_less_protected() {
    const RARE: &str = r#"{"record":{"first_name":"Qorvex"}}"#;
    const COMMON: &str = r#"{"record":{"first_name":"Maren","last_name":"May"}}"#;
    const FULL: &str = r#"{"record":{"full_name":"Maren Okafor"}}"#;
    const PASSPORT: &str =
        r#"{"record":{"passport":"SYN-PASS-123456"},"field_map":{"/passport":"custom:passport"}}"#;
    const NATIONAL_ID: &str = r#"{"record":{"national_id":"SYN ID 12345"},"field_map":{"/national_id":"custom:national_id"}}"#;
    const CITY: &str = r#"{"record":{"city":"Exampleville"}}"#;
    const CITY_EXACT: &str =
        r#"{"record":{"city":"Exampleville"},"record_match_kinds":{"address_part":["exact"]}}"#;
    const RARE_FOLDED_ONLY: &str = r#"{"record":{"first_name":"Qorvex"},"record_match_kinds":{"name_single":["case_folded"]}}"#;
    const RARE_EXACT_ONLY: &str =
        r#"{"record":{"first_name":"Qorvex"},"record_match_kinds":{"name_single":["exact"]}}"#;
    const FULL_LOOSEST_ONLY: &str = r#"{"record":{"full_name":"Maren Okafor"},"record_match_kinds":{"name_multi":["whitespace_case_folded"]}}"#;
    const FULL_EXACT_ONLY: &str =
        r#"{"record":{"full_name":"Maren Okafor"},"record_match_kinds":{"name_multi":["exact"]}}"#;
    for (context, raw, value, protected) in [
        // Single unlisted name: every case spelling, including the declared one.
        (RARE, "found Qorvex today", "Qorvex", true),
        (RARE, "found qorvex today", "qorvex", true),
        (RARE, "found QORVEX today", "QORVEX", true),
        (RARE, "found Qorvexian today", "Qorvexian", false),
        (RARE, "ticket qorvex_build failed", "qorvex_build", false),
        (RARE, "found Qorvax today", "Qorvax", false),
        // Common single names stay behind corroboration in every case.
        (COMMON, "May 2026 report", "May 2026", false),
        (COMMON, "MAY 2026 report", "MAY 2026", false),
        (COMMON, "you may proceed", "may proceed", false),
        (COMMON, "Maren called.", "Maren", true),
        // Full names: case and whitespace spellings, never reordered names.
        (FULL, "Maren Okafor called", "Maren Okafor", true),
        (FULL, "MAREN OKAFOR called", "MAREN OKAFOR", true),
        (FULL, "Maren  Okafor called", "Maren  Okafor", true),
        (FULL, "MAREN  OKAFOR called", "MAREN  OKAFOR", true),
        (FULL, "Maren-Okafor called", "Maren-Okafor", false),
        // Identifier classes stay exact or whitespace-flexible; case is significant.
        (
            PASSPORT,
            "passport SYN-PASS-123456 ok",
            "SYN-PASS-123456",
            true,
        ),
        (
            PASSPORT,
            "passport syn-pass-123456 ok",
            "syn-pass-123456",
            false,
        ),
        (
            PASSPORT,
            "passport SYN-PASS-1234567 ok",
            "SYN-PASS-1234567",
            false,
        ),
        (NATIONAL_ID, "id SYN ID 12345 ok", "SYN ID 12345", true),
        (NATIONAL_ID, "id SYN  ID  12345 ok", "SYN  ID  12345", true),
        (NATIONAL_ID, "id syn id 12345 ok", "syn id 12345", false),
        // Address parts stay off until the adopter enables them.
        (CITY, "city Exampleville", "Exampleville", false),
        (CITY_EXACT, "city Exampleville", "Exampleville", true),
        (CITY_EXACT, "city EXAMPLEVILLE", "EXAMPLEVILLE", false),
        // Explicit kind lists: a closer spelling satisfies a looser kind, never the reverse.
        (RARE_FOLDED_ONLY, "found Qorvex today", "Qorvex", true),
        (RARE_EXACT_ONLY, "found qorvex today", "qorvex", false),
        (
            FULL_LOOSEST_ONLY,
            "Maren Okafor called",
            "Maren Okafor",
            true,
        ),
        (
            FULL_LOOSEST_ONLY,
            "Maren  Okafor called",
            "Maren  Okafor",
            true,
        ),
        (
            FULL_LOOSEST_ONLY,
            "maren okafor called",
            "maren okafor",
            true,
        ),
        (
            FULL_EXACT_ONLY,
            "maren okafor called",
            "maren okafor",
            false,
        ),
        (
            FULL_EXACT_ONLY,
            "Maren  Okafor called",
            "Maren  Okafor",
            false,
        ),
    ] {
        let clean = clean_with_context(context, raw);
        assert_eq!(
            !clean.contains(value),
            protected,
            "{context} :: {raw:?} -> {clean:?}"
        );
    }
}

#[test]
fn address_part_exact_requires_opt_in() {
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let raw = "The fictional city is Exampleville.";
    for (context_json, should_match) in [
        (r#"{"record":{"city":"Exampleville"}}"#, false),
        (
            r#"{"record":{"city":"Exampleville"},"record_match_kinds":{"address_part":["exact"]}}"#,
            true,
        ),
    ] {
        let context = Context::from_json_str(context_json).unwrap();
        let pipeline =
            build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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
        assert_eq!(clean.contains("Exampleville"), !should_match);
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
        let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
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

#[test]
fn normalized_known_terms_protect_original_bytes_and_session_repeats() {
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    for value in [
        "Ｑx7Lm2Rt",
        "Qx\u{200b}7Lm2Rt",
        "Qx7Lm2Rt",
        "Qe\u{301}7Lm2Rt",
    ] {
        for context_json in [
            serde_json::json!({"record":{"tag":value},"field_map":{"/tag":"custom:tag"},"record_match_kinds":{"custom:tag":["exact"]}}),
            serde_json::json!({"dictionaries":{"tag":{"terms":[value],"case_sensitive":true}},"class_map":{"tag":"custom:tag"}}),
        ] {
            let context = Context::from_json_str(&context_json.to_string()).unwrap();
            let configured = policy(Action::Tokenize);
            let pipeline = build_pipeline(&configured, &context, &[], &locales, None).unwrap();
            let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let raw = format!("found {value} here");
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
            assert!(
                clean.starts_with("found <"),
                "declared term leaked: {raw:?}"
            );
            assert!(
                clean.ends_with("> here"),
                "entire source value must be covered: {clean:?}"
            );
            assert_eq!(
                pipeline
                    .restore_with_telemetry(&session, &clean)
                    .unwrap()
                    .0
                    .text,
                raw
            );
            let resident = &pipeline;
            let CleanDocument::Text(repeated) = resident
                .pseudonymize_with_detect_context(
                    &session,
                    RawDocument::Text(raw.clone()),
                    locales.as_slice(),
                    &gaze::DictionaryBundle::default(),
                )
                .unwrap()
            else {
                panic!("expected text")
            };
            assert!(!repeated.contains(value), "session repeat leaked");
            assert_eq!(
                resident
                    .restore_with_telemetry(&session, &repeated)
                    .unwrap()
                    .0
                    .text,
                raw
            );
        }
    }
}

#[test]
fn normalized_short_records_still_refuse_before_assembly() {
    for value in ["Ｑx", "Q\u{115f}x", "１２３", "\u{200b}\u{200c}\u{200d}"] {
        let context = Context::from_json_str(&serde_json::json!({"record":{"tag":value},"field_map":{"/tag":"custom:tag"},"record_match_kinds":{"custom:tag":["exact"]}}).to_string()).unwrap();
        assert_eq!(
            context.untransformable_reason(),
            Some(gaze::UntransformableReason::CallerValueTooShort)
        );
        let locales = LocaleChain::merge_policy_and_cli(None, None);
        assert!(matches!(
            build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None),
            Err(BuildError::Untransformable(
                gaze::UntransformableReason::CallerValueTooShort
            ))
        ));
    }
}

#[test]
fn normalized_prefix_terms_cannot_hide_existing_longer_dictionary_values() {
    let context = Context::from_json_str(r#"{"dictionaries":{"tag":{"terms":["Ｑx7", "Qx7Lm2Rt"],"case_sensitive":true}},"class_map":{"tag":"custom:tag"}}"#).unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline =
        build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "found Qx7Lm2Rt here";
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
        clean.starts_with("found <"),
        "the preexisting longer match must survive: {clean:?}"
    );
    assert!(clean.ends_with("> here"));
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
fn ascii_record_terms_protect_non_ascii_text_spellings_and_restore() {
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    for (value, spelling) in [
        ("Qx7Lm2Rt", "Ｑx７Lm２Rt"),
        ("Qx7Lm2Rt", "Qx\u{200b}7Lm2Rt"),
        ("Qx7-Lm2Rt", "Qx7\u{2011}Lm2Rt"),
        ("Qx7 Lm2Rt", "Qx7\u{2007}Lm2Rt"),
    ] {
        let context = Context::from_json_str(
            &serde_json::json!({
                "record": {"tag": value}, "field_map": {"/tag": "custom:tag"},
                "record_match_kinds": {"custom:tag": ["exact"]},
            })
            .to_string(),
        )
        .unwrap();
        let pipeline =
            build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
        let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let raw = format!("found {spelling} here");
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
        assert!(
            clean.starts_with("found <"),
            "normalized text value leaked: {clean:?}"
        );
        assert!(
            clean.ends_with("> here"),
            "token must cover every source byte"
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
}

#[test]
fn record_normalization_does_not_equate_distinct_separators_or_combining_marks() {
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    for (value, spelling) in [
        ("Qx7-Lm2Rt", "Qx7Lm2Rt"),
        ("Qx7 Lm2Rt", "Qx7Lm2Rt"),
        ("Qx7-Lm2Rt", "Qx7 Lm2Rt"),
        ("Qx7-Lm2Rt", "Qx7\u{2010}Lm2Rt"),
        ("Qx7Lm2Rt", "Qx7Lm2R\u{0301}t"),
    ] {
        for context_json in [
            serde_json::json!({"record":{"tag":value},"field_map":{"/tag":"custom:tag"},"record_match_kinds":{"custom:tag":["exact"]}}),
            serde_json::json!({"dictionaries":{"tag":{"terms":[value],"case_sensitive":true}},"class_map":{"tag":"custom:tag"}}),
        ] {
            let context = Context::from_json_str(&context_json.to_string()).unwrap();
            let pipeline =
                build_pipeline(&policy(Action::Tokenize), &context, &[], &locales, None).unwrap();
            let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let raw = format!("found {spelling} here");
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
            assert_eq!(
                clean, raw,
                "distinct values must stay distinct under the shared fold"
            );
            assert!(
                session.tokens().is_empty(),
                "unrelated text must emit no private token"
            );
        }
    }
}
