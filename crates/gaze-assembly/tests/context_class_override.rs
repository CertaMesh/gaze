use gaze::{
    Action, CleanDocument, Context, LocaleChain, PiiClass, Policy, RawDocument, RuleSpec, Scope,
    Session,
};
use gaze_assembly::build_pipeline;

fn context_with_mapping(dictionary_name: &str, class: Option<&str>) -> Context {
    let class_map = class.map_or_else(serde_json::Map::new, |class| {
        serde_json::Map::from_iter([(dictionary_name.to_string(), serde_json::json!(class))])
    });
    Context::from_json_str(
        &serde_json::json!({
            "dictionaries": {
                dictionary_name: {"terms": ["Synthetic Colleague"], "case_sensitive": true}
            },
            "class_map": class_map,
            "fields": {}
        })
        .to_string(),
    )
    .unwrap()
}

#[test]
fn invalid_dictionary_fallback_without_override_is_rejected() {
    for dictionary_name in ["!!!", "東京"] {
        let context = context_with_mapping(dictionary_name, None);
        let locales = LocaleChain::merge_policy_and_cli(None, None);
        assert!(matches!(
            build_pipeline(&Policy::default(), &context, &[], &locales, None),
            Err(gaze_assembly::BuildError::Pipeline(
                gaze::Error::EmptyCustomClassName(_)
            ))
        ));
    }
}

#[test]
fn non_protective_override_is_rejected_for_invalid_and_valid_fallbacks() {
    for dictionary_name in ["!!!", "東京", "dict_alpha"] {
        let context = context_with_mapping(dictionary_name, Some("Name"));
        // First matching action wins, including a Preserve before a protective rule.
        for rules in [
            vec![],
            vec![RuleSpec::Default {
                action: Action::Preserve,
            }],
            vec![
                RuleSpec::Class {
                    class: PiiClass::Name,
                    action: Action::Preserve,
                },
                RuleSpec::Default {
                    action: Action::Tokenize,
                },
            ],
            vec![
                RuleSpec::Default {
                    action: Action::Preserve,
                },
                RuleSpec::Class {
                    class: PiiClass::Name,
                    action: Action::Tokenize,
                },
            ],
        ] {
            let mut policy = Policy::default();
            policy.rules = rules;
            let locales = LocaleChain::merge_policy_and_cli(None, None);
            let result = build_pipeline(&policy, &context, &[], &locales, None);
            if dictionary_name == "dict_alpha" {
                assert!(matches!(
                    result,
                    Err(gaze_assembly::BuildError::Rulepack(
                        gaze::RulepackError::ClassMapOverrideClash { .. }
                    ))
                ));
            } else {
                assert!(matches!(
                    result,
                    Err(gaze_assembly::BuildError::Pipeline(
                        gaze::Error::EmptyCustomClassName(_)
                    ))
                ));
            }
        }
    }
}

#[test]
fn unchanged_custom_class_keeps_existing_preserve_behavior() {
    let context = context_with_mapping("dict_alpha", Some("custom:dict_alpha"));
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Preserve,
    }];
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    assert!(build_pipeline(&policy, &context, &[], &locales, None).is_ok());
}

#[test]
fn explicit_class_override_does_not_validate_unused_dictionary_name_as_class() {
    for dictionary_name in ["!!!", "東京"] {
        let context = Context::from_json_str(
            &serde_json::json!({
                "dictionaries": {
                    dictionary_name: {"terms": ["Synthetic Colleague"], "case_sensitive": true}
                },
                "class_map": {dictionary_name: "Name"},
                "fields": {}
            })
            .to_string(),
        )
        .expect("dictionary identifiers are not class labels");
        let mut policy = Policy::default();
        policy.rules = vec![
            RuleSpec::Class {
                class: PiiClass::Name,
                action: Action::Tokenize,
            },
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ];
        let locales = LocaleChain::merge_policy_and_cli(None, None);
        let pipeline = build_pipeline(&policy, &context, &[], &locales, None)
            .expect("explicit valid class must replace the unused fallback");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let dictionaries = gaze::dictionary_bundle_from_context(&context);
        let clean = pipeline
            .pseudonymize_with_detect_context(
                &session,
                RawDocument::Text("Synthetic Colleague".into()),
                locales.as_slice(),
                &dictionaries,
            )
            .unwrap();
        let CleanDocument::Text(token) = clean else {
            panic!("expected text");
        };
        assert!(token.ends_with(":Name_1>"));
        assert_eq!(
            session.restore_strict(&token).unwrap(),
            "Synthetic Colleague"
        );
    }
}
