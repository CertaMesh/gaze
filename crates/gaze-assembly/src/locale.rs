use std::collections::{BTreeSet, HashMap};

use gaze::{LocaleChain, Rulepack};

use crate::registration::AssemblyBuilder;

pub(crate) fn merged_locale_vocab(
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
) -> HashMap<String, Vec<String>> {
    let mut buckets = HashMap::new();
    let mut seen = HashMap::<String, BTreeSet<String>>::new();

    for active_locale in active_locales.as_slice() {
        for rulepack in rulepacks {
            if !rulepack.default_locales.contains(active_locale) {
                continue;
            }
            let Some(locale) = rulepack.locale.as_ref() else {
                continue;
            };
            for (bucket_name, bucket) in &locale.buckets {
                let bucket_values = buckets
                    .entry(bucket_name.clone())
                    .or_insert_with(Vec::<String>::new);
                let bucket_seen = seen.entry(bucket_name.clone()).or_default();
                for name in &bucket.names {
                    if bucket_seen.insert(name.clone()) {
                        bucket_values.push(name.clone());
                    }
                }
            }
        }
    }

    buckets
}

pub(crate) fn register_anchor_cue_bundles(
    builder: &mut AssemblyBuilder,
    rulepacks: &[Rulepack],
    active_locales: &LocaleChain,
) {
    for active_locale in active_locales.as_slice() {
        for rulepack in rulepacks {
            if !rulepack.default_locales.contains(active_locale) {
                continue;
            }
            let Some(locale) = rulepack.locale.as_ref() else {
                continue;
            };
            for (anchor_key, bundle) in &locale.cues {
                builder.register_anchor_cue_bundle(
                    active_locale.clone(),
                    anchor_key.clone(),
                    bundle.names.clone(),
                    bundle.window_chars,
                );
            }
        }
    }
}
