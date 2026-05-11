use anyhow::{bail, Context, Result};
use gaze::{RecognizerRegistry, Rulepack, RulepackSource};

const BUNDLED_RULEPACKS: &[&str] = &["core", "core-extended", "locale-de", "locale-en"];

pub fn run() -> Result<()> {
    for name in BUNDLED_RULEPACKS {
        let raw = gaze_recognizers::embedded(name)
            .with_context(|| format!("missing bundled rulepack `{name}`"))?;
        let rulepack = Rulepack::load(RulepackSource::Embedded(raw))
            .with_context(|| format!("failed to parse bundled rulepack `{name}`"))?;
        let mut builder = RecognizerRegistry::builder();
        let mut collision_count = 0usize;
        for recognizer in rulepack.recognizers {
            if let Some(collision) = recognizer.collision {
                collision_count += 1;
                builder = builder.register_collision(recognizer.id, collision);
            }
        }
        let registry = builder.build();
        if *name == "core-extended" {
            if collision_count < 5 {
                bail!(
                    "family-policy-table-coherence: core-extended expected at least 5 collision declarations, found {collision_count}"
                );
            }
            if registry
                .family_policy()
                .compare("iban.structural", "card.structural")
                != Some(true)
            {
                bail!("family-policy-table-coherence: IBAN must outrank PAN");
            }
            if registry
                .family_policy()
                .compare("phone.structural", "phone.national.de")
                .is_some()
            {
                bail!("family-policy-table-coherence: same phone variant must not arbitrate");
            }
        }
    }

    println!("family-policy-table-coherence: passed");
    Ok(())
}
