use std::ops::Range;

use serde::{Deserialize, Serialize};

pub trait Detector: Send + Sync {
    fn detect(&self, input: &str) -> Vec<Detection>;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PiiClass {
    Email,
    Name,
    Location,
    Organization,
    Custom(String),
}

pub const BUILTIN_CLASS_NAMES: &[&str] = &["Email", "Name", "Location", "Organization"];

impl PiiClass {
    pub fn builtin_variants() -> &'static [PiiClass] {
        const _EXHAUSTIVE: fn(&PiiClass) = |c| match c {
            PiiClass::Email
            | PiiClass::Name
            | PiiClass::Location
            | PiiClass::Organization
            | PiiClass::Custom(_) => (),
        };
        &[
            PiiClass::Email,
            PiiClass::Name,
            PiiClass::Location,
            PiiClass::Organization,
        ]
    }

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
