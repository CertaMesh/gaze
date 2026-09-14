use gaze::{
    Action, CleanDocument, Context, LocaleChain, PiiClass, Policy, RawDocument, RuleSpec, Scope,
    Session,
};
use gaze_assembly::build_pipeline;

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
