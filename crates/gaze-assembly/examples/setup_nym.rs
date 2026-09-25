use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

use gaze::{
    CleanDocument, Context, DictionaryBundle, LocaleChain, Policy, RawDocument, Rulepack,
    RulepackSource, SafetyNetPolicy, Session,
};

fn main() -> Result<(), Box<dyn Error>> {
    let policy = Policy::load_for_cli(Path::new("gaze.toml"))?;
    let context = Context {
        dictionaries: HashMap::new(),
        class_map: HashMap::new(),
        fields: serde_json::Map::new(),
    };
    let mut rulepacks = Vec::new();
    for name in &policy.rulepacks.bundled {
        let contents = gaze_recognizers::embedded(name).ok_or("unknown bundled rulepack")?;
        rulepacks.push(Rulepack::load(RulepackSource::Embedded(contents))?);
    }
    for path in &policy.rulepacks.paths {
        rulepacks.push(Rulepack::load(RulepackSource::Path(path.clone()))?);
    }
    let locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = gaze_assembly::build_pipeline(&policy, &context, &rulepacks, &locales, None)?;
    let session = Session::from_policy(&policy)?;
    let (clean, _, _) = pipeline.clean_with_safety_net_policy_detect_context(
        &session,
        RawDocument::Text("Das Fahrzeug mit dem Kennzeichen M-AB 1234 wurde abgeschleppt.".into()), // fixture-cited(crates/gaze-cli/tests/nym_cli.rs:live_nym_net_tokenizes_a_plate_the_rules_miss)
        locales.as_slice(),
        &DictionaryBundle::default(),
        SafetyNetPolicy::default(),
    )?;
    let CleanDocument::Text(text) = clean else {
        return Err("expected text output".into());
    };
    println!("{text}");
    let snapshot = session.export()?;
    // Keep snapshot.into_bytes() on the owner side for authorized restore.
    let _owner_blob = snapshot.into_bytes();
    Ok(())
}
