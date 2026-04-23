use std::ops::Range;

use serde::{Deserialize, Serialize};

use regex::Regex;

use crate::{Error, Result};

pub trait Detector: Send + Sync {
    fn detect(&self, input: &str) -> Vec<Detection>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PiiClass {
    Email,
    Name,
    Location,
    Organization,
    Custom(String),
}

pub(crate) const BUILTIN_CLASS_NAMES: &[&str] =
    &["Email", "Name", "Location", "Organization"];

impl PiiClass {
    pub fn custom(name: &str) -> Self {
        let mut normalized = String::new();
        let mut pending_underscore = false;
        for ch in name.trim().chars() {
            if ch.is_ascii_alphanumeric() {
                if pending_underscore && !normalized.is_empty() {
                    normalized.push('_');
                }
                normalized.push(ch.to_ascii_lowercase());
                pending_underscore = false;
            } else {
                pending_underscore = true;
            }
        }

        Self::Custom(normalized)
    }

    pub fn as_custom_name(&self) -> Option<&str> {
        match self {
            Self::Custom(name) => Some(name.as_str()),
            _ => None,
        }
    }

    pub(crate) fn class_name(&self) -> String {
        match self {
            Self::Email => BUILTIN_CLASS_NAMES[0].to_string(),
            Self::Name => BUILTIN_CLASS_NAMES[1].to_string(),
            Self::Location => BUILTIN_CLASS_NAMES[2].to_string(),
            Self::Organization => BUILTIN_CLASS_NAMES[3].to_string(),
            Self::Custom(name) => format!("Custom:{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub span: Range<usize>,
    pub class: PiiClass,
    pub source: String,
}

pub struct RegexDetector {
    regex: Regex,
    class: PiiClass,
    source: String,
}

impl RegexDetector {
    pub fn new(pattern: &str, class: PiiClass) -> Result<Self> {
        Self::with_source(pattern, class, "regex")
    }

    pub fn with_source(pattern: &str, class: PiiClass, source: &str) -> Result<Self> {
        let regex = Regex::new(pattern).map_err(Error::InvalidRegex)?;
        Ok(Self {
            regex,
            class,
            source: source.to_string(),
        })
    }

    pub fn emails() -> Result<Self> {
        Self::new(
            r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b",
            PiiClass::Email,
        )
    }
}

impl Detector for RegexDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        self.regex
            .find_iter(input)
            .map(|m| Detection {
                span: m.range(),
                class: self.class.clone(),
                source: self.source.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::PiiClass;

    #[test]
    fn custom_name_normalizes_without_reserved_rewrite() {
        assert_eq!(
            PiiClass::custom("email"),
            PiiClass::Custom("email".to_string())
        );
    }
}
