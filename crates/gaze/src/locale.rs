use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LocaleTag {
    Global,
    DeDe,
    DeAt,
    DeCh,
    EnUs,
    EnGb,
    EnIe,
    EnAu,
    EnCa,
    Other(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LocaleError {
    #[error("unsupported locale")]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleChain(Vec<LocaleTag>);

impl LocaleTag {
    pub const GLOBAL: LocaleTag = LocaleTag::Global;

    pub fn parse(s: &str) -> Result<LocaleTag, LocaleError> {
        let raw = s.trim().replace('_', "-");
        let normalized = raw.to_ascii_lowercase();
        match normalized.as_str() {
            "global" | "*" => Ok(LocaleTag::Global),
            "de-de" => Ok(LocaleTag::DeDe),
            "de-at" => Ok(LocaleTag::DeAt),
            "de-ch" => Ok(LocaleTag::DeCh),
            "en-us" => Ok(LocaleTag::EnUs),
            "en-gb" => Ok(LocaleTag::EnGb),
            "en-ie" => Ok(LocaleTag::EnIe),
            "en-au" => Ok(LocaleTag::EnAu),
            "en-ca" => Ok(LocaleTag::EnCa),
            "" => Err(LocaleError::Unsupported),
            _ if is_bcp47_parseable(&raw) => Ok(LocaleTag::Other(canonical_other(&raw))),
            _ => Err(LocaleError::Unsupported),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            LocaleTag::Global => "global",
            LocaleTag::DeDe => "de-DE",
            LocaleTag::DeAt => "de-AT",
            LocaleTag::DeCh => "de-CH",
            LocaleTag::EnUs => "en-US",
            LocaleTag::EnGb => "en-GB",
            LocaleTag::EnIe => "en-IE",
            LocaleTag::EnAu => "en-AU",
            LocaleTag::EnCa => "en-CA",
            LocaleTag::Other(tag) => tag.as_str(),
        }
    }
}

impl LocaleChain {
    pub(crate) fn from_tags(mut tags: Vec<LocaleTag>) -> LocaleChain {
        ensure_global(&mut tags);
        LocaleChain(tags)
    }

    pub fn from_cli(raw: &str) -> Result<LocaleChain, LocaleError> {
        let tags = raw
            .split(',')
            .map(LocaleTag::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LocaleChain::from_tags(tags))
    }

    pub fn merge_policy_and_cli(
        policy: Option<&[LocaleTag]>,
        cli: Option<&[LocaleTag]>,
    ) -> LocaleChain {
        Self::merge_cli_policy_rulepack_default(cli, policy, None)
    }

    pub fn merge_cli_policy_rulepack_default(
        cli: Option<&[LocaleTag]>,
        policy: Option<&[LocaleTag]>,
        rulepack_defaults: Option<&[LocaleTag]>,
    ) -> LocaleChain {
        let tags = cli
            .filter(|tags| !tags.is_empty())
            .or_else(|| policy.filter(|tags| !tags.is_empty()))
            .or_else(|| rulepack_defaults.filter(|tags| !tags.is_empty()))
            .map(|tags| tags.to_vec())
            .unwrap_or_else(|| vec![LocaleTag::Global]);
        LocaleChain::from_tags(tags)
    }

    pub fn intersects(&self, recognizer_locales: &[LocaleTag]) -> bool {
        if recognizer_locales.is_empty() {
            return true;
        }
        recognizer_locales.iter().any(|recognizer_locale| {
            *recognizer_locale == LocaleTag::Global
                || matches!(recognizer_locale, LocaleTag::Other(_))
                || self.0.iter().any(|active| active == recognizer_locale)
        })
    }

    pub fn as_slice(&self) -> &[LocaleTag] {
        &self.0
    }

    pub fn to_strings(&self) -> Vec<String> {
        self.0.iter().map(ToString::to_string).collect()
    }
}

impl fmt::Display for LocaleTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn ensure_global(tags: &mut Vec<LocaleTag>) {
    if !tags.iter().any(|tag| *tag == LocaleTag::Global) {
        tags.push(LocaleTag::Global);
    }
}

fn is_bcp47_parseable(raw: &str) -> bool {
    let mut parts = raw.split('-');
    let Some(language) = parts.next() else {
        return false;
    };
    if !(2..=8).contains(&language.len()) || !language.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    parts.all(|part| {
        (2..=8).contains(&part.len()) && part.chars().all(|ch| ch.is_ascii_alphanumeric())
    })
}

fn canonical_other(raw: &str) -> String {
    let mut parts = raw.split('-');
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    let rest = parts.map(|part| {
        if part.len() == 2 && part.chars().all(|ch| ch.is_ascii_alphabetic()) {
            part.to_ascii_uppercase()
        } else {
            part.to_ascii_lowercase()
        }
    });
    std::iter::once(language)
        .chain(rest)
        .collect::<Vec<_>>()
        .join("-")
}

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
    fn intersection_treats_global_and_unshipped_other_as_fallback() {
        let chain = LocaleChain::from_cli("fr-FR").unwrap();
        assert!(chain.intersects(&[LocaleTag::Global]));
        assert!(chain.intersects(&[LocaleTag::Other("fr-FR".to_string())]));
        assert!(!chain.intersects(&[LocaleTag::DeDe]));
    }
}
