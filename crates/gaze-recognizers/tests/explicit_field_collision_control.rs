//! Synthetic collision control, runnable unchanged with the exact pre-floor core TOML.
//! The builtin partial-overlap behavior exists before these field rules.

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Policy,
    RuleSpec, Rulepack, RulepackSource, SafetyNetPolicy, Scope, Session,
};

/// The field floor must not change how a partial builtin overlap resolves. That
/// is still what this asserts: the same whole 0..15 Name selection, the same
/// source IDs modulo whether `password.field` is present in the loaded core.
///
/// What changed is the suffix. This test used to pin ` right"` as leftover raw
/// text, because whole-candidate arbitration kept 0..15 and dropped every other
/// byte the losing `password.field` original covered. Residual coverage now
/// protects that remainder, so the expectation moves from "the suffix survives
/// in the clear" to "the suffix is covered by a second reversible replacement",
/// and the clean text has nothing left after the two tokens.
#[test]
fn partial_builtin_overlap_resolves_identically_before_and_after_field_floor() {
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
    // The whole selection is unchanged: same span, same class, same restored bytes.
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].class, PiiClass::Name);
    assert_eq!(spans[0].raw_span, 0..15);
    assert!(spans[0].origin.is_whole());
    assert_eq!(&raw[spans[0].raw_span.clone()], "password: \"left");
    assert_eq!(
        session
            .restore(&clean[spans[0].clean_span.clone()])
            .as_deref(),
        Some("password: \"left")
    );

    // The suffix this test used to pin as leftover raw text is now covered.
    assert_eq!(spans[1].raw_span, 15..21);
    assert!(spans[1].origin.is_residual_fragment());
    assert_eq!(&raw[spans[1].raw_span.clone()], " right");
    assert_eq!(
        session
            .restore(&clean[spans[1].clean_span.clone()])
            .as_deref(),
        Some(" right")
    );
    // The admitted union is 0..21, because `password.field` matches up to but
    // not including the closing quote. Byte 21 was never evidenced by any
    // original, so it is outside U and residual coverage does not touch it.
    // This is the documented limit, shown here on a real rulepack rather than
    // asserted in prose: coverage protects evidenced bytes, not all bytes.
    assert_eq!(
        &clean[spans[1].clean_span.end..],
        "\"",
        "bytes outside the admitted union stay uncovered"
    );

    assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0].raw_start()..trace[0].raw_end(), 0..15);
    assert_eq!(trace[0].class(), &PiiClass::Name);
    assert_eq!(trace[1].raw_start()..trace[1].raw_end(), 15..21);
    // The residual projects to the existing tuple; it is primary-pipeline
    // policy-driven tokenization, and that tuple does not certify a whole entity.
    for item in &trace {
        assert_eq!(
            (item.stage(), item.decision(), item.action()),
            ("primary_pipeline", "policy", "tokenize")
        );
    }
    let expected = if has_field {
        vec!["password.field", "synthetic.partial.name"]
    } else {
        vec!["synthetic.partial.name"]
    };
    assert_eq!(trace[0].source_ids(), expected.as_slice());
    println!(
        "field_present={has_field}; whole=0..15 class=Name; residual={:?} covering {:?}; sources={:?}; exact_restore=true",
        spans[1].raw_span,
        &raw[spans[1].raw_span.clone()],
        trace[0].source_ids()
    );
}
