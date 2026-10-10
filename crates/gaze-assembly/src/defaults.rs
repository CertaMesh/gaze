use std::collections::BTreeSet;
use std::path::PathBuf;

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    Policy, RawDocument, RuleSpec, Rulepack, Session,
};

use crate::{build_pipeline, resolve_policy_inputs, BuildError};

const CORE_BUNDLED_RULEPACK: &str = "core";

#[derive(Debug, Clone, Default)]
pub struct CorePipelineConfig {
    locale: Option<Vec<LocaleTag>>,
    extra_bundled: Vec<String>,
    extra_rulepack_paths: Vec<PathBuf>,
}

pub struct CorePipeline {
    pipeline: Pipeline,
    locale_chain: LocaleChain,
    dictionaries: DictionaryBundle,
}

impl CorePipelineConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_locale(mut self, locale: &[LocaleTag]) -> Self {
        self.locale = Some(locale.to_vec());
        self
    }

    pub fn with_bundled_rulepack(mut self, id: &str) -> Self {
        self.extra_bundled.push(id.to_string());
        self
    }

    pub fn with_rulepack_path(mut self, path: PathBuf) -> Self {
        self.extra_rulepack_paths.push(path);
        self
    }

    /// Bundled rulepacks selected by this configuration, in load order.
    pub fn bundled_rulepack_ids(&self) -> Vec<&str> {
        std::iter::once(CORE_BUNDLED_RULEPACK)
            .chain(
                self.extra_bundled
                    .iter()
                    .map(String::as_str)
                    .filter(|id| !matches!(*id, "core" | "core-extended")),
            )
            .collect()
    }

    pub fn build(self) -> Result<CorePipeline, BuildError> {
        let mut policy = default_policy(self.locale.clone(), Vec::new());
        policy.rulepacks.bundled = self
            .bundled_rulepack_ids()
            .into_iter()
            .map(str::to_string)
            .collect();
        policy.rulepacks.paths = self.extra_rulepack_paths;
        policy.rulepacks.auto_activate_locale_gated = self
            .extra_bundled
            .iter()
            .any(|bundle| bundle == "core-extended");
        let inputs = resolve_policy_inputs(&policy, None, None, None)?;
        policy.rules = class_rules_from_rulepacks(&inputs.rulepacks);
        let context = Context {
            dictionaries: std::collections::HashMap::new(),
            class_map: std::collections::HashMap::new(),
            fields: serde_json::Map::new(),
            record_match_kinds: Default::default(),
            record_value_rejections: Default::default(),
        };
        let pipeline = build_pipeline(
            &policy,
            &context,
            &inputs.rulepacks,
            &inputs.locale_chain,
            None,
        )?;

        Ok(CorePipeline {
            pipeline,
            locale_chain: inputs.locale_chain,
            dictionaries: inputs.dictionaries,
        })
    }
}

impl CorePipeline {
    /// Low-level pipeline access. Calls must pass [`Self::locale_chain`] and
    /// [`Self::dictionaries`] to retain configured detection inputs.
    pub fn pipeline(&self) -> &Pipeline {
        &self.pipeline
    }

    pub fn locale_chain(&self) -> &LocaleChain {
        &self.locale_chain
    }

    pub fn dictionaries(&self) -> &DictionaryBundle {
        &self.dictionaries
    }

    /// Low-level escape hatch that discards configured locales and dictionaries.
    /// Prefer [`Self::pseudonymize_text`] or [`Self::into_parts`]; callers of the
    /// returned pipeline must retain and pass both detection inputs themselves.
    pub fn into_pipeline(self) -> Pipeline {
        self.pipeline
    }

    /// Decompose without losing the locale and dictionary detection inputs.
    pub fn into_parts(self) -> (Pipeline, LocaleChain, DictionaryBundle) {
        (self.pipeline, self.locale_chain, self.dictionaries)
    }

    pub fn pseudonymize_text(
        &self,
        session: &Session,
        input: impl Into<String>,
    ) -> Result<CleanDocument, gaze::Error> {
        self.pipeline.pseudonymize_with_detect_context(
            session,
            RawDocument::Text(input.into()),
            self.locale_chain.as_slice(),
            &self.dictionaries,
        )
    }
}

fn class_rules_from_rulepacks(rulepacks: &[Rulepack]) -> Vec<RuleSpec> {
    let mut seen = BTreeSet::<PiiClass>::new();
    let mut rules = Vec::new();

    for recognizer in rulepacks
        .iter()
        .flat_map(|rulepack| rulepack.recognizers.iter())
        .filter(|recognizer| recognizer.enabled)
    {
        // note: MED-3 investigated 2026-05-08; loaded rulepack recognizer classes,
        // including core-extended custom classes, receive explicit Tokenize rules here.
        if seen.insert(recognizer.class.clone()) {
            rules.push(RuleSpec::Class {
                class: recognizer.class.clone(),
                action: Action::Tokenize,
            });
        }
        if let Some(family_class) = recognizer
            .collision
            .as_ref()
            .and_then(|collision| {
                collision
                    .mandatory_anchor
                    .as_ref()
                    .map(|_| &collision.family)
            })
            .map(|family| PiiClass::family(family))
        {
            if seen.insert(family_class.clone()) {
                rules.push(RuleSpec::Class {
                    class: family_class,
                    action: Action::Tokenize,
                });
            }
        }
    }

    rules.push(RuleSpec::Default {
        action: Action::Tokenize,
    });
    rules
}

fn default_policy(locale: Option<Vec<LocaleTag>>, rules: Vec<RuleSpec>) -> Policy {
    let mut policy = Policy::default();
    policy.rules = rules;
    policy.locale = locale;
    policy
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze::RulepackSource;

    #[test]
    fn default_policy_unseen_class_does_not_preserve() {
        let core = Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded(CORE_BUNDLED_RULEPACK).expect("core rulepack"),
        ))
        .expect("core loads");
        let policy = default_policy(None, class_rules_from_rulepacks(&[core]));
        let default_action = policy
            .rules
            .iter()
            .find_map(|rule| match rule {
                RuleSpec::Default { action } => Some(*action),
                _ => None,
            })
            .expect("default rule");

        assert_eq!(default_action, Action::Tokenize);
        assert_ne!(default_action, Action::Preserve);
    }

    /// `gaze mcp serve` and policy-less `gaze proxy` build their pipeline from
    /// `CorePipelineConfig::new()`; pin that it carries the `core` floor (#3706).
    #[test]
    fn core_pipeline_config_tokenizes_the_core_floor() {
        let core = CorePipelineConfig::new().build().expect("core pipeline");
        let session = Session::new(gaze::Scope::Ephemeral).expect("session");
        let input = "Card 4111 1111 1111 1111 ok, IBAN AT61 1904 3002 3457 3201 bitte, \
                     ip 10.1.2.3, mail jane.roe@example.com";
        let CleanDocument::Text(clean) = core.pseudonymize_text(&session, input).expect("clean")
        else {
            panic!("text in, text out");
        };

        assert_eq!(core.locale_chain().as_slice(), &[LocaleTag::Global]);
        for raw in [
            "4111 1111 1111 1111",
            "AT61 1904 3002 3457 3201",
            "10.1.2.3",
            "jane.roe@example.com",
        ] {
            assert!(!clean.contains(raw), "raw {raw:?} leaked: {clean}");
        }
        for token in [
            ":Custom:credit_card_1>",
            ":Custom:iban_1>",
            ":Custom:ip_address_1>",
            ":Email_1>",
        ] {
            assert!(clean.contains(token), "missing {token}: {clean}");
        }
    }
}
