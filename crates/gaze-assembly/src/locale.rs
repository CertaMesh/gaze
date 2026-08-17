use std::collections::{BTreeSet, HashMap};

use gaze::{LocaleBasis, LocaleChain, LocaleTag, Rulepack, SafetyTier};

use crate::registration::AssemblyBuilder;

/// Compatibility order for the activation locales the v0.6 `core-extended`
/// alias shipped with. Ordering only: a tag listed here is NOT added to the
/// activation set unless a loaded locale-gated recognizer declares it. Kept so
/// per-class locale-fallback winners for the bundled `core` recognizers do not
/// change when the set is derived instead of spelled out.
const COMPATIBILITY_ACTIVATION_ORDER: [LocaleTag; 4] = [
    LocaleTag::EnUs,
    LocaleTag::DeDe,
    LocaleTag::DeAt,
    LocaleTag::DeCh,
];

/// Locales that `auto_activate_locale_gated` adds to the rulepack-default
/// locale chain so every loaded `safety_tier = "locale_gated"` recognizer can
/// activate.
///
/// This is the union of `locales` over the enabled, `locale_basis = "document"`
/// locale-gated recognizers in `rulepacks`, minus `global`. It is derived from
/// the loaded packs rather than spelled out, so a locale-gated recognizer
/// cannot ship (bundled or adopter path pack) whose locale is missing from the
/// activation set. Format-basis recognizers do not contribute: they run
/// regardless of the document chain. `global` never needs pushing.
///
/// The push only takes effect when neither the CLI nor the policy sets a
/// locale (see [`LocaleChain::merge_cli_policy_rulepack_default`]).
///
/// Ordering is deterministic and stable: the compatibility order shipped since
/// v0.6 for the tags it named (`en-US`, `de-DE`, `de-AT`, `de-CH`) comes
/// first; any other tag follows in canonical BCP-47 string order.
pub fn locale_gated_activation_locales(rulepacks: &[Rulepack]) -> Vec<LocaleTag> {
    let mut locales = Vec::<LocaleTag>::new();
    for recognizer in rulepacks.iter().flat_map(|rulepack| &rulepack.recognizers) {
        if !recognizer.enabled
            || recognizer.safety_tier != SafetyTier::LocaleGated
            || recognizer.locale_basis != LocaleBasis::Document
        {
            continue;
        }
        for locale in &recognizer.locales {
            if *locale != LocaleTag::Global && !locales.contains(locale) {
                locales.push(locale.clone());
            }
        }
    }
    locales.sort_by_key(activation_order_key);
    locales
}

fn activation_order_key(locale: &LocaleTag) -> (usize, String) {
    let rank = COMPATIBILITY_ACTIVATION_ORDER
        .iter()
        .position(|compatibility| compatibility == locale)
        .unwrap_or(COMPATIBILITY_ACTIVATION_ORDER.len());
    (rank, locale.as_str().to_string())
}

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
