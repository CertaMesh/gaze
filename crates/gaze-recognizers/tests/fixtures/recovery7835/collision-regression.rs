use gaze::{Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, Policy, RuleSpec, Rulepack, RulepackSource, SafetyNetPolicy, Scope, Session};
const RAW: &str = "password: \"left right\"\nmarker";
fn run(current: bool) -> Vec<std::ops::Range<usize>> {
    let core = if current { Rulepack::load(RulepackSource::Embedded(gaze_recognizers::embedded("core").unwrap())).unwrap() } else { Rulepack::parse(include_str!("base-core.toml")).unwrap() };
    let competitor = Rulepack::parse(r#"
schema_version = "0.1.0"
rulepack_id = "independent-two-fragments"
rulepack_version = "0.1.0"
[[recognizers]]
id = "synthetic.fragments"
class = "Name"
enabled = true
locales = ["global"]
[recognizers.match]
kind = "regex"
pattern = 'left|right"\nmarker'
[recognizers.scoring]
base = 0.90
priority = 0
"#).unwrap();
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default { action: Action::Tokenize }];
    let context = Context { dictionaries: Default::default(), class_map: Default::default(), fields: Default::default() };
    let pipeline = gaze_assembly::build_pipeline(&policy, &context, &[core,competitor], &LocaleChain::from(&[LocaleTag::Global][..]), None).unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean,spans,_,trace) = pipeline.clean_text_with_safety_net_policy_detect_context_and_protection_trace(&session,RAW,&[LocaleTag::Global],&DictionaryBundle::default(),SafetyNetPolicy::default()).unwrap();
    let CleanDocument::Text(clean) = clean else { panic!("text") };
    assert_eq!(session.restore_strict_text(&clean).unwrap(), RAW);
    println!("current={current}; clean={clean:?}; spans={spans:?}; trace={trace:?}");
    let strict_session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = strict_session.begin_transaction();
    let strict = pipeline.protect_text_transaction(&mut tx, RAW, gaze::ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default())).unwrap();
    assert_eq!(tx.restore_strict_text(&strict).unwrap(), RAW);
    assert!(strict_session.tokens().is_empty());
    println!("strict current={current}; clean={strict:?}; exact_restore=true; live_empty=true");
    assert_eq!(strict.contains("left"), clean.contains("left"));
    drop(tx);
    assert!(strict_session.tokens().is_empty());
    spans.into_iter().map(|s|s.raw_span).collect()
}
#[test]
fn exact_base_protects_both_fragments() {
    assert_eq!(run(false),vec![11..15,16..29]);
}
#[test]
fn new_floor_must_not_expose_previously_protected_left() {
    let spans=run(true);
    assert!(spans.iter().any(|r|r.start<=11 && r.end>=15),"new field removes prior protection of raw11..15");
}
