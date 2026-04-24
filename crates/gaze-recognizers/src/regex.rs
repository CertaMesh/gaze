use gaze::{Candidate, DetectContext, Detection, Detector, Error, PiiClass, Recognizer, Result};
use regex::Regex;

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

impl Recognizer for RegexDetector {
    fn id(&self) -> &str {
        &self.source
    }

    fn supported_class(&self) -> &PiiClass {
        &self.class
    }

    fn detect(&self, input: &str, _ctx: &DetectContext<'_>) -> Vec<Candidate> {
        self.regex
            .find_iter(input)
            .map(|m| Candidate {
                span: m.range(),
                class: self.class.clone(),
                recognizer_id: self.source.clone(),
                score: 0.70,
                canonical_form: None,
                token_family: self.token_family().to_string(),
                source: self.source.clone(),
            })
            .collect()
    }

    fn token_family(&self) -> &str {
        "counter"
    }
}
