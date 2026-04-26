pub use gaze_types::{LocaleChain, LocaleError, LocaleTag};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_locale_tags() {
        assert_eq!(LocaleTag::parse("global"), Ok(LocaleTag::Global));
        assert_eq!(LocaleTag::parse("de-DE"), Ok(LocaleTag::DeDe));
        assert_eq!(LocaleTag::parse("en_IE"), Ok(LocaleTag::EnIe));
    }

    #[test]
    fn display_uses_canonical_tag() {
        assert_eq!(LocaleTag::EnGb.to_string(), "en-GB");
        assert_eq!(LocaleTag::parse("pt-BR").unwrap().to_string(), "pt-BR");
    }

    #[test]
    fn rejects_invalid_locale_tags() {
        assert_eq!(
            LocaleTag::parse("not a locale"),
            Err(LocaleError::Unsupported)
        );
        assert_eq!(LocaleTag::parse(""), Err(LocaleError::Unsupported));
    }

    #[test]
    fn cli_chain_appends_global() {
        let chain = LocaleChain::from_cli("de-DE,en-US").unwrap();
        assert_eq!(chain.to_strings(), vec!["de-DE", "en-US", "global"]);
    }

    #[test]
    fn cli_chain_replaces_policy_chain_when_explicit() {
        let policy = [LocaleTag::EnGb, LocaleTag::Global];
        let cli = [LocaleTag::DeDe];
        let chain = LocaleChain::merge_policy_and_cli(Some(&policy), Some(&cli));
        assert_eq!(chain.to_strings(), vec!["de-DE", "global"]);
    }

    #[test]
    fn merge_includes_rulepack_default_between_policy_and_system_default() {
        let rulepack = [LocaleTag::EnUs];
        let chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(&rulepack));
        assert_eq!(chain.to_strings(), vec!["en-US", "global"]);

        let policy = [LocaleTag::DeDe];
        let chain =
            LocaleChain::merge_cli_policy_rulepack_default(None, Some(&policy), Some(&rulepack));
        assert_eq!(chain.to_strings(), vec!["de-DE", "global"]);
    }

    #[test]
    fn intersection_treats_global_as_universal() {
        let chain = LocaleChain::from_cli("fr-FR").unwrap();
        assert!(chain.intersects(&[LocaleTag::Global]));
        assert!(chain.intersects(&[LocaleTag::Other("fr-FR".to_string())]));
        assert!(!chain.intersects(&[LocaleTag::DeDe]));
    }

    #[test]
    fn locale_other_does_not_match_unrelated_other() {
        let chain = LocaleChain::from_cli("en-US").unwrap();
        assert!(!chain.intersects(&[LocaleTag::Other("fr-FR".to_string())]));
    }

    #[test]
    fn locale_other_matches_same_other_in_chain() {
        let chain = LocaleChain::from_cli("zh-CN").unwrap();
        assert!(chain.intersects(&[LocaleTag::Other("zh-CN".to_string())]));
    }
}
