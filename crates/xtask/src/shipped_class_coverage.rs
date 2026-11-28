//! Every class a bundled rulepack emits must be a first-class `PiiClass` variant.
//!
//! `PiiClass::Custom` is reserved for adopter and safety-net-only classes. A bundled
//! recognizer whose class, or whose collision family, still parses to `Custom` means a new
//! shipped class was added without its variant, so call sites that match on the variant
//! would silently skip it.

use anyhow::{bail, Context, Result};
use gaze::{PiiClass, Rulepack, RulepackSource};

pub fn run() -> Result<()> {
    let mut checked = 0usize;
    let mut violations = Vec::new();
    for (name, raw) in gaze_recognizers::embedded_rulepacks() {
        let rulepack = Rulepack::load(RulepackSource::Embedded(raw))
            .with_context(|| format!("failed to parse bundled rulepack `{name}`"))?;
        checked += rulepack.recognizers.len();
        violations.extend(
            custom_classes(&rulepack)
                .into_iter()
                .map(|violation| format!("{name}: {violation}")),
        );
    }
    violations.extend(first_class_round_trip_failures());
    if !violations.is_empty() {
        bail!(
            "shipped-class-coverage: {} violation(s):\n  {}",
            violations.len(),
            violations.join("\n  ")
        );
    }
    println!(
        "shipped-class-coverage: passed ({checked} bundled recognizers, {} first-class variants)",
        PiiClass::first_class_variants().len()
    );
    Ok(())
}

/// Recognizer classes and derived family classes that are still `PiiClass::Custom`.
fn custom_classes(rulepack: &Rulepack) -> Vec<String> {
    let mut violations = Vec::new();
    for recognizer in &rulepack.recognizers {
        if matches!(recognizer.class, PiiClass::Custom(_)) {
            violations.push(format!(
                "recognizer `{}` emits `{}`, which has no first-class PiiClass variant",
                recognizer.id,
                recognizer.class.to_canonical_str()
            ));
        }
        if let Some(collision) = &recognizer.collision {
            let family = PiiClass::family(&collision.family);
            if matches!(family, PiiClass::Custom(_)) {
                violations.push(format!(
                    "recognizer `{}` family `{}` is not a PiiClass::Ambiguous class",
                    recognizer.id,
                    family.to_canonical_str()
                ));
            }
        }
    }
    violations
}

/// First-class variants whose canonical spelling does not parse back to the same variant.
fn first_class_round_trip_failures() -> Vec<String> {
    PiiClass::first_class_variants()
        .iter()
        .filter_map(|class| {
            let canonical = class.to_canonical_str();
            match PiiClass::from_policy_name(&canonical) {
                Some(parsed) if &parsed == class && !matches!(parsed, PiiClass::Custom(_)) => None,
                parsed => Some(format!(
                    "first-class variant {class:?} spelled `{canonical}` parses to {parsed:?}"
                )),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rulepack(class: &str) -> Rulepack {
        let toml = format!(
            "schema_version = \"0.1.0\"\nrulepack_id = \"coverage-probe\"\n\
             rulepack_version = \"0.1.0\"\ndefault_locales = [\"global\"]\n\n\
             [[recognizers]]\nid = \"probe.rule\"\nclass = \"{class}\"\nenabled = true\n\
             locale_basis = \"document\"\nlocales = [\"global\"]\nsafety_tier = \"safe_default\"\n\n\
             [recognizers.match]\nkind = \"regex\"\npattern = '''probe-value'''\n"
        );
        Rulepack::load(RulepackSource::Embedded(Box::leak(toml.into_boxed_str())))
            .expect("probe rulepack loads")
    }

    #[test]
    fn an_unshipped_custom_class_is_reported() {
        let violations = custom_classes(&rulepack("custom:zzz"));
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].contains("probe.rule") && violations[0].contains("custom:zzz"));
    }

    #[test]
    fn shipped_and_builtin_classes_pass() {
        for class in ["custom:phone", "custom:postal_code", "email", "name"] {
            assert!(custom_classes(&rulepack(class)).is_empty(), "{class}");
        }
    }

    #[test]
    fn every_bundled_rulepack_and_variant_passes() {
        for (name, raw) in gaze_recognizers::embedded_rulepacks() {
            let rulepack = Rulepack::load(RulepackSource::Embedded(raw)).expect("bundled loads");
            assert!(custom_classes(&rulepack).is_empty(), "{name}");
        }
        assert!(first_class_round_trip_failures().is_empty());
    }
}
