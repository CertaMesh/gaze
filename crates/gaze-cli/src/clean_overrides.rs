use std::path::PathBuf;

use gaze::{NerPolicy, Policy, RulepackPolicy, SessionScope, DEFAULT_NER_THRESHOLD};

#[derive(Debug, Clone, Default)]
pub struct CleanOverrides {
    pub session_scope: Option<SessionScope>,
    pub ner_model_dir: Option<PathBuf>,
    pub ner_locale: Option<String>,
    pub rulepack_bundled: Option<Vec<String>>,
    pub rulepack_paths: Vec<PathBuf>,
    pub auto_activate_locale_gated: bool,
}

impl CleanOverrides {
    pub fn apply_to(&self, policy: &Policy) -> Policy {
        let mut resolved = policy.clone();
        resolved.session.scope = self
            .session_scope
            .clone()
            .or(Some(policy.session.scope.clone()))
            .unwrap_or(SessionScope::Persistent);

        let policy_ner = policy.ner.as_ref();
        let ner_model_dir = self
            .ner_model_dir
            .clone()
            .or_else(|| policy_ner.and_then(|ner| ner.model_dir.clone()))
            .or(None);
        let ner_locale = self
            .ner_locale
            .clone()
            .or_else(|| policy_ner.and_then(|ner| ner.locale.clone()))
            .or(None);
        let ner_threshold = policy_ner
            .map(|ner| ner.threshold)
            .unwrap_or(DEFAULT_NER_THRESHOLD);
        resolved.ner = if policy_ner.is_some() || ner_model_dir.is_some() || ner_locale.is_some() {
            let mut ner = NerPolicy::default();
            ner.model_dir = ner_model_dir;
            ner.locale = ner_locale;
            ner.threshold = ner_threshold;
            Some(ner)
        } else {
            None
        };

        resolved.rulepacks = RulepackPolicy::default();
        resolved.rulepacks.bundled = self
            .rulepack_bundled
            .clone()
            .or(Some(policy.rulepacks.bundled.clone()))
            .unwrap_or_else(|| vec!["core".to_string()]);
        resolved.rulepacks.paths = if self.rulepack_paths.is_empty() {
            policy.rulepacks.paths.clone()
        } else {
            self.rulepack_paths.clone()
        };
        resolved.rulepacks.auto_activate_locale_gated =
            policy.rulepacks.auto_activate_locale_gated || self.auto_activate_locale_gated;
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze::{Action, RuleSpec, SessionPolicy};

    fn policy() -> Policy {
        let mut session = SessionPolicy::default();
        session.scope = SessionScope::Conversation;

        let mut ner = NerPolicy::default();
        ner.model_dir = Some(PathBuf::from("/policy/model"));
        ner.locale = Some("de-DE".to_string());
        ner.threshold = 0.7;

        let mut rulepacks = RulepackPolicy::default();
        rulepacks.bundled = vec!["locale-de".to_string()];
        rulepacks.paths = vec![PathBuf::from("/policy/rulepack.toml")];

        let mut policy = Policy::default();
        policy.session = session;
        policy.rules = vec![RuleSpec::Default {
            action: Action::Preserve,
        }];
        policy.ner = Some(ner);
        policy.rulepacks = rulepacks;
        policy
    }

    #[test]
    fn session_scope_resolves_cli_policy_then_default() {
        let policy = policy();
        let cli = CleanOverrides {
            session_scope: Some(SessionScope::Ephemeral),
            ..Default::default()
        };
        assert_eq!(cli.apply_to(&policy).session.scope, SessionScope::Ephemeral);
        assert_eq!(
            CleanOverrides::default().apply_to(&policy).session.scope,
            SessionScope::Conversation
        );
    }

    #[test]
    fn ner_model_dir_resolves_cli_policy_then_absent_default() {
        let policy = policy();
        let cli = CleanOverrides {
            ner_model_dir: Some(PathBuf::from("/cli/model")),
            ..Default::default()
        };
        assert_eq!(
            cli.apply_to(&policy)
                .ner
                .as_ref()
                .and_then(|ner| ner.model_dir.as_deref()),
            Some(std::path::Path::new("/cli/model"))
        );
        assert_eq!(
            CleanOverrides::default()
                .apply_to(&policy)
                .ner
                .as_ref()
                .and_then(|ner| ner.model_dir.as_deref()),
            Some(std::path::Path::new("/policy/model"))
        );

        let mut no_ner_policy = policy;
        no_ner_policy.ner = None;
        assert!(
            CleanOverrides::default()
                .apply_to(&no_ner_policy)
                .ner
                .is_none(),
            "absent NER defaults must not register a detector"
        );
    }

    #[test]
    fn ner_locale_resolves_cli_policy_then_absent_default() {
        let policy = policy();
        let cli = CleanOverrides {
            ner_locale: Some("en-US".to_string()),
            ..Default::default()
        };
        assert_eq!(
            cli.apply_to(&policy)
                .ner
                .as_ref()
                .and_then(|ner| ner.locale.as_deref()),
            Some("en-US")
        );
        assert_eq!(
            CleanOverrides::default()
                .apply_to(&policy)
                .ner
                .as_ref()
                .and_then(|ner| ner.locale.as_deref()),
            Some("de-DE")
        );

        let mut no_ner_policy = policy;
        no_ner_policy.ner = None;
        assert!(
            CleanOverrides::default()
                .apply_to(&no_ner_policy)
                .ner
                .is_none(),
            "absent NER defaults must not register a detector"
        );
    }

    #[test]
    fn bundled_rulepacks_resolve_cli_policy_then_default() {
        let policy = policy();
        let cli = CleanOverrides {
            rulepack_bundled: Some(vec!["core".to_string(), "locale-en".to_string()]),
            ..Default::default()
        };
        assert_eq!(
            cli.apply_to(&policy).rulepacks.bundled,
            vec!["core", "locale-en"]
        );
        assert_eq!(
            CleanOverrides::default()
                .apply_to(&policy)
                .rulepacks
                .bundled,
            vec!["locale-de"]
        );
    }

    #[test]
    fn rulepack_paths_resolve_cli_policy_then_default() {
        let policy = policy();
        let cli = CleanOverrides {
            rulepack_paths: vec![PathBuf::from("/cli/rulepack.toml")],
            ..Default::default()
        };
        assert_eq!(
            cli.apply_to(&policy).rulepacks.paths,
            vec![PathBuf::from("/cli/rulepack.toml")]
        );
        assert_eq!(
            CleanOverrides::default().apply_to(&policy).rulepacks.paths,
            vec![PathBuf::from("/policy/rulepack.toml")]
        );
    }
}
