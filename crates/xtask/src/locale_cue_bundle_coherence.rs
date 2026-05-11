use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use gaze::{Rulepack, RulepackSource};

const RULEPACKS_WITH_MANDATORY_ANCHORS: &[&str] = &["core", "core-extended"];
const LOCALE_RULEPACKS: &[&str] = &["locale-de", "locale-en"];

pub(crate) fn run() -> Result<()> {
    let locale_cues = load_locale_cues()?;
    let mut missing = Vec::new();

    for rulepack_id in RULEPACKS_WITH_MANDATORY_ANCHORS {
        let rulepack = load_embedded(rulepack_id)?;
        for recognizer in &rulepack.recognizers {
            let Some(anchor_key) = recognizer
                .collision
                .as_ref()
                .and_then(|collision| collision.mandatory_anchor.as_deref())
            else {
                continue;
            };
            if !locale_cues.contains(anchor_key) {
                missing.push(format!("{} -> {}", recognizer.id, anchor_key));
            }
        }
    }

    if !missing.is_empty() {
        bail!(
            "mandatory-anchor declarations missing locale cue bundle: {}",
            missing.join(", ")
        );
    }

    println!("locale-cue-bundle-coherence: passed");
    Ok(())
}

fn load_locale_cues() -> Result<BTreeSet<String>> {
    let mut cues = BTreeSet::new();
    for rulepack_id in LOCALE_RULEPACKS {
        let rulepack = load_embedded(rulepack_id)?;
        let Some(locale) = rulepack.locale else {
            continue;
        };
        cues.extend(locale.cues.into_keys());
    }
    Ok(cues)
}

fn load_embedded(rulepack_id: &str) -> Result<Rulepack> {
    let contents = gaze_recognizers::embedded(rulepack_id)
        .with_context(|| format!("unknown embedded rulepack '{rulepack_id}'"))?;
    Rulepack::load(RulepackSource::Embedded(contents))
        .with_context(|| format!("failed to load embedded rulepack '{rulepack_id}'"))
}
