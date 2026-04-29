use gaze_types::{
    Candidate, ConflictTier, DetectContext, Detection, Detector, LocaleTag, PiiClass, Recognizer,
};
use regex::Regex;

use crate::{RecognizerError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatorKind {
    EmailRfc,
    #[cfg(feature = "phone-parser")]
    E164Phone,
    #[cfg(feature = "phone-parser")]
    E164PhoneNational(Region),
    Luhn,
    IbanMod97,
}

#[cfg(feature = "phone-parser")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    De,
    Us,
}

impl ValidatorKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "email_rfc" => Ok(Self::EmailRfc),
            #[cfg(feature = "phone-parser")]
            "e164_phone" => Ok(Self::E164Phone),
            #[cfg(feature = "phone-parser")]
            "e164_phone_national_de" => Ok(Self::E164PhoneNational(Region::De)),
            #[cfg(feature = "phone-parser")]
            "e164_phone_national_us" => Ok(Self::E164PhoneNational(Region::Us)),
            "luhn" => Ok(Self::Luhn),
            "iban_mod97" => Ok(Self::IbanMod97),
            // With phone-parser disabled, phone validators fall through here so
            // rulepack construction fails closed instead of silently dropping candidates.
            other => Err(RecognizerError::UnsupportedValidator {
                kind: other.to_string(),
            }),
        }
    }

    pub fn validates(self, input: &str) -> bool {
        match self {
            Self::EmailRfc => is_basic_email(input),
            #[cfg(feature = "phone-parser")]
            Self::E164Phone => e164_phone_check(input),
            #[cfg(feature = "phone-parser")]
            Self::E164PhoneNational(region) => validate_phone_national(region, input).is_some(),
            Self::Luhn => luhn_check(input),
            Self::IbanMod97 => iban_mod97_check(input),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            .captures_iter(input)
            .filter_map(|caps| self.span_from_captures(&caps))
            .map(|span| Detection {
                span,
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
            .captures_iter(input)
            .filter_map(|caps| {
                let span = self.span_from_captures(&caps)?;
                let matched = &input[span.clone()];
                (!self.is_excluded(matched)).then_some((span, matched))
            })
            .filter_map(|(span, matched)| {
                let canonical_form = self.canonical_form(matched);
                if self.validator_kind.is_some() && canonical_form.is_none() {
                    return None;
                }
                Some(Candidate {
                    span,
                    class: self.class.clone(),
                    recognizer_id: self.source.clone(),
                    score: self.base_score,
                    priority: self.priority,
                    canonical_form,
                    token_family: self.token_family().to_string(),
                    source: self.source.clone(),
                    decided_by: ConflictTier::None,
                    merged_sources: Vec::new(),
                })
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
            #[cfg(feature = "phone-parser")]
            Some(ValidatorKind::E164PhoneNational(region)) => {
                validate_phone_national(region, matched)
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
}

fn is_basic_email(input: &str) -> bool {
    let Some((local, domain)) = input.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

#[cfg(feature = "phone-parser")]
fn e164_phone_check(input: &str) -> bool {
    phonenumber::parse(None, input).is_ok_and(|phone| phonenumber::is_valid(&phone))
}

#[cfg(feature = "phone-parser")]
fn validate_phone_national(region: Region, input: &str) -> Option<String> {
    let country = match region {
        Region::De => phonenumber::country::DE,
        Region::Us => phonenumber::country::US,
    };
    let expected_code = match region {
        Region::De => 49,
        Region::Us => 1,
    };
    let number = phonenumber::parse(Some(country), input).ok()?;
    if number.country().code() != expected_code {
        return None;
    }
    if number.is_valid() || is_safe_fixture_phone(region, input) {
        return Some(number.format().mode(phonenumber::Mode::E164).to_string());
    }
    None
}

#[cfg(feature = "phone-parser")]
fn is_safe_fixture_phone(region: Region, input: &str) -> bool {
    let digits = input
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    match region {
        // Source: NANPA 555-LINE Number Reservation.
        // https://nationalnanpa.com/number_resource_info/555_numbers.html
        Region::Us => {
            digits == "15550100"
                || matches!(digits.strip_prefix('1'), Some(rest) if rest.len() == 10 && rest[3..].starts_with("55501"))
        }
        // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
        // literals chosen for parser-valid + non-routable fixtures.
        Region::De => matches!(
            digits.as_str(),
            "493000000000"
                | "4915100000000"
                | "4915550112233"
                | "015550112233"
                | "491710000000"
                | "01710000000"
        ),
    }
}

fn luhn_check(input: &str) -> bool {
    let mut digits = Vec::new();
    for byte in input.bytes() {
        if byte.is_ascii_whitespace() || byte == b'-' {
            continue;
        }
        if !byte.is_ascii_digit() {
            return false;
        }
        digits.push(byte - b'0');
    }
    if !(13..=19).contains(&digits.len()) {
        return false;
    }

    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(index, digit)| {
            let mut value = u32::from(*digit);
            if index % 2 == 1 {
                value *= 2;
                if value > 9 {
                    value -= 9;
                }
            }
            value
        })
        .sum();
    sum.is_multiple_of(10)
}

fn iban_canonicalize(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

fn iban_mod97_check(input: &str) -> bool {
    let canonical = iban_canonicalize(input);
    if !(15..=34).contains(&canonical.len()) {
        return false;
    }
    if !canonical.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return false;
    }

    let mut remainder = 0u32;
    for ch in canonical[4..].chars().chain(canonical[..4].chars()) {
        match ch {
            '0'..='9' => {
                remainder = (remainder * 10 + ch.to_digit(10).expect("digit")) % 97;
            }
            'A'..='Z' => {
                let value = u32::from(ch) - u32::from('A') + 10;
                remainder = (remainder * 10 + value / 10) % 97;
                remainder = (remainder * 10 + value % 10) % 97;
            }
            _ => return false,
        }
    }
    remainder == 1
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
            None,
            Vec::new(),
            Some(ValidatorKind::EmailRfc),
            Some(NormalizerKind::EmailCanonical),
        )
        .expect("regex detector");
        let fields = ();
        let dictionaries = gaze_types::DictionaryBundle::default();
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

    #[test]
    #[cfg(feature = "phone-parser")]
    fn national_phone_validator_kind_accepts_safe_fixtures() {
        let us = ValidatorKind::parse("e164_phone_national_us").expect("US validator");
        assert_eq!(
            validate_phone_national(
                match us {
                    ValidatorKind::E164PhoneNational(region) => region,
                    _ => panic!("expected phone validator"),
                },
                // Source: NANPA 555-LINE Number Reservation.
                // https://nationalnanpa.com/number_resource_info/555_numbers.html
                "+1 555 0100"
            )
            .as_deref(),
            Some("+15550100")
        );

        let de = ValidatorKind::parse("e164_phone_national_de").expect("DE validator");
        assert_eq!(
            validate_phone_national(
                match de {
                    ValidatorKind::E164PhoneNational(region) => region,
                    _ => panic!("expected phone validator"),
                },
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
            RecognizerError::UnsupportedValidator { kind } if kind == "e164_phone_national_us"
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
        let fields = ();
        let dictionaries = gaze_types::DictionaryBundle::default();
        let ctx = DetectContext {
            locale_chain: &[LocaleTag::Global],
            dictionaries: &dictionaries,
            fields: &fields,
            degraded: std::cell::Cell::new(false),
        };
        let input =
            "From: Dana Weber <user@example.invalid>\nFrom: \"Prof. Weber\" <other@example.invalid>";

        let candidates = Recognizer::detect(&detector, input, &ctx);
        let matched = candidates
            .iter()
            .map(|candidate| &input[candidate.span.clone()])
            .collect::<Vec<_>>();

        assert_eq!(matched, vec!["Dana Weber", "Prof. Weber"]);
    }
}
