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
