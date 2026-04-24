use gaze::{
    Candidate, ConflictTier, DetectContext, Detection, Detector, Error, LocaleTag, PiiClass,
    Recognizer, Result, RulepackError,
};
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatorKind {
    EmailRfc,
}

impl ValidatorKind {
    pub fn parse(s: &str) -> std::result::Result<Self, RulepackError> {
        match s {
            "email_rfc" => Ok(Self::EmailRfc),
            other => Err(RulepackError::UnsupportedValidator {
                kind: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizerKind {
    EmailCanonical,
}

impl NormalizerKind {
    pub fn parse(s: &str) -> std::result::Result<Self, RulepackError> {
        match s {
            "email_canonical" => Ok(Self::EmailCanonical),
            other => Err(RulepackError::UnsupportedNormalizer {
                kind: other.to_string(),
            }),
        }
    }
}

pub struct RegexDetector {
    regex: Regex,
    class: PiiClass,
    source: String,
    locales: Vec<LocaleTag>,
    base_score: f32,
    priority: i32,
    token_family: String,
    token_format: String,
    exclusions: Vec<String>,
    validator_kind: Option<ValidatorKind>,
    normalizer_kind: Option<NormalizerKind>,
}

impl RegexDetector {
    pub fn new(pattern: &str, class: PiiClass) -> Result<Self> {
        Self::with_source(pattern, class, "regex")
    }

    pub fn with_source(pattern: &str, class: PiiClass, source: &str) -> Result<Self> {
        Self::with_rulepack_fields(
            pattern,
            class,
            source,
            vec![LocaleTag::Global],
            0.70,
            0,
            "counter",
            "{Class}_{n}",
            Vec::new(),
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_rulepack_fields(
        pattern: &str,
        class: PiiClass,
        source: &str,
        locales: Vec<LocaleTag>,
        base_score: f32,
        priority: i32,
        token_family: &str,
        token_format: &str,
        exclusions: Vec<String>,
        validator_kind: Option<ValidatorKind>,
        normalizer_kind: Option<NormalizerKind>,
    ) -> Result<Self> {
        let regex = Regex::new(pattern).map_err(Error::InvalidRegex)?;
        Ok(Self {
            regex,
            class,
            source: source.to_string(),
            locales,
            base_score,
            priority,
            token_family: token_family.to_string(),
            token_format: token_format.to_string(),
            exclusions,
            validator_kind,
            normalizer_kind,
        })
    }

    pub fn emails() -> Result<Self> {
        Self::new(
            r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b",
            PiiClass::Email,
        )
    }

    pub fn token_format(&self) -> &str {
        &self.token_format
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
            .filter(|m| !self.is_excluded(m.as_str()))
            .map(|m| Candidate {
                span: m.range(),
                class: self.class.clone(),
                recognizer_id: self.source.clone(),
                score: self.base_score,
                priority: self.priority,
                canonical_form: self.canonical_form(m.as_str()),
                token_family: self.token_family().to_string(),
                source: self.source.clone(),
                decided_by: ConflictTier::None,
                merged_sources: Vec::new(),
            })
            .collect()
    }

    fn token_family(&self) -> &str {
        &self.token_family
    }

    fn locales(&self) -> &[LocaleTag] {
        &self.locales
    }
}

impl RegexDetector {
    fn is_excluded(&self, matched: &str) -> bool {
        self.exclusions
            .iter()
            .any(|excluded| matched.eq_ignore_ascii_case(excluded) || matched.contains(excluded))
    }

    fn canonical_form(&self, matched: &str) -> Option<String> {
        match self.validator_kind {
            Some(ValidatorKind::EmailRfc) if is_basic_email(matched) => {
                Some(match self.normalizer_kind {
                    Some(NormalizerKind::EmailCanonical) => matched.to_ascii_lowercase(),
                    _ => matched.to_string(),
                })
            }
            Some(_) => None,
            None => None,
        }
    }
}

fn is_basic_email(input: &str) -> bool {
    let Some((local, domain)) = input.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_rfc_validator_kind_populates_canonical_form() {
        let detector = RegexDetector::with_rulepack_fields(
            r"(?i)\b[a-z0-9._%+\-]+@example\.invalid\b",
            PiiClass::Email,
            "email.test",
            vec![LocaleTag::Global],
            0.70,
            0,
            "counter",
            "{Class}_{n}",
            Vec::new(),
            Some(ValidatorKind::EmailRfc),
            Some(NormalizerKind::EmailCanonical),
        )
        .expect("regex detector");
        let fields = serde_json::Map::new();
        let dictionaries = gaze::DictionaryBundle;
        let ctx = DetectContext {
            locale_chain: &[LocaleTag::Global],
            dictionaries: &dictionaries,
            fields: &fields,
            degraded: std::cell::Cell::new(false),
        };
        let detections = Recognizer::detect(&detector, "Email Alice@Example.invalid", &ctx);

        assert_eq!(
            detections[0].canonical_form.as_deref(),
            Some("alice@example.invalid")
        );
    }
}
