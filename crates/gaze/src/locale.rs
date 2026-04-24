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

impl LocaleTag {
    pub const GLOBAL: LocaleTag = LocaleTag::Global;

    pub fn parse(s: &str) -> Result<LocaleTag, LocaleError> {
        let normalized = s.trim().replace('_', "-").to_ascii_lowercase();
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
            other => Ok(LocaleTag::Other(other.to_string())),
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

impl fmt::Display for LocaleTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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
        assert_eq!(LocaleTag::Other("pt-br".to_string()).as_str(), "pt-br");
    }
}
