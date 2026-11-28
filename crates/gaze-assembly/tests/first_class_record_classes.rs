//! Caller-known records keep working when their class parses to a first-class variant.

use gaze::{
    Action, CleanDocument, Context, LocaleChain, PiiClass, Policy, RawDocument, RuleSpec, Scope,
    Session,
};
use gaze_assembly::build_pipeline;

/// Documented example IBAN with a valid mod-97 checksum and only two letters.
const SYNTHETIC_IBAN: &str = "DE89370400440532013000";

#[test]
#[ignore = "needs the context.rs IBAN precursor"]
fn policy_mapped_iban_record_is_accepted_and_tokenized() {
    let class = PiiClass::from_policy_name("custom:iban").expect("iban policy class");
    assert_eq!(class.to_canonical_str(), "custom:iban");

    let context = Context::from_json_str(&format!(
        r#"{{"record":{{"account":"{SYNTHETIC_IBAN}"}},"field_map":{{"/account":"custom:iban"}}}}"#
    ))
    .expect("record context");
    assert!(
        context.record_value_rejections.is_empty(),
        "a checksum-valid IBAN record must not be refused as too short"
    );

    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy, &context, &[], &locales, None).expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = format!("Account {SYNTHETIC_IBAN} is on file.");
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
    assert!(!clean.contains(SYNTHETIC_IBAN));
    assert!(clean.contains("Custom:iban"), "clean text: {clean}");
    assert_eq!(
        pipeline
            .restore_with_telemetry(&session, &clean)
            .unwrap()
            .0
            .text,
        raw
    );
}
