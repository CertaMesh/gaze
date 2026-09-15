//! Synthetic collision control, runnable unchanged with the exact pre-floor core TOML.
//! The builtin partial-overlap behavior exists before these field rules.

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Policy,
    RuleSpec, Rulepack, RulepackSource, SafetyNetPolicy, Scope, Session,
};

#[test]
fn partial_builtin_overlap_keeps_identical_raw_suffix_before_and_after_field_floor() {
    let core = Rulepack::load(RulepackSource::Embedded(
        gaze_recognizers::embedded("core").unwrap(),
    ))
    .unwrap();
    let has_field = core.recognizers.iter().any(|r| r.id == "password.field");
    let competitor = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "synthetic-partial-overlap"
rulepack_version = "0.1.0"
[[recognizers]]
id = "synthetic.partial.name"
class = "Name"
enabled = true
locales = ["global"]
[recognizers.match]
kind = "regex"
pattern = 'password: "left'
[recognizers.scoring]
base = 0.90
priority = 0
"#,
    )
    .unwrap();
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let context = Context {
        dictionaries: Default::default(),
        class_map: Default::default(),
        fields: Default::default(),
    };
    let pipeline = gaze_assembly::build_pipeline(
        &policy,
        &context,
        &[core, competitor],
        &LocaleChain::from(&[LocaleTag::Global][..]),
        None,
    )
    .unwrap();
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
    let CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].class, PiiClass::Name);
    assert_eq!(spans[0].raw_span, 0..15);
    assert_eq!(&raw[spans[0].raw_span.clone()], "password: \"left");
    assert_eq!(&clean[spans[0].clean_span.end..], " right\"");
    assert_eq!(
        session
            .restore(&clean[spans[0].clean_span.clone()])
            .as_deref(),
        Some("password: \"left")
    );
    assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].raw_start()..trace[0].raw_end(), 0..15);
    assert_eq!(trace[0].class(), &PiiClass::Name);
    let expected = if has_field {
        vec!["password.field", "synthetic.partial.name"]
    } else {
        vec!["synthetic.partial.name"]
    };
    assert_eq!(trace[0].source_ids(), expected.as_slice());
    println!("field_present={has_field}; raw=0..15; class=Name; leftover={:?}; sources={:?}; exact_restore=true", &clean[spans[0].clean_span.end..], trace[0].source_ids());
}
