use std::collections::BTreeSet;
use std::path::PathBuf;

use gaze::{
    Action, CleanDocument, Context, LocaleChain, LocaleTag, PiiClass, Pipeline, Policy,
    PolicyError, RawDocument, RuleSpec, Rulepack, RulepackSource, Session,
};

use crate::{build_pipeline, BuildError};

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

    pub fn build(self) -> Result<CorePipeline, BuildError> {
        let rulepacks = self.load_rulepacks()?;
        let rulepack_defaults = merged_rulepack_default_locales(&rulepacks);
        let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(
            None,
            self.locale.as_deref(),
            Some(&rulepack_defaults),
        );
        let policy = default_policy(self.locale, class_rules_from_rulepacks(&rulepacks));
        let context = Context {
            dictionaries: std::collections::HashMap::new(),
            class_map: std::collections::HashMap::new(),
            fields: serde_json::Map::new(),
        };
        let pipeline = build_pipeline(&policy, &context, &rulepacks, &locale_chain, None)?;

        Ok(CorePipeline {
            pipeline,
            locale_chain,
        })
    }

    fn load_rulepacks(&self) -> Result<Vec<Rulepack>, BuildError> {
        let mut rulepacks = Vec::new();
        let contents = load_embedded_rulepack_contents(CORE_BUNDLED_RULEPACK)?;
        rulepacks.push(Rulepack::load(RulepackSource::Embedded(contents))?);

        for bundled in &self.extra_bundled {
            let contents = load_embedded_rulepack_contents(bundled)?;
            rulepacks.push(Rulepack::load(RulepackSource::Embedded(contents))?);
        }
        for path in &self.extra_rulepack_paths {
            rulepacks.push(Rulepack::load(RulepackSource::Path(path.clone()))?);
        }

        Ok(rulepacks)
    }
}

impl CorePipeline {
    pub fn pipeline(&self) -> &Pipeline {
        &self.pipeline
    }

    pub fn locale_chain(&self) -> &LocaleChain {
        &self.locale_chain
    }

    pub fn into_pipeline(self) -> Pipeline {
        self.pipeline
    }

    pub fn redact_text(
        &self,
        session: &Session,
        input: impl Into<String>,
    ) -> Result<CleanDocument, gaze::Error> {
        self.pipeline.redact_with_context(
            session,
            RawDocument::Text(input.into()),
            self.locale_chain.as_slice(),
        )
    }
}

fn load_embedded_rulepack_contents(id: &str) -> Result<&'static str, BuildError> {
    gaze_recognizers::embedded(id).ok_or_else(|| {
        BuildError::Policy(PolicyError::BundledRulepackUnknown {
            value: id.to_string(),
        })
    })
}

fn merged_rulepack_default_locales(rulepacks: &[Rulepack]) -> Vec<LocaleTag> {
    let mut locales = Vec::new();
    for rulepack in rulepacks {
        for locale in &rulepack.default_locales {
            if !locales.iter().any(|existing| existing == locale) {
                locales.push(locale.clone());
            }
        }
    }
    locales
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
    }

    rules.push(RuleSpec::Default {
        action: Action::Preserve,
    });
    rules
}

fn default_policy(locale: Option<Vec<LocaleTag>>, rules: Vec<RuleSpec>) -> Policy {
    let mut policy = Policy::default();
    policy.rules = rules;
    policy.locale = locale;
    policy
}
