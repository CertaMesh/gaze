use gaze_types::{
    Candidate, ConflictTier, DetectContext, Detection, Detector, LocaleTag, PiiClass, Recognizer,
    ValidatorKind,
};
use regex::Regex;

use crate::{RecognizerError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NormalizerKind {
    EmailCanonical,
    IbanCanonical,
}

impl NormalizerKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "email_canonical" => Ok(Self::EmailCanonical),
            "iban_canonical" => Ok(Self::IbanCanonical),
            other => Err(RecognizerError::UnsupportedNormalizer {
                kind: other.to_string(),
            }),
        }
    }

    pub fn normalize(self, input: &str) -> String {
        match self {
            Self::EmailCanonical => input.to_ascii_lowercase(),
            Self::IbanCanonical => iban_canonicalize(input),
        }
    }
}

/// Regex-backed [`Recognizer`] implementation.
///
/// Construct via [`RegexDetector::emails`] for the bundled email recognizer, or
/// supply a custom pattern through the rulepack mechanism. Patterns use Rust
/// regex syntax: no lookahead, lookbehind, or backreferences. Prefer TOML
/// literal strings (`'...'`) in policy files to avoid double-escaping.
///
/// [`Candidate::span`] uses byte ranges, not char indices.
///
/// [`Candidate::span`]: gaze_types::Candidate::span
pub struct RegexDetector {
    regex: Regex,
    class: PiiClass,
    source: String,
    locales: Vec<LocaleTag>,
    base_score: f32,
    priority: i32,
    token_family: String,
    capture_groups: Option<Vec<u32>>,
    exclusions: Vec<String>,
    validator_kind: Option<ValidatorKind>,
    normalizer_kind: Option<NormalizerKind>,
    ascii_email_boundary: bool,
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
            None,
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
        capture_groups: Option<Vec<u32>>,
        exclusions: Vec<String>,
        validator_kind: Option<ValidatorKind>,
        normalizer_kind: Option<NormalizerKind>,
    ) -> Result<Self> {
        let regex = Regex::new(pattern).map_err(RecognizerError::InvalidRegex)?;
        Ok(Self {
            regex,
            class,
            source: source.to_string(),
            locales,
            base_score,
            priority,
            token_family: token_family.to_string(),
            capture_groups,
            exclusions,
            validator_kind,
            normalizer_kind,
            ascii_email_boundary: false,
        })
    }

    pub fn emails() -> Result<Self> {
        let mut detector = Self::new(
            r"(?i)[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}",
            PiiClass::Email,
        )?;
        detector.ascii_email_boundary = true;
        Ok(detector)
    }
}

impl Detector for RegexDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        self.regex
            .captures_iter(input)
            .filter_map(|caps| self.span_from_captures(&caps))
            .filter(|span| self.boundary_accepts(input, span))
            .map(|span| Detection::new(span, self.class.clone(), self.source.clone()))
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

    fn detect(
        &self,
        input: &str,
        _ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self
            .regex
            .captures_iter(input)
            .filter_map(|caps| {
                let span = self.span_from_captures(&caps)?;
                if !self.boundary_accepts(input, &span) {
                    return None;
                }
                let matched = &input[span.clone()];
                (!self.is_excluded(matched)).then_some((span, matched))
            })
            .map(|(span, matched)| {
                let canonical_form = self.canonical_form(matched);
                Candidate::new(
                    span,
                    self.class.clone(),
                    self.source.clone(),
                    self.base_score,
                    self.priority,
                    canonical_form,
                    self.token_family(),
                    self.source.clone(),
                    ConflictTier::None,
                    Vec::new(),
                )
            })
            .collect())
    }

    fn token_family(&self) -> &str {
        &self.token_family
    }

    fn validator_kind(&self) -> Option<ValidatorKind> {
        self.validator_kind
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
            #[cfg(feature = "phone-parser")]
            Some(ValidatorKind::E164PhoneNational(_)) => {
                self.validator_kind?.canonical_form(matched)
            }
            Some(validator_kind) if validator_kind.validates(matched) => {
                Some(self.normalizer_kind.map_or_else(
                    || matched.to_string(),
                    |normalizer| normalizer.normalize(matched),
                ))
            }
            Some(_) => None,
            None => None,
        }
    }

    fn span_from_captures(&self, caps: &regex::Captures<'_>) -> Option<std::ops::Range<usize>> {
        if let Some(groups) = &self.capture_groups {
            groups
                .iter()
                .filter_map(|group| caps.get(*group as usize))
                .find(|m| !m.as_str().is_empty())
                .map(|m| m.range())
        } else {
            caps.get(0).map(|m| m.range())
        }
    }

    fn boundary_accepts(&self, input: &str, span: &std::ops::Range<usize>) -> bool {
        if !self.ascii_email_boundary {
            return true;
        }

        let previous_ok = input[..span.start]
            .chars()
            .next_back()
            .map_or(true, |ch| !is_ascii_email_continuation(ch));
        let next_ok = input[span.end..]
            .chars()
            .next()
            .map_or(true, |ch| !is_ascii_email_continuation(ch));

        previous_ok && next_ok
    }
}

fn iban_canonicalize(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

fn is_ascii_email_continuation(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_email_detector_matches_before_non_ascii_letter() {
        let detector = RegexDetector::emails().expect("email detector");
        let detections = Detector::detect(&detector, "a@example.invalidø");

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].span, 0..17);
    }

    #[test]
    fn bundled_email_detector_matches_after_non_ascii_letter() {
        let detector = RegexDetector::emails().expect("email detector");
        let detections = Detector::detect(&detector, "øa@example.invalid");

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].span, 2..19);
    }

    #[test]
    fn bundled_email_detector_matches_comma_separated_addresses() {
        let detector = RegexDetector::emails().expect("email detector");
        let detections = Detector::detect(&detector, "a@example.invalid,b@example.invalid");

        assert_eq!(detections.len(), 2);
        assert_eq!(detections[0].span, 0..17);
        assert_eq!(detections[1].span, 18..35);
    }

    #[test]
    fn bundled_email_detector_rejects_ascii_continuation_suffix() {
        let detector = RegexDetector::emails().expect("email detector");
        let detections = Detector::detect(&detector, "a@example.invalid1");

        assert!(detections.is_empty());
    }

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
            None,
            Vec::new(),
            Some(ValidatorKind::EmailRfc),
            Some(NormalizerKind::EmailCanonical),
        )
        .expect("regex detector");
        let dictionaries = gaze_types::DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        let detections =
            Recognizer::detect(&detector, "Email Alice@Example.invalid", &ctx).unwrap();

        assert_eq!(
            detections[0].canonical_form.as_deref(),
            Some("alice@example.invalid")
        );
    }

    #[test]
    #[cfg(feature = "phone-parser")]
    fn national_phone_validator_kind_accepts_safe_fixtures() {
        let us = ValidatorKind::parse("e164_phone_national_us").expect("US validator");
        assert_eq!(
            us.canonical_form(
                // Source: NANPA 555-LINE Number Reservation.
                // https://nationalnanpa.com/number_resource_info/555_numbers.html
                "+1 555 0100"
            )
            .as_deref(),
            Some("+15550100")
        );

        let de = ValidatorKind::parse("e164_phone_national_de").expect("DE validator");
        assert_eq!(
            de.canonical_form(
                // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
                // literals chosen for parser-valid + non-routable.
                "+49 30 0000 0000"
            )
            .as_deref(),
            Some("+493000000000")
        );
    }

    #[test]
    #[cfg(not(feature = "phone-parser"))]
    fn national_phone_validator_kind_fails_closed_without_feature() {
        let err = ValidatorKind::parse("e164_phone_national_us")
            .expect_err("phone parser feature is disabled");
        assert!(matches!(
            err,
            gaze_types::ValidatorKindParseError::UnsupportedValidator { kind }
                if kind == "e164_phone_national_us"
        ));
    }

    #[test]
    fn regex_recognizer_uses_first_non_empty_capture_group() {
        let detector = RegexDetector::with_rulepack_fields(
            r#"(?m)^From:\s+(?:"([^"]+)"|([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+))\s+<[^>]+>"#,
            PiiClass::Name,
            "email.header.name",
            vec![LocaleTag::Global],
            0.90,
            0,
            "email.header.name",
            Some(vec![1, 2]),
            Vec::new(),
            None,
            None,
        )
        .expect("regex detector");
        let dictionaries = gaze_types::DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        let input =
            "From: Dana Weber <user@example.invalid>\nFrom: \"Prof. Weber\" <other@example.invalid>";

        let candidates = Recognizer::detect(&detector, input, &ctx).unwrap();
        let matched = candidates
            .iter()
            .map(|candidate| &input[candidate.span.clone()])
            .collect::<Vec<_>>();

        assert_eq!(matched, vec!["Dana Weber", "Prof. Weber"]);
    }
}
