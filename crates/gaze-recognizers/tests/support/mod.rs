// Each integration-test binary uses a different subset of these helpers.
#![allow(dead_code)]

use gaze::{
    CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, Pipeline, RawDocument,
    RuleSpec, Rulepack, RulepackSource, Session,
};
use gaze_recognizers::embedded;

pub fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    }
}

pub fn pipeline_for(
    locales: &[LocaleTag],
    bundles: &[&str],
    rules: Vec<RuleSpec>,
    auto_activate_locale_gated: bool,
) -> Pipeline {
    let rulepacks: Vec<Rulepack> = bundles
        .iter()
        .map(|name| {
            Rulepack::load(RulepackSource::Embedded(
                embedded(name).unwrap_or_else(|| panic!("{name} rulepack")),
            ))
            .unwrap_or_else(|error| panic!("{name} loads: {error}"))
        })
        .collect();
    let mut policy = gaze::Policy::default();
    policy.rules = rules;
    policy.rulepacks.bundled = bundles.iter().map(|name| (*name).to_string()).collect();
    policy.rulepacks.auto_activate_locale_gated = auto_activate_locale_gated;
    let chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(locales));
    gaze_assembly::build_pipeline(&policy, &empty_context(), &rulepacks, &chain, None)
        .expect("pipeline")
}

pub fn clean(pipeline: &Pipeline, session: &Session, text: &str, locales: &[LocaleTag]) -> String {
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            session,
            RawDocument::Text(text.to_string()),
            locales,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    }
}
