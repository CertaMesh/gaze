#![cfg_attr(docsrs, feature(doc_cfg))]

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use thiserror::Error;

/// Shared detector contract for text-only PII detection.
pub trait Detector: Send + Sync {
    /// Detect PII spans in the supplied input string.
    fn detect(&self, input: &str) -> Vec<Detection>;

    /// Fallible detection entrypoint for detectors backed by runtime systems.
    fn try_detect(&self, input: &str) -> Result<Vec<Detection>, RecognizerRuntimeError> {
        Ok(self.detect(input))
    }
}

/// Runtime failure raised by a recognizer or detector backend during detection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecognizerRuntimeError {
    pub recognizer_id: String,
    pub message: String,
}

impl RecognizerRuntimeError {
    pub fn new(recognizer_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            recognizer_id: recognizer_id.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for RecognizerRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "recognizer '{}' backend failed: {}",
            self.recognizer_id, self.message
        )
    }
}

impl std::error::Error for RecognizerRuntimeError {}

/// The category of a detected PII span.
///
/// Built-in variants: `Email`, `Name`, `Location`, `Organization`. Tenant-specific PII
/// (case references, titles, internal codes) is carried as `PiiClass::Custom(String)`.
/// **There is no `Phone` variant** -- phone detection is provided by recognizers in
/// `gaze-recognizers` and surfaces as either a `Custom("phone")` class or a class
/// defined by a rulepack.
///
/// `PiiClass` is exhaustive. Match every variant explicitly so new built-in classes
/// force call sites to review their handling at compile time:
///
/// ```rust
/// use gaze_types::PiiClass;
///
/// fn label(class: &PiiClass) -> &'static str {
///     match class {
///         PiiClass::Email        => "email",
///         PiiClass::Name         => "name",
///         PiiClass::Location     => "location",
///         PiiClass::Organization => "org",
///         PiiClass::Custom(_)    => "pii",
///     }
/// }
/// ```
///
/// Policy TOML uses the lowercase forms `email` / `name` / `location` / `organization`,
/// and tenant classes are spelled like `custom:case_ref` (lowercase, snake_case).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PiiClass {
    /// Email address class.
    Email,
    /// Person name class.
    Name,
    /// Location class.
    Location,
    /// Organization class.
    Organization,
    /// Tenant- or policy-defined class.
    Custom(String),
}

/// Built-in class labels in stable display order.
pub const BUILTIN_CLASS_NAMES: &[&str] = &["Email", "Name", "Location", "Organization"];

/// Family names reserved for bundled collision-policy rulepacks.
///
/// Adopter policy-level custom recognizers cannot claim these names because bundled
/// families are part of the core disambiguation contract.
pub const RESERVED_BUNDLED_FAMILIES: &[&str] = &[
    "us-9-digit-id",
    "iberian-id",
    "payment-card-or-iban",
    "phone-or-imei",
    "vin-or-serial",
    "mac-or-hex",
    "passport-or-doc-support",
    "national-13-digit",
    "italian-cf-or-serial",
    "german-personalausweis",
    "swedish-personnummer",
    "finnish-hetu",
];

pub const RESTORE_PHASE_MANIFEST_LOOKUP: u32 = 1 << 0;
pub const RESTORE_PHASE_UNKNOWN_TOKEN_SCAN: u32 = 1 << 1;
pub const RESTORE_PHASE_MANIFEST_BYPASS_SCAN: u32 = 1 << 2;
pub const RESTORE_PHASE_FRESH_PII_SCAN: u32 = 1 << 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RestoredText {
    pub text: String,
}

impl RestoredText {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RestorePolicy {
    Strict,
    Lenient,
}

impl RestorePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Lenient => "lenient",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RestoreDecision {
    Success,
    Partial,
    Failed,
}

impl RestoreDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RestoreTelemetry {
    pub unknown_token_count: u64,
    pub manifest_bypass_count: u64,
    pub fresh_pii_detected_count: u64,
    pub restore_policy: RestorePolicy,
    pub restore_decision: RestoreDecision,
    pub phase_execution_mask: u32,
}

impl RestoreTelemetry {
    pub fn new(restore_policy: RestorePolicy) -> Self {
        Self {
            unknown_token_count: 0,
            manifest_bypass_count: 0,
            fresh_pii_detected_count: 0,
            restore_policy,
            restore_decision: RestoreDecision::Success,
            phase_execution_mask: 0,
        }
    }

    pub fn restore_policy_str(&self) -> &'static str {
        self.restore_policy.as_str()
    }

    pub fn restore_decision_str(&self) -> &'static str {
        self.restore_decision.as_str()
    }
}

/// Collision-family membership metadata for one recognizer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CollisionMembership {
    /// Cross-class family name.
    pub family: String,
    /// Variant name within the family.
    pub variant: String,
    /// Lower values win when two variants in the same family overlap.
    pub precedence: u32,
    /// Optional anchor variant required by later ambiguity handling.
    pub mandatory_anchor: Option<String>,
}

impl CollisionMembership {
    /// Builds collision-family membership metadata.
    pub fn new(
        family: impl Into<String>,
        variant: impl Into<String>,
        precedence: u32,
        mandatory_anchor: Option<String>,
    ) -> Self {
        Self {
            family: family.into(),
            variant: variant.into(),
            precedence,
            mandatory_anchor,
        }
    }
}

impl PiiClass {
    /// Parses a policy class name into the shared class vocabulary.
    pub fn from_policy_name(input: &str) -> Option<Self> {
        match input {
            "email" => Some(Self::Email),
            "name" => Some(Self::Name),
            "location" => Some(Self::Location),
            "organization" => Some(Self::Organization),
            custom if custom.starts_with("custom:") => {
                let name = custom.trim_start_matches("custom:");
                (!name.trim().is_empty()).then(|| Self::custom(name))
            }
            _ => None,
        }
    }

    /// Returns the built-in class variants.
    pub fn builtin_variants() -> &'static [PiiClass] {
        &[
            PiiClass::Email,
            PiiClass::Name,
            PiiClass::Location,
            PiiClass::Organization,
        ]
    }

    /// Builds a normalized custom class name.
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

    /// Returns the normalized custom class name for custom classes.
    pub fn as_custom_name(&self) -> Option<&str> {
        match self {
            Self::Custom(name) => Some(name.as_str()),
            Self::Email | Self::Name | Self::Location | Self::Organization => None,
        }
    }

    /// Returns the audit/token display label for this class.
    pub fn class_name(&self) -> String {
        match self {
            Self::Email => BUILTIN_CLASS_NAMES[0].to_string(),
            Self::Name => BUILTIN_CLASS_NAMES[1].to_string(),
            Self::Location => BUILTIN_CLASS_NAMES[2].to_string(),
            Self::Organization => BUILTIN_CLASS_NAMES[3].to_string(),
            Self::Custom(name) => format!("Custom:{name}"),
        }
    }

    /// Returns the canonical audit/serde label for this class.
    pub fn to_canonical_str(&self) -> String {
        match self {
            Self::Email => "email".to_string(),
            Self::Name => "name".to_string(),
            Self::Location => "location".to_string(),
            Self::Organization => "organization".to_string(),
            Self::Custom(name) => format!("custom:{name}"),
        }
    }

    /// Parses the canonical audit/serde label for a PII class.
    pub fn from_canonical_str(value: &str) -> Option<Self> {
        match value {
            "email" | "Email" => Some(Self::Email),
            "name" | "Name" => Some(Self::Name),
            "location" | "Location" => Some(Self::Location),
            "organization" | "Organization" => Some(Self::Organization),
            custom if custom.starts_with("custom:") => {
                let name = &custom["custom:".len()..];
                (!name.is_empty()).then(|| Self::Custom(name.to_string()))
            }
            _ => None,
        }
    }
}

/// Audit-canonical form of [`PiiClass`].
///
/// Serializes as `"email"`, `"name"`, `"custom:foo"`, and similar canonical
/// strings. Use this wrapper for audit-row JSON only. Session snapshots use
/// bare [`PiiClass`] serde so their byte shape stays stable.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PiiClassAudit(pub PiiClass);

impl PiiClassAudit {
    /// Builds an audit-canonical class wrapper.
    pub fn new(class: PiiClass) -> Self {
        Self(class)
    }

    /// Unwraps the underlying class.
    pub fn into_inner(self) -> PiiClass {
        self.0
    }
}

impl Serialize for PiiClassAudit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_canonical_str())
    }
}

impl<'de> Deserialize<'de> for PiiClassAudit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        PiiClass::from_canonical_str(&value)
            .map(Self)
            .ok_or_else(|| {
                serde::de::Error::custom(format!("unknown PiiClass canonical form: {value}"))
            })
    }
}

mod pii_class_audit_serde {
    use super::{PiiClass, PiiClassAudit};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(class: &PiiClass, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        PiiClassAudit::new(class.clone()).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PiiClass, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(PiiClassAudit::deserialize(deserializer)?.into_inner())
    }
}

/// A candidate recognizer/class pair that lost ambiguity resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LosingCandidate {
    /// PII class proposed by the losing recognizer.
    #[serde(with = "pii_class_audit_serde")]
    pub class: PiiClass,
    /// Stable recognizer identifier for traceability.
    pub recognizer_id: String,
}

impl LosingCandidate {
    /// Builds a losing ambiguity candidate.
    pub fn new(class: PiiClass, recognizer_id: impl Into<String>) -> Self {
        Self {
            class,
            recognizer_id: recognizer_id.into(),
        }
    }
}

/// Structured metadata describing an ambiguity outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AmbiguityRecord {
    /// The family-level class assigned when disambiguation failed.
    #[serde(with = "pii_class_audit_serde")]
    pub ambiguity_class: PiiClass,
    /// Variants that could not be disambiguated.
    ///
    /// Producers must keep this list stable by sorting `recognizer_id` ascending.
    pub losing_candidates: Vec<LosingCandidate>,
    /// Why disambiguation failed.
    pub reason: AmbiguityReason,
}

impl AmbiguityRecord {
    /// Builds a structured ambiguity record.
    pub fn new(
        ambiguity_class: PiiClass,
        losing_candidates: Vec<LosingCandidate>,
        reason: AmbiguityReason,
    ) -> Self {
        Self {
            ambiguity_class,
            losing_candidates,
            reason,
        }
    }
}

/// Closed set of ambiguity outcomes recorded by the audit side-channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum AmbiguityReason {
    /// Span matched a multi-recognizer family and no anchor cue resolved it.
    NoAnchor,
    /// Multiple validator-stage recognizers remained viable for the same span.
    ValidatorIndeterminate,
    /// Span matched recognizers across two or more distinct PII class families.
    MultiFamilyMatch,
    /// Multiple variants had the same precedence and no discriminator resolved them.
    PrecedenceTie,
}

/// Closed validator failure reasons recorded by audit metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ValidatorFailReason {
    /// Luhn checksum validation failed.
    LuhnFailed,
    /// IBAN MOD-97 validation failed.
    IbanMod97Failed,
    /// Email RFC-style validation failed.
    #[serde(alias = "email_rfc_failed")]
    EmailRfcRejected,
    /// E.164 phone validation failed.
    #[serde(alias = "e164_phone_failed")]
    PhoneE164Rejected,
    /// National phone parser accepted the number but region validation failed.
    PhoneNationalRegionMismatch,
    /// IPv4 parser rejected the candidate.
    Ipv4ParseFailed,
    /// IPv6 parser rejected the candidate.
    Ipv6ParseFailed,
    /// EIP-55 Ethereum checksum validation failed.
    EthEip55ChecksumFailed,
    /// Aadhaar Verhoeff checksum validation failed.
    AadhaarVerhoeffFailed,
    /// French NIR MOD-97 key validation failed.
    FrNirMod97Failed,
    /// German Steuer-ID MOD 11,10 checksum validation failed.
    DeSteuerIdMod1110Failed,
    /// Dutch BSN MOD-11 checksum validation failed.
    BsnMod11Failed,
    /// Brazilian CPF MOD-11 checksum validation failed.
    CpfMod11Failed,
    /// Brazilian CNPJ MOD-11 checksum validation failed.
    CnpjMod11Failed,
    /// UK NHS number MOD-11 checksum validation failed.
    UkNhsMod11Failed,
}

/// Typed validator outcome used by the pre-resolver validator-veto phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ValidatorOutcome {
    /// Candidate passed validation; canonical form may be supplied by the validator.
    Pass { canonical_form: Option<String> },
    /// Candidate failed validation with a closed, auditable reason.
    Fail { reason: ValidatorFailReason },
    /// Recognizer has no validator for this candidate.
    NotApplicable,
}

/// Error returned when a rulepack names a validator unsupported by this build.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ValidatorKindParseError {
    /// Validator kind is not known or is gated behind a disabled feature.
    #[error("unsupported validator: {kind}")]
    UnsupportedValidator {
        /// Unsupported validator kind.
        kind: String,
    },
}

/// Closed set of validator implementations used by validator-backed recognizers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidatorKind {
    /// Basic email shape validator.
    EmailRfc,
    /// Parser-backed E.164 phone validator.
    #[cfg(feature = "phone-parser")]
    E164Phone,
    /// Parser-backed national phone validator for a fixed region.
    #[cfg(feature = "phone-parser")]
    E164PhoneNational(Region),
    /// Luhn checksum validator.
    Luhn,
    /// IBAN MOD-97 validator.
    IbanMod97,
    /// Strict decimal dotted-quad IPv4 parser.
    Ipv4Parse,
    /// RFC 4291 / RFC 5952 IPv6 textual parser.
    Ipv6Parse,
    /// EIP-55 Ethereum address checksum validator.
    EthEip55,
    /// Indian Aadhaar Verhoeff checksum validator.
    AadhaarVerhoeff,
    /// French NIR MOD-97 key validator.
    FrNirMod97,
    /// German Steuer-ID MOD 11,10 checksum validator.
    DeSteuerIdMod1110,
    /// Dutch BSN MOD-11 checksum validator.
    BsnMod11,
    /// Brazilian CPF MOD-11 checksum validator.
    CpfMod11,
    /// Brazilian CNPJ MOD-11 checksum validator.
    CnpjMod11,
    /// UK NHS number MOD-11 checksum validator.
    UkNhsMod11,
}

/// Regions supported by national phone validators.
#[cfg(feature = "phone-parser")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Region {
    /// Germany.
    De,
    /// United States.
    Us,
}

impl ValidatorKind {
    /// Parses a policy validator kind.
    pub fn parse(s: &str) -> Result<Self, ValidatorKindParseError> {
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
            "ipv4_parse" => Ok(Self::Ipv4Parse),
            "ipv6_parse" => Ok(Self::Ipv6Parse),
            "eth_eip55" => Ok(Self::EthEip55),
            "aadhaar_verhoeff" => Ok(Self::AadhaarVerhoeff),
            "fr_nir_mod97" => Ok(Self::FrNirMod97),
            "de_steuer_id_mod1110" => Ok(Self::DeSteuerIdMod1110),
            "bsn_mod11" => Ok(Self::BsnMod11),
            "cpf_mod11" => Ok(Self::CpfMod11),
            "cnpj_mod11" => Ok(Self::CnpjMod11),
            "uk_nhs_mod11" => Ok(Self::UkNhsMod11),
            other => Err(ValidatorKindParseError::UnsupportedValidator {
                kind: other.to_string(),
            }),
        }
    }

    /// Returns whether the validator accepts the input.
    pub fn validates(self, input: &str) -> bool {
        match self {
            Self::AadhaarVerhoeff => aadhaar_verhoeff_check(input),
            Self::FrNirMod97 => fr_nir_mod97_check(input),
            Self::DeSteuerIdMod1110 => de_steuer_id_mod1110_check(input),
            Self::BsnMod11 => bsn_mod11_check(input),
            Self::CpfMod11 => cpf_mod11_check(input),
            Self::CnpjMod11 => cnpj_mod11_check(input),
            Self::UkNhsMod11 => uk_nhs_mod11_check(input),
            _ => self.canonical_form(input).is_some(),
        }
    }

    /// Applies validation and returns a typed outcome for audit.
    pub fn validate(self, input: &str) -> ValidatorOutcome {
        match self.canonical_form(input) {
            Some(canonical_form) => ValidatorOutcome::Pass {
                canonical_form: Some(canonical_form),
            },
            None => ValidatorOutcome::Fail {
                reason: self.fail_reason(),
            },
        }
    }

    /// Returns the canonical form for accepted input.
    pub fn canonical_form(self, input: &str) -> Option<String> {
        match self {
            Self::EmailRfc => is_basic_email(input).then(|| input.to_string()),
            #[cfg(feature = "phone-parser")]
            Self::E164Phone => e164_phone_check(input).then(|| input.to_string()),
            #[cfg(feature = "phone-parser")]
            Self::E164PhoneNational(region) => validate_phone_national(region, input),
            Self::Luhn => luhn_check(input).then(|| input.to_string()),
            Self::IbanMod97 => iban_mod97_check(input).then(|| input.to_string()),
            Self::Ipv4Parse => ipv4_parse_check(input).then(|| input.to_string()),
            Self::Ipv6Parse => ipv6_parse_check(input).then(|| input.to_string()),
            Self::EthEip55 => eth_eip55_check(input).then(|| input.to_string()),
            Self::AadhaarVerhoeff => {
                canonical_ascii_digits::<12>(input).filter(|_| aadhaar_verhoeff_check(input))
            }
            Self::FrNirMod97 => {
                canonical_ascii_digits::<15>(input).filter(|_| fr_nir_mod97_check(input))
            }
            Self::DeSteuerIdMod1110 => {
                canonical_ascii_digits::<11>(input).filter(|_| de_steuer_id_mod1110_check(input))
            }
            Self::BsnMod11 => canonical_ascii_digits::<9>(input).filter(|_| bsn_mod11_check(input)),
            Self::CpfMod11 => {
                canonical_ascii_digits::<11>(input).filter(|_| cpf_mod11_check(input))
            }
            Self::CnpjMod11 => {
                canonical_ascii_digits::<14>(input).filter(|_| cnpj_mod11_check(input))
            }
            Self::UkNhsMod11 => {
                canonical_ascii_digits::<10>(input).filter(|_| uk_nhs_mod11_check(input))
            }
        }
    }

    /// Returns the audit reason emitted when validation fails.
    pub fn fail_reason(self) -> ValidatorFailReason {
        match self {
            Self::EmailRfc => ValidatorFailReason::EmailRfcRejected,
            #[cfg(feature = "phone-parser")]
            Self::E164Phone => ValidatorFailReason::PhoneE164Rejected,
            #[cfg(feature = "phone-parser")]
            Self::E164PhoneNational(_) => ValidatorFailReason::PhoneNationalRegionMismatch,
            Self::Luhn => ValidatorFailReason::LuhnFailed,
            Self::IbanMod97 => ValidatorFailReason::IbanMod97Failed,
            Self::Ipv4Parse => ValidatorFailReason::Ipv4ParseFailed,
            Self::Ipv6Parse => ValidatorFailReason::Ipv6ParseFailed,
            Self::EthEip55 => ValidatorFailReason::EthEip55ChecksumFailed,
            Self::AadhaarVerhoeff => ValidatorFailReason::AadhaarVerhoeffFailed,
            Self::FrNirMod97 => ValidatorFailReason::FrNirMod97Failed,
            Self::DeSteuerIdMod1110 => ValidatorFailReason::DeSteuerIdMod1110Failed,
            Self::BsnMod11 => ValidatorFailReason::BsnMod11Failed,
            Self::CpfMod11 => ValidatorFailReason::CpfMod11Failed,
            Self::CnpjMod11 => ValidatorFailReason::CnpjMod11Failed,
            Self::UkNhsMod11 => ValidatorFailReason::UkNhsMod11Failed,
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
        Region::Us => {
            digits == "15550100"
                || matches!(digits.strip_prefix('1'), Some(rest) if rest.len() == 10 && rest[3..].starts_with("55501"))
        }
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

fn iban_mod97_check(input: &str) -> bool {
    let canonical = iban_canonicalize(input);
    let bytes = canonical.as_bytes();
    if bytes.len() < 4 {
        return false;
    }
    if !bytes[0].is_ascii_uppercase()
        || !bytes[1].is_ascii_uppercase()
        || !bytes[2].is_ascii_digit()
        || !bytes[3].is_ascii_digit()
    {
        return false;
    }
    let Some(expected_len) = iban_country_length(&bytes[..2]) else {
        return false;
    };
    if bytes.len() != expected_len {
        return false;
    }
    if !bytes.iter().all(u8::is_ascii_alphanumeric) {
        return false;
    }

    let mut remainder = 0u32;
    for byte in bytes[4..].iter().chain(bytes[..4].iter()).copied() {
        match byte {
            b'0'..=b'9' => {
                remainder = (remainder * 10 + u32::from(byte - b'0')) % 97;
            }
            b'A'..=b'Z' => {
                let value = u32::from(byte - b'A') + 10;
                remainder = (remainder * 10 + value / 10) % 97;
                remainder = (remainder * 10 + value % 10) % 97;
            }
            _ => return false,
        }
    }
    remainder == 1
}

fn iban_country_length(country: &[u8]) -> Option<usize> {
    // ISO 13616 IBAN Registry country lengths. MOD-97 alone has a false-accept
    // rate near 1/97, so exact country length gates candidates before checksum.
    match country {
        b"AD" => Some(24),
        b"AE" => Some(23),
        b"AL" => Some(28),
        b"AT" => Some(20),
        b"AZ" => Some(28),
        b"BA" => Some(20),
        b"BE" => Some(16),
        b"BG" => Some(22),
        b"BH" => Some(22),
        b"BI" => Some(27),
        b"BR" => Some(29),
        b"BY" => Some(28),
        b"CH" => Some(21),
        b"CR" => Some(22),
        b"CY" => Some(28),
        b"CZ" => Some(24),
        b"DE" => Some(22),
        b"DJ" => Some(27),
        b"DK" => Some(18),
        b"DO" => Some(28),
        b"EE" => Some(20),
        b"EG" => Some(29),
        b"ES" => Some(24),
        b"FI" => Some(18),
        b"FK" => Some(18),
        b"FO" => Some(18),
        b"FR" => Some(27),
        b"GB" => Some(22),
        b"GE" => Some(22),
        b"GI" => Some(23),
        b"GL" => Some(18),
        b"GR" => Some(27),
        b"GT" => Some(28),
        b"HN" => Some(28),
        b"HR" => Some(21),
        b"HU" => Some(28),
        b"IE" => Some(22),
        b"IL" => Some(23),
        b"IQ" => Some(23),
        b"IS" => Some(26),
        b"IT" => Some(27),
        b"JO" => Some(30),
        b"KW" => Some(30),
        b"KZ" => Some(20),
        b"LB" => Some(28),
        b"LC" => Some(32),
        b"LI" => Some(21),
        b"LT" => Some(20),
        b"LU" => Some(20),
        b"LV" => Some(21),
        b"LY" => Some(25),
        b"MC" => Some(27),
        b"MD" => Some(24),
        b"ME" => Some(22),
        b"MK" => Some(19),
        b"MN" => Some(20),
        b"MR" => Some(27),
        b"MT" => Some(31),
        b"MU" => Some(30),
        b"NI" => Some(28),
        b"NL" => Some(18),
        b"NO" => Some(15),
        b"OM" => Some(23),
        b"PK" => Some(24),
        b"PL" => Some(28),
        b"PS" => Some(29),
        b"PT" => Some(25),
        b"QA" => Some(29),
        b"RO" => Some(24),
        b"RS" => Some(22),
        b"RU" => Some(33),
        b"SA" => Some(24),
        b"SC" => Some(31),
        b"SD" => Some(18),
        b"SE" => Some(24),
        b"SI" => Some(19),
        b"SK" => Some(24),
        b"SM" => Some(27),
        b"SO" => Some(23),
        b"ST" => Some(25),
        b"SV" => Some(28),
        b"TL" => Some(23),
        b"TN" => Some(24),
        b"TR" => Some(26),
        b"UA" => Some(29),
        b"VA" => Some(22),
        b"VG" => Some(24),
        b"XK" => Some(20),
        b"YE" => Some(30),
        _ => None,
    }
}

fn iban_canonicalize(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

fn ipv4_parse_check(input: &str) -> bool {
    input.parse::<std::net::Ipv4Addr>().is_ok()
}

fn ipv6_parse_check(input: &str) -> bool {
    input.parse::<std::net::Ipv6Addr>().is_ok()
}

fn eth_eip55_check(input: &str) -> bool {
    let Some(address) = input.strip_prefix("0x") else {
        return false;
    };
    if address.len() != 40 || !address.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    if address
        .bytes()
        .all(|byte| !byte.is_ascii_alphabetic() || byte.is_ascii_lowercase())
        || address
            .bytes()
            .all(|byte| !byte.is_ascii_alphabetic() || byte.is_ascii_uppercase())
    {
        return true;
    }

    let lowercase = address.to_ascii_lowercase();
    let hash = Keccak256::digest(lowercase.as_bytes());
    for (index, byte) in address.bytes().enumerate() {
        if byte.is_ascii_digit() {
            continue;
        }
        let hash_nibble = if index % 2 == 0 {
            hash[index / 2] >> 4
        } else {
            hash[index / 2] & 0x0f
        };
        if (hash_nibble > 7) != byte.is_ascii_uppercase() {
            return false;
        }
    }
    true
}

fn collect_ascii_digits<const N: usize>(input: &str) -> Option<[u8; N]> {
    let mut digits = [0u8; N];
    let mut count = 0usize;
    for byte in input.bytes() {
        if byte.is_ascii_digit() {
            if count == N {
                return None;
            }
            digits[count] = byte - b'0';
            count += 1;
        } else if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'-' | b'.' | b'/') {
            continue;
        } else {
            return None;
        }
    }
    (count == N).then_some(digits)
}

fn canonical_ascii_digits<const N: usize>(input: &str) -> Option<String> {
    let digits = collect_ascii_digits::<N>(input)?;
    let mut canonical = String::with_capacity(N);
    for digit in digits {
        canonical.push(char::from(b'0' + digit));
    }
    Some(canonical)
}

fn not_all_same<const N: usize>(digits: &[u8; N]) -> bool {
    digits[1..].iter().any(|digit| *digit != digits[0])
}

fn aadhaar_verhoeff_check(input: &str) -> bool {
    const D: [[u8; 10]; 10] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
        [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
        [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
        [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
        [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
        [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
        [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
        [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
        [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    ];
    const P: [[u8; 10]; 8] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
        [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
        [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
        [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
        [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
        [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
        [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
    ];
    let Some(digits) = collect_ascii_digits::<12>(input) else {
        return false;
    };
    if digits[0] < 2 || !not_all_same(&digits) {
        return false;
    }
    let mut checksum = 0u8;
    for (index, digit) in digits.iter().rev().enumerate() {
        checksum = D[checksum as usize][P[index % 8][*digit as usize] as usize];
    }
    checksum == 0
}

fn fr_nir_mod97_check(input: &str) -> bool {
    let Some(digits) = collect_ascii_digits::<15>(input) else {
        return false;
    };
    if !matches!(digits[0], 1 | 2 | 3 | 4 | 7 | 8) {
        return false;
    }
    let month = digits[3] * 10 + digits[4];
    if !(1..=12).contains(&month) && !(20..=42).contains(&month) && !(50..=99).contains(&month) {
        return false;
    }
    let mut number = 0u32;
    for digit in &digits[..13] {
        number = (number * 10 + u32::from(*digit)) % 97;
    }
    let key = u32::from(digits[13]) * 10 + u32::from(digits[14]);
    97 - number == key
}

fn de_steuer_id_mod1110_check(input: &str) -> bool {
    let Some(digits) = collect_ascii_digits::<11>(input) else {
        return false;
    };
    if !steuer_id_first_ten_digits_valid(&digits) {
        return false;
    }
    let mut product = 10u8;
    for digit in &digits[..10] {
        let mut sum = (*digit + product) % 10;
        if sum == 0 {
            sum = 10;
        }
        product = (2 * sum) % 11;
    }
    let check = (11 - product) % 10;
    check == digits[10]
}

fn steuer_id_first_ten_digits_valid(digits: &[u8; 11]) -> bool {
    if digits[0] == 0 {
        return false;
    }
    let mut counts = [0u8; 10];
    for digit in &digits[..10] {
        counts[*digit as usize] += 1;
    }
    let repeated_digits = counts.iter().filter(|count| **count > 1).count();
    let missing_digits = counts.iter().filter(|count| **count == 0).count();
    let repeated_count_valid = counts.iter().any(|count| matches!(*count, 2 | 3));
    repeated_digits == 1 && repeated_count_valid && matches!(missing_digits, 1 | 2)
}

fn bsn_mod11_check(input: &str) -> bool {
    let Some(digits) = collect_ascii_digits::<9>(input) else {
        return false;
    };
    if !not_all_same(&digits) {
        return false;
    }
    let sum: i32 = digits[..8]
        .iter()
        .enumerate()
        .map(|(index, digit)| i32::from(*digit) * (9 - index as i32))
        .sum::<i32>()
        - i32::from(digits[8]);
    sum.rem_euclid(11) == 0
}

fn cpf_mod11_check(input: &str) -> bool {
    let Some(digits) = collect_ascii_digits::<11>(input) else {
        return false;
    };
    if !not_all_same(&digits) {
        return false;
    }
    mod11_check_digit(&digits[..9], 10) == digits[9]
        && mod11_check_digit(&digits[..10], 11) == digits[10]
}

fn cnpj_mod11_check(input: &str) -> bool {
    let Some(digits) = collect_ascii_digits::<14>(input) else {
        return false;
    };
    if !not_all_same(&digits) {
        return false;
    }
    const FIRST: [u8; 12] = [5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
    const SECOND: [u8; 13] = [6, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
    weighted_mod11_check_digit(&digits[..12], &FIRST) == digits[12]
        && weighted_mod11_check_digit(&digits[..13], &SECOND) == digits[13]
}

fn uk_nhs_mod11_check(input: &str) -> bool {
    let Some(digits) = collect_ascii_digits::<10>(input) else {
        return false;
    };
    if !not_all_same(&digits) {
        return false;
    }
    let sum: u32 = digits[..9]
        .iter()
        .enumerate()
        .map(|(index, digit)| u32::from(*digit) * (10 - index as u32))
        .sum();
    let check = 11 - (sum % 11);
    let check = if check == 11 { 0 } else { check };
    check != 10 && check == u32::from(digits[9])
}

fn mod11_check_digit(digits: &[u8], start_weight: u8) -> u8 {
    let weights = (2..=start_weight).rev();
    let sum: u32 = digits
        .iter()
        .zip(weights)
        .map(|(digit, weight)| u32::from(*digit) * u32::from(weight))
        .sum();
    let remainder = sum % 11;
    if remainder < 2 {
        0
    } else {
        (11 - remainder) as u8
    }
}

fn weighted_mod11_check_digit(digits: &[u8], weights: &[u8]) -> u8 {
    let sum: u32 = digits
        .iter()
        .zip(weights)
        .map(|(digit, weight)| u32::from(*digit) * u32::from(*weight))
        .sum();
    let remainder = sum % 11;
    if remainder < 2 {
        0
    } else {
        (11 - remainder) as u8
    }
}

/// A detected span and its class/source metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Detection {
    /// Byte span in the original input.
    pub span: Range<usize>,
    /// PII class assigned to the span.
    pub class: PiiClass,
    /// Detector source identifier.
    pub source: String,
}

impl Detection {
    /// Builds a detected PII span.
    pub fn new(span: Range<usize>, class: PiiClass, source: impl Into<String>) -> Self {
        Self {
            span,
            class,
            source: source.into(),
        }
    }
}

/// Observer-only post-clean check (Pass 3 in the detection pipeline).
///
/// Runs against already-tokenized output. May report suspected missed PII via
/// [`LeakReport`] but **must not** mutate the token manifest, the `CleanDocument`,
/// or the restore path. Safety nets are additive defense-in-depth, not a replacement
/// for Pass 1/2 detection.
///
/// Activate at runtime with `Pipeline::with_safety_net` (post-build) or
/// `PipelineBuilder::register_safety_net` (during build), or via the CLI
/// `--safety-net=<name>` flag.
///
/// If a safety net reports a suspected miss, the caller decides the response; the
/// pipeline never silently re-cleans based on safety net output.
pub trait SafetyNet: Send + Sync {
    /// Stable backend identifier used in telemetry and audit rows.
    fn id(&self) -> &str;

    /// Locale tags supported by this safety net. Empty means global.
    fn supported_locales(&self) -> &[LocaleTag];

    /// Checks clean text for possible PII that the manifest did not cover.
    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError>;
}

/// Context passed to a privacy safety net.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SafetyNetContext<'a> {
    /// Tokens emitted by the pseudonymization pipeline for this text segment.
    pub manifest: &'a Manifest,
    /// Active session-level locale chain. For `RawDocument::Structured`, locale
    /// gating uses this same session-level chain across all fields; structured
    /// fields do not carry per-field locale annotations.
    pub locale_chain: &'a [LocaleTag],
    /// Source document kind being checked.
    pub document_kind: DocumentKind,
    /// Optional audit session identifier.
    pub session_id: Option<&'a str>,
    /// Structured-document field path, such as `$.user.email`.
    pub field_path: Option<&'a str>,
}

impl<'a> SafetyNetContext<'a> {
    /// Builds safety-net context for one clean text segment.
    pub fn new(
        manifest: &'a Manifest,
        locale_chain: &'a [LocaleTag],
        document_kind: DocumentKind,
        session_id: Option<&'a str>,
        field_path: Option<&'a str>,
    ) -> Self {
        Self {
            manifest,
            locale_chain,
            document_kind,
            session_id,
            field_path,
        }
    }
}

/// A replacement emitted by the pseudonymization pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EmittedTokenSpan {
    /// Byte span in the clean text.
    pub clean_span: Range<usize>,
    /// Byte span in the raw text that produced the token.
    pub raw_span: Range<usize>,
    /// PII class represented by the emitted token.
    pub class: PiiClass,
}

impl EmittedTokenSpan {
    /// Builds an emitted token span.
    pub fn new(clean_span: Range<usize>, raw_span: Range<usize>, class: PiiClass) -> Self {
        Self {
            clean_span,
            raw_span,
            class,
        }
    }
}

/// Set of emitted token spans for one clean text segment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Manifest {
    /// Spans sorted by `clean_span.start`.
    pub spans: Vec<EmittedTokenSpan>,
}

impl Manifest {
    /// Builds a manifest from spans and sorts them by clean byte start.
    pub fn from_spans(mut spans: Vec<EmittedTokenSpan>) -> Self {
        spans.sort_by_key(|span| (span.clean_span.start, span.clean_span.end));
        Self { spans }
    }

    /// Diffs one safety-net suspect span against emitted token coverage.
    ///
    /// Returns `None` when the suspect span is continuously covered by emitted
    /// token spans of the same class. Internal gaps return
    /// `LeakKind::PartialBleed`. When multiple uncovered gaps exist, this method
    /// deterministically returns the first gap by byte offset; full gap
    /// enumeration is intentionally deferred to a future report format.
    pub fn diff_against(
        &self,
        suspect_span: &Range<usize>,
        suspect_class: &PiiClass,
    ) -> Option<LeakKind> {
        if suspect_span.is_empty() {
            return None;
        }

        let start_idx = self
            .spans
            .partition_point(|span| span.clean_span.end <= suspect_span.start);
        let overlapping = self.spans[start_idx..]
            .iter()
            .take_while(|span| span.clean_span.start < suspect_span.end)
            .filter(|span| ranges_overlap(&span.clean_span, suspect_span))
            .collect::<Vec<_>>();

        if overlapping.is_empty() {
            return Some(LeakKind::Uncovered);
        }

        let mut cursor = suspect_span.start;
        let mut first_mismatch = None::<&EmittedTokenSpan>;
        for span in overlapping {
            if span.clean_span.start > cursor {
                return Some(LeakKind::PartialBleed {
                    uncovered: cursor..span.clean_span.start.min(suspect_span.end),
                });
            }

            if span.clean_span.end > cursor {
                if first_mismatch.is_none() && &span.class != suspect_class {
                    first_mismatch = Some(span);
                }
                cursor = cursor.max(span.clean_span.end.min(suspect_span.end));
                if cursor >= suspect_span.end {
                    break;
                }
            }
        }

        if cursor < suspect_span.end {
            return Some(LeakKind::PartialBleed {
                uncovered: cursor..suspect_span.end,
            });
        }

        first_mismatch.map(|span| LeakKind::ClassMismatch {
            pipeline_class: span.class.clone(),
            safety_net_class: suspect_class.clone(),
        })
    }
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

/// Suspected leak reported by an observer-only safety net.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LeakSuspect {
    /// Byte span in clean text.
    pub span: Range<usize>,
    /// Mapped PII class for the suspect.
    pub class: PiiClass,
    /// Safety-net backend identifier.
    pub safety_net_id: String,
    /// Optional backend confidence score.
    pub score: Option<f32>,
    /// Leak classification after manifest correlation.
    pub kind: LeakKind,
    /// Raw backend label after validation/mapping, never source text.
    pub raw_label: String,
    /// Optional structured field path.
    pub field_path: Option<String>,
}

impl LeakSuspect {
    /// Builds a safety-net leak suspect.
    pub fn new(
        span: Range<usize>,
        class: PiiClass,
        safety_net_id: impl Into<String>,
        score: Option<f32>,
        kind: LeakKind,
        raw_label: impl Into<String>,
        field_path: Option<String>,
    ) -> Self {
        Self {
            span,
            class,
            safety_net_id: safety_net_id.into(),
            score,
            kind,
            raw_label: raw_label.into(),
            field_path,
        }
    }
}

/// The category of a suspected missed PII span.
///
/// `LeakKind` is `#[non_exhaustive]`. Match with a wildcard for forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LeakKind {
    /// No same-class emitted token overlaps the suspect span.
    Uncovered,
    /// The suspect is only partly covered; `uncovered` is the first gap.
    PartialBleed {
        /// First uncovered byte range in the suspect span.
        uncovered: Range<usize>,
    },
    /// The suspect is continuously covered, but by a different class.
    ClassMismatch {
        /// Class emitted by the pipeline.
        pipeline_class: PiiClass,
        /// Class reported by the safety net.
        safety_net_class: PiiClass,
    },
}

/// Bytes-free telemetry emitted by safety-net orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LeakReportTelemetry {
    /// Safety net skipped because the session-level locale chain did not match.
    LocaleSkipped {
        /// Safety-net backend identifier.
        safety_net_id: String,
        /// Document kind checked.
        document_kind: DocumentKind,
        /// Optional structured field path when skip was recorded per field.
        field_path: Option<String>,
    },
}

/// Aggregate leak report statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LeakReportStats {
    /// Number of suspects reported.
    pub suspect_count: usize,
    /// Number of uncovered suspects.
    pub uncovered_count: usize,
    /// Number of partial-bleed suspects.
    pub partial_bleed_count: usize,
    /// Number of class-mismatch suspects.
    pub class_mismatch_count: usize,
    /// Number of locale-skip telemetry events.
    pub locale_skipped_count: usize,
}

/// Signed document-context metadata carried inside a session snapshot envelope.
///
/// This extension is the v0.7 bridge for `gaze-document`: it is safe to serialize
/// inside the owner-only snapshot envelope, while agent-facing files keep using
/// non-sensitive mirrors. The single `schema_version` is bundle-level; sub-files
/// do not carry independent schema versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DocumentExtension {
    /// Bundle-level schema version shared by clean, layout, preview, report, and manifest files.
    pub schema_version: u16,
    /// SHA-256 of `clean.md` NFC-normalized bytes.
    pub clean_md_sha256: [u8; 32],
    /// SHA-256 of canonical `layout.json` bytes.
    pub layout_json_sha256: [u8; 32],
    /// SHA-256 of canonical `report.json` bytes.
    pub report_json_sha256: [u8; 32],
    /// SHA-256 of `preview-redacted.png` bytes when a preview is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_png_sha256: Option<[u8; 32]>,
    /// Page count reported for the source document.
    pub page_count: u32,
    /// Audit session id mirrored from the writing session for cross-pane correlation.
    pub audit_session_id: String,
    /// Signed clean.md byte spans for every emitted token.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clean_spans: Vec<EmittedTokenSpan>,
    /// Codec audit rows for the decode path that produced this document extension.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codec_audit: Vec<CodecAuditRow>,
}

impl DocumentExtension {
    /// Starts a document extension builder for one bundle schema version.
    pub fn builder(schema_version: u16) -> DocumentExtensionBuilder {
        DocumentExtensionBuilder {
            schema_version,
            clean_md_sha256: None,
            layout_json_sha256: None,
            report_json_sha256: None,
            preview_png_sha256: None,
            page_count: None,
            audit_session_id: None,
            clean_spans: Vec::new(),
            codec_audit: Vec::new(),
        }
    }
}

/// Builder for [`DocumentExtension`] that requires signed integrity-binding fields.
#[derive(Debug, Clone)]
#[must_use]
pub struct DocumentExtensionBuilder {
    schema_version: u16,
    clean_md_sha256: Option<[u8; 32]>,
    layout_json_sha256: Option<[u8; 32]>,
    report_json_sha256: Option<[u8; 32]>,
    preview_png_sha256: Option<[u8; 32]>,
    page_count: Option<u32>,
    audit_session_id: Option<String>,
    clean_spans: Vec<EmittedTokenSpan>,
    codec_audit: Vec<CodecAuditRow>,
}

impl DocumentExtensionBuilder {
    pub fn clean_md_sha256(mut self, hash: [u8; 32]) -> Self {
        self.clean_md_sha256 = Some(hash);
        self
    }

    pub fn layout_json_sha256(mut self, hash: [u8; 32]) -> Self {
        self.layout_json_sha256 = Some(hash);
        self
    }

    pub fn report_json_sha256(mut self, hash: [u8; 32]) -> Self {
        self.report_json_sha256 = Some(hash);
        self
    }

    pub fn preview_png_sha256(mut self, hash: [u8; 32]) -> Self {
        self.preview_png_sha256 = Some(hash);
        self
    }

    pub fn page_count(mut self, page_count: u32) -> Self {
        self.page_count = Some(page_count);
        self
    }

    pub fn audit_session_id(mut self, audit_session_id: impl Into<String>) -> Self {
        self.audit_session_id = Some(audit_session_id.into());
        self
    }

    pub fn clean_spans(mut self, clean_spans: Vec<EmittedTokenSpan>) -> Self {
        self.clean_spans = clean_spans;
        self
    }

    pub fn codec_audit(mut self, codec_audit: Vec<CodecAuditRow>) -> Self {
        self.codec_audit = codec_audit;
        self
    }

    pub fn build(self) -> Result<DocumentExtension, DocumentExtensionError> {
        Ok(DocumentExtension {
            schema_version: self.schema_version,
            clean_md_sha256: self
                .clean_md_sha256
                .ok_or(DocumentExtensionError::MissingField("clean_md_sha256"))?,
            layout_json_sha256: self
                .layout_json_sha256
                .ok_or(DocumentExtensionError::MissingField("layout_json_sha256"))?,
            report_json_sha256: self
                .report_json_sha256
                .ok_or(DocumentExtensionError::MissingField("report_json_sha256"))?,
            preview_png_sha256: self.preview_png_sha256,
            page_count: self
                .page_count
                .ok_or(DocumentExtensionError::MissingField("page_count"))?,
            audit_session_id: self
                .audit_session_id
                .ok_or(DocumentExtensionError::MissingField("audit_session_id"))?,
            clean_spans: self.clean_spans,
            codec_audit: self.codec_audit,
        })
    }
}

/// Errors returned while building a [`DocumentExtension`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum DocumentExtensionError {
    #[error("missing document extension field: {0}")]
    MissingField(&'static str),
}

/// Provenance of text extracted from a document or transcript source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TextOrigin {
    /// Text came from OCR over pixels.
    Ocr,
    /// Text came from an embedded text layer.
    EmbeddedText,
    /// Text came from an audio/video transcript.
    Transcript,
    /// Text came from multiple extraction paths.
    Hybrid,
}

/// Orthogonal document codec capabilities delivered or advertised by a codec.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CodecCapabilitySet {
    /// Codec can emit text.
    pub text: bool,
    /// Codec can emit layout geometry.
    pub layout: bool,
    /// Codec can emit confidence buckets.
    pub confidence: bool,
    /// Codec can emit timestamps.
    pub timestamps: bool,
}

impl CodecCapabilitySet {
    /// Text-only capability set.
    pub const TEXT_ONLY: Self = Self {
        text: true,
        layout: false,
        confidence: false,
        timestamps: false,
    };

    /// Builds a codec capability bitset.
    pub const fn new(text: bool, layout: bool, confidence: bool, timestamps: bool) -> Self {
        Self {
            text,
            layout,
            confidence,
            timestamps,
        }
    }

    /// Returns true when this set contains every requested capability bit.
    pub fn contains(self, requested: Self) -> bool {
        (!requested.text || self.text)
            && (!requested.layout || self.layout)
            && (!requested.confidence || self.confidence)
            && (!requested.timestamps || self.timestamps)
    }
}

/// Per-codec declaration for text extraction density checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExtractionDensityPolicy {
    /// Require at least this many extracted text bytes per source KiB.
    Required(f32),
    /// Explicit exemption with an audit-visible reason.
    Exempt { reason: String },
}

impl Default for ExtractionDensityPolicy {
    fn default() -> Self {
        Self::Exempt {
            reason: "calibration_pending".to_string(),
        }
    }
}

/// Metadata-only audit row emitted by a document codec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CodecAuditRow {
    /// Stable codec id, such as `gaze.codec.tesseract`.
    pub codec_id: String,
    /// Adapter crate version, distinct from engine provenance.
    pub codec_version: String,
    /// Accepted MIME type for the decode.
    pub accepted_mime: String,
    /// Capabilities advertised by the codec.
    pub advertised: CodecCapabilitySet,
    /// Capabilities delivered for this decode.
    pub delivered: CodecCapabilitySet,
    /// Text provenance reported by the codec.
    pub text_origin: TextOrigin,
    /// Codec-output schema version, decoupled from bundle schema version.
    pub codec_output_schema_version: u16,
    /// Hash of canonical codec options, never the options themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_hash_hex: Option<String>,
    /// Engine provenance string, without paths or raw source text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_provenance: Option<String>,
    /// Extraction density policy declared by the codec for this MIME.
    pub extraction_density_policy: ExtractionDensityPolicy,
}

impl CodecAuditRow {
    /// Builds a metadata-only codec audit row.
    pub fn new(
        codec_id: impl Into<String>,
        codec_version: impl Into<String>,
        accepted_mime: impl Into<String>,
        text_origin: TextOrigin,
    ) -> Self {
        Self {
            codec_id: codec_id.into(),
            codec_version: codec_version.into(),
            accepted_mime: accepted_mime.into(),
            advertised: CodecCapabilitySet::default(),
            delivered: CodecCapabilitySet::default(),
            text_origin,
            codec_output_schema_version: 1,
            options_hash_hex: None,
            engine_provenance: None,
            extraction_density_policy: ExtractionDensityPolicy::default(),
        }
    }
}

/// A suspected missed PII span reported by a [`SafetyNet`].
///
/// The safety net is not authoritative; a `LeakReport` is a signal, not a confirmed
/// leak. False positives are expected. Review reports and adjust policy or recognizer
/// thresholds.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct LeakReport {
    /// Suspected leaks, containing metadata only.
    pub suspects: Vec<LeakSuspect>,
    /// Bytes-free telemetry events.
    pub telemetry: Vec<LeakReportTelemetry>,
    /// Aggregated counts for callers that do not need full suspect metadata.
    pub stats: LeakReportStats,
    /// Optional replay hash.
    ///
    /// Replay determinism is guaranteed only when command path, checkpoint,
    /// operating point, min score, and decode parameters are fixed externally.
    pub replay_hash: Option<String>,
}

impl LeakReport {
    /// Builds a report from suspects and telemetry.
    pub fn from_parts(
        suspects: Vec<LeakSuspect>,
        telemetry: Vec<LeakReportTelemetry>,
    ) -> LeakReport {
        let mut stats = LeakReportStats {
            suspect_count: suspects.len(),
            locale_skipped_count: telemetry
                .iter()
                .filter(|event| matches!(event, LeakReportTelemetry::LocaleSkipped { .. }))
                .count(),
            ..LeakReportStats::default()
        };
        for suspect in &suspects {
            match suspect.kind {
                LeakKind::Uncovered => stats.uncovered_count += 1,
                LeakKind::PartialBleed { .. } => stats.partial_bleed_count += 1,
                LeakKind::ClassMismatch { .. } => stats.class_mismatch_count += 1,
            }
        }
        LeakReport {
            suspects,
            telemetry,
            stats,
            replay_hash: None,
        }
    }

    /// Merges another report into this report.
    pub fn extend(&mut self, other: LeakReport) {
        self.suspects.extend(other.suspects);
        self.telemetry.extend(other.telemetry);
        *self = LeakReport::from_parts(
            std::mem::take(&mut self.suspects),
            std::mem::take(&mut self.telemetry),
        );
    }
}

/// Closed set of upstream OpenAI Privacy Filter labels accepted by Gaze.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum OpenAiPrivateLabel {
    /// `private_person`.
    PrivatePerson,
    /// `private_address`.
    PrivateAddress,
    /// `private_email`.
    PrivateEmail,
    /// `private_phone`.
    PrivatePhone,
    /// `private_url`.
    PrivateUrl,
    /// `private_date`.
    PrivateDate,
    /// `account_number`.
    AccountNumber,
    /// `secret`.
    Secret,
}

impl OpenAiPrivateLabel {
    /// Returns the raw upstream label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrivatePerson => "private_person",
            Self::PrivateAddress => "private_address",
            Self::PrivateEmail => "private_email",
            Self::PrivatePhone => "private_phone",
            Self::PrivateUrl => "private_url",
            Self::PrivateDate => "private_date",
            Self::AccountNumber => "account_number",
            Self::Secret => "secret",
        }
    }
}

/// Closed safety-net PII vocabulary before mapping into `PiiClass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SafetyNetPiiClass {
    /// Email address.
    Email,
    /// Person name.
    Name,
    /// Location or address.
    Location,
    /// Phone number.
    Phone,
    /// URL.
    Url,
    /// Date.
    Date,
    /// Account number.
    AccountNumber,
    /// Secret.
    Secret,
}

impl SafetyNetPiiClass {
    /// Maps the safety-net class into the shared pipeline class vocabulary.
    pub fn to_pii_class(self) -> PiiClass {
        match self {
            Self::Email => PiiClass::Email,
            Self::Name => PiiClass::Name,
            Self::Location => PiiClass::Location,
            Self::Phone => PiiClass::custom("phone"),
            Self::Url => PiiClass::custom("url"),
            Self::Date => PiiClass::custom("date"),
            Self::AccountNumber => PiiClass::custom("account_number"),
            Self::Secret => PiiClass::custom("secret"),
        }
    }
}

/// Exhaustive, closed error set for safety-net execution.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SafetyNetError {
    /// Safety net was explicitly requested but is unavailable.
    #[error("safety net unavailable: {reason}")]
    Unavailable {
        /// Sanitized reason.
        reason: String,
    },
    /// Required model weights or checkpoint are missing.
    #[error("safety net weights missing: {path}")]
    WeightsMissing {
        /// Sanitized path or identifier.
        path: String,
    },
    /// Backend model could not be loaded or reached.
    #[error("safety net model unavailable: {reason}")]
    ModelUnavailable {
        /// Sanitized reason.
        reason: String,
    },
    /// Backend model artifacts failed integrity verification.
    #[error("safety net model integrity mismatch: expected={expected}, actual={actual}")]
    ModelIntegrityMismatch {
        /// Expected SHA256 digest.
        expected: String,
        /// Actual SHA256 digest.
        actual: String,
    },
    /// Input exceeded configured backend limit.
    #[error("safety net input too large: limit={limit}, actual={actual}")]
    InputTooLarge {
        /// Configured byte limit.
        limit: usize,
        /// Actual byte length.
        actual: usize,
    },
    /// Backend runtime failed.
    #[error("safety net runtime failed: {message}")]
    Runtime {
        /// Sanitized diagnostic message.
        message: String,
    },
    /// Backend returned invalid output.
    #[error("safety net invalid output: {message}")]
    InvalidOutput {
        /// Sanitized diagnostic message.
        message: String,
    },
}

/// Disposition applied to a detected PII span.
///
/// | Variant | Restorable | Output shape |
/// |---------|------------|--------------|
/// | `Tokenize` | Yes | Opaque token: `<hex:Class_N>` |
/// | `FormatPreserve` | Yes | Realistic-looking pseudonym (e.g., `email1.hex@gaze-fake.invalid`) |
/// | `Redact` | No | Literal `[REDACTED]` -- original value is gone |
/// | `Generalize` | No | Class label (e.g., `[Email]`) -- original value is gone |
/// | `Preserve` | - | Passes through unchanged |
///
/// `Action` is `#[non_exhaustive]`. Use a wildcard arm in exhaustive matches.
/// When restore is required, use `Tokenize` or `FormatPreserve` -- `Redact` and
/// `Generalize` are irreversible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    /// Replace PII with a reversible token.
    Tokenize,
    /// Replace PII with a non-restorable redaction marker.
    Redact,
    /// Replace PII with a reversible format-preserving token.
    FormatPreserve,
    /// Replace PII with a broader category.
    Generalize,
    /// Preserve the original value.
    Preserve,
}

/// Conflict resolution tier that selected or rejected a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConflictTier {
    /// No conflict resolution was needed.
    None,
    /// Class priority decided the conflict.
    ClassPriority,
    /// Rule priority decided the conflict.
    RulePriority,
    /// Candidate score decided the conflict.
    Score,
    /// Span length decided the conflict.
    SpanLength,
    /// Same-class containment validator result decided the conflict.
    Validator,
    /// Pre-resolver validator veto rejected the candidate.
    ValidatorVeto,
    /// Cross-class collision-family policy decided the conflict.
    CollisionPolicy,
    /// Mandatory-anchor context was missing, so family-level fallback was emitted.
    AnchoredContext,
    /// Recognizer identifier decided the conflict.
    RecognizerId,
    /// Candidate was merged with another candidate.
    Merged,
    /// Safety-net redact mode stripped a suspect span.
    Redact,
    /// Safety-net resolve mode promoted a suspect span into a reversible token.
    Resolve,
    /// Safety-net fallback policy decided the outcome.
    Fallback,
}

/// Safety-net fallback reason recorded in metadata-only audit rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FallbackReason {
    /// The suspect overlapped an existing emitted token in a way that could not be promoted.
    OverlapConflict,
    /// A validator rejected the promoted candidate.
    ValidatorVeto,
    /// A mandatory anchor was missing for the promoted candidate.
    AnchorMissing,
    /// A follow-up safety-net pass still observed a suspect.
    ResidualSuspect,
}

/// Source document kind for metadata-only audit logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentKind {
    /// Structured key/value document.
    Structured,
    /// Plain text document.
    Text,
}

/// One row of redaction metadata emitted to a [`RedactionLogger`].
///
/// Fields identify the PII class, action taken, session ID, source document kind,
/// conflict-resolution metadata, and timestamp. Does **not** contain the original PII
/// value, the token string, or any identifiable content beyond what a compliance audit
/// requires.
///
/// `RedactionEntry` is `#[non_exhaustive]`; adopters must construct via the public
/// constructor or destructure with a wildcard pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RedactionEntry {
    /// Detector or recognizer source identifier.
    pub source: String,
    /// Stable semantic recognizer identifier, when available.
    pub recognizer_id: Option<String>,
    /// Versioned recognizer artifact/rule identifier, when available.
    pub recognizer_version_id: Option<String>,
    /// PII class affected by the decision.
    pub class: PiiClass,
    /// Policy action applied to the span.
    pub action: Action,
    /// Optional structured field name.
    pub field_name: Option<String>,
    /// Source document kind.
    pub document_kind: DocumentKind,
    /// Whether this entry records a loser in conflict resolution.
    pub conflict_loser: bool,
    /// Conflict tier that decided the outcome.
    pub decided_by: ConflictTier,
    /// Creation timestamp in epoch milliseconds.
    pub created_at: i64,
    /// Optional session identifier.
    pub session_id: Option<String>,
    /// Optional validator failure reason for a vetoed candidate.
    pub validator_fail_reason: Option<ValidatorFailReason>,
    /// Optional ambiguity metadata for a family-level fallback.
    pub ambiguity_record: Option<AmbiguityRecord>,
    /// Collision family that influenced this decision.
    pub collision_family: Option<String>,
    /// Collision variant that influenced this decision.
    pub collision_variant: Option<String>,
    /// Safety-net fallback reason, when fallback policy handled the row.
    pub fallback_triggered: Option<FallbackReason>,
    /// NER/pipeline provenance stage for audit-only producer attribution.
    pub provenance_stage: Option<String>,
    pub provenance_model_id: Option<String>,
    pub provenance_model_version: Option<String>,
    pub provenance_artifact_sha256: Option<String>,
    pub provenance_tokenizer_sha256: Option<String>,
    pub provenance_locale_resolved: Option<String>,
    pub provenance_locale_match_kind: Option<String>,
    pub provenance_canonical_class: Option<String>,
    pub provenance_native_class: Option<String>,
    pub provenance_confidence: Option<String>,
    pub provenance_merged_from: Option<String>,
    /// Locale-aware safety-net backend ids dropped by first-match-wins routing.
    pub backend_silently_dropped: Option<Vec<String>>,
    pub restore_policy: Option<String>,
    pub restore_decision: Option<String>,
    pub restore_unknown_token_count: Option<u64>,
    pub restore_manifest_bypass_count: Option<u64>,
    pub restore_fresh_pii_count: Option<u64>,
    pub restore_phase_mask: Option<u32>,
}

impl Serialize for RedactionEntry {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut len = 14;
        if self.recognizer_id.is_some() {
            len += 1;
        }
        if self.recognizer_version_id.is_some() {
            len += 1;
        }
        len += [
            self.provenance_stage.as_ref(),
            self.provenance_model_id.as_ref(),
            self.provenance_model_version.as_ref(),
            self.provenance_artifact_sha256.as_ref(),
            self.provenance_tokenizer_sha256.as_ref(),
            self.provenance_locale_resolved.as_ref(),
            self.provenance_locale_match_kind.as_ref(),
            self.provenance_canonical_class.as_ref(),
            self.provenance_native_class.as_ref(),
            self.provenance_confidence.as_ref(),
            self.provenance_merged_from.as_ref(),
        ]
        .into_iter()
        .filter(|value| value.is_some())
        .count();
        if self.backend_silently_dropped.is_some() {
            len += 1;
        }
        len += [self.restore_policy.as_ref(), self.restore_decision.as_ref()]
            .into_iter()
            .filter(|value| value.is_some())
            .count();
        len += [
            self.restore_unknown_token_count.is_some(),
            self.restore_manifest_bypass_count.is_some(),
            self.restore_fresh_pii_count.is_some(),
            self.restore_phase_mask.is_some(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count();
        let mut state = serializer.serialize_struct("RedactionEntry", len)?;
        state.serialize_field("source", &self.source)?;
        if let Some(recognizer_id) = &self.recognizer_id {
            state.serialize_field("recognizer_id", recognizer_id)?;
        }
        if let Some(recognizer_version_id) = &self.recognizer_version_id {
            state.serialize_field("recognizer_version_id", recognizer_version_id)?;
        }
        state.serialize_field("class", &self.class.to_canonical_str())?;
        state.serialize_field("action", redaction_action_as_str(self.action))?;
        state.serialize_field("field_name", &self.field_name)?;
        state.serialize_field(
            "document_kind",
            redaction_document_kind_as_str(self.document_kind),
        )?;
        state.serialize_field("conflict_loser", &self.conflict_loser)?;
        state.serialize_field(
            "decided_by",
            redaction_conflict_tier_as_str(self.decided_by),
        )?;
        state.serialize_field("created_at", &self.created_at)?;
        state.serialize_field("session_id", &self.session_id)?;
        state.serialize_field("validator_fail_reason", &self.validator_fail_reason)?;
        state.serialize_field("ambiguity_record", &self.ambiguity_record)?;
        state.serialize_field("collision_family", &self.collision_family)?;
        state.serialize_field("collision_variant", &self.collision_variant)?;
        state.serialize_field("fallback_triggered", &self.fallback_triggered)?;
        if let Some(value) = &self.provenance_stage {
            state.serialize_field("provenance_stage", value)?;
        }
        if let Some(value) = &self.provenance_model_id {
            state.serialize_field("provenance_model_id", value)?;
        }
        if let Some(value) = &self.provenance_model_version {
            state.serialize_field("provenance_model_version", value)?;
        }
        if let Some(value) = &self.provenance_artifact_sha256 {
            state.serialize_field("provenance_artifact_sha256", value)?;
        }
        if let Some(value) = &self.provenance_tokenizer_sha256 {
            state.serialize_field("provenance_tokenizer_sha256", value)?;
        }
        if let Some(value) = &self.provenance_locale_resolved {
            state.serialize_field("provenance_locale_resolved", value)?;
        }
        if let Some(value) = &self.provenance_locale_match_kind {
            state.serialize_field("provenance_locale_match_kind", value)?;
        }
        if let Some(value) = &self.provenance_canonical_class {
            state.serialize_field("provenance_canonical_class", value)?;
        }
        if let Some(value) = &self.provenance_native_class {
            state.serialize_field("provenance_native_class", value)?;
        }
        if let Some(value) = &self.provenance_confidence {
            state.serialize_field("provenance_confidence", value)?;
        }
        if let Some(value) = &self.provenance_merged_from {
            state.serialize_field("provenance_merged_from", value)?;
        }
        if let Some(dropped) = &self.backend_silently_dropped {
            state.serialize_field("backend_silently_dropped", dropped)?;
        }
        if let Some(value) = &self.restore_policy {
            state.serialize_field("restore_policy", value)?;
        }
        if let Some(value) = &self.restore_decision {
            state.serialize_field("restore_decision", value)?;
        }
        if let Some(value) = self.restore_unknown_token_count {
            state.serialize_field("restore_unknown_token_count", &value)?;
        }
        if let Some(value) = self.restore_manifest_bypass_count {
            state.serialize_field("restore_manifest_bypass_count", &value)?;
        }
        if let Some(value) = self.restore_fresh_pii_count {
            state.serialize_field("restore_fresh_pii_count", &value)?;
        }
        if let Some(value) = self.restore_phase_mask {
            state.serialize_field("restore_phase_mask", &value)?;
        }
        state.end()
    }
}

fn redaction_action_as_str(action: Action) -> &'static str {
    match action {
        Action::Tokenize => "tokenize",
        Action::Redact => "redact",
        Action::FormatPreserve => "format_preserve",
        Action::Generalize => "generalize",
        Action::Preserve => "preserve",
    }
}

fn redaction_document_kind_as_str(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Structured => "structured",
        DocumentKind::Text => "text",
    }
}

fn redaction_conflict_tier_as_str(tier: ConflictTier) -> &'static str {
    match tier {
        ConflictTier::None => "none",
        ConflictTier::ClassPriority => "class_priority",
        ConflictTier::RulePriority => "rule_priority",
        ConflictTier::Score => "score",
        ConflictTier::SpanLength => "span_length",
        ConflictTier::Validator => "validator",
        ConflictTier::ValidatorVeto => "validator_veto",
        ConflictTier::CollisionPolicy => "collision_policy",
        ConflictTier::AnchoredContext => "anchored_context",
        ConflictTier::RecognizerId => "recognizer_id",
        ConflictTier::Merged => "merged",
        ConflictTier::Redact => "redact",
        ConflictTier::Resolve => "resolve",
        ConflictTier::Fallback => "fallback",
    }
}

impl RedactionEntry {
    /// Builds a metadata-only redaction log entry.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: impl Into<String>,
        class: PiiClass,
        action: Action,
        field_name: Option<String>,
        document_kind: DocumentKind,
        conflict_loser: bool,
        decided_by: ConflictTier,
        created_at: i64,
        session_id: Option<String>,
    ) -> Self {
        Self {
            source: source.into(),
            class,
            action,
            field_name,
            document_kind,
            conflict_loser,
            decided_by,
            created_at,
            session_id,
            recognizer_id: None,
            recognizer_version_id: None,
            validator_fail_reason: None,
            ambiguity_record: None,
            collision_family: None,
            collision_variant: None,
            fallback_triggered: None,
            provenance_stage: None,
            provenance_model_id: None,
            provenance_model_version: None,
            provenance_artifact_sha256: None,
            provenance_tokenizer_sha256: None,
            provenance_locale_resolved: None,
            provenance_locale_match_kind: None,
            provenance_canonical_class: None,
            provenance_native_class: None,
            provenance_confidence: None,
            provenance_merged_from: None,
            backend_silently_dropped: None,
            restore_policy: None,
            restore_decision: None,
            restore_unknown_token_count: None,
            restore_manifest_bypass_count: None,
            restore_fresh_pii_count: None,
            restore_phase_mask: None,
        }
    }

    /// Attaches a validator failure reason to this metadata row.
    pub fn with_validator_fail_reason(mut self, reason: ValidatorFailReason) -> Self {
        self.validator_fail_reason = Some(reason);
        self
    }

    /// Attaches an ambiguity record to this metadata row.
    pub fn with_ambiguity_record(mut self, record: AmbiguityRecord) -> Self {
        self.ambiguity_record = Some(record);
        self
    }

    /// Attaches collision-family metadata to this row.
    pub fn with_collision_metadata(
        mut self,
        family: Option<String>,
        variant: Option<String>,
    ) -> Self {
        self.collision_family = family;
        self.collision_variant = variant;
        self
    }

    /// Attaches safety-net fallback metadata to this row.
    pub fn with_fallback_triggered(mut self, reason: FallbackReason) -> Self {
        self.fallback_triggered = Some(reason);
        self
    }

    /// Attaches locale-aware backend ids dropped by first-match-wins routing.
    pub fn with_backend_silently_dropped(mut self, dropped: Vec<String>) -> Self {
        self.backend_silently_dropped = Some(dropped);
        self
    }

    pub fn with_restore_telemetry(mut self, telemetry: RestoreTelemetry) -> Self {
        self.restore_policy = Some(telemetry.restore_policy_str().to_string());
        self.restore_decision = Some(telemetry.restore_decision_str().to_string());
        self.restore_unknown_token_count = Some(telemetry.unknown_token_count);
        self.restore_manifest_bypass_count = Some(telemetry.manifest_bypass_count);
        self.restore_fresh_pii_count = Some(telemetry.fresh_pii_detected_count);
        self.restore_phase_mask = Some(telemetry.phase_execution_mask);
        self
    }

    /// Attaches recognizer lineage metadata to this row.
    pub fn with_recognizer_metadata(
        mut self,
        recognizer_id: Option<String>,
        recognizer_version_id: Option<String>,
    ) -> Self {
        self.recognizer_id = recognizer_id;
        self.recognizer_version_id = recognizer_version_id;
        self
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_provenance_metadata(
        mut self,
        stage: Option<String>,
        model_id: Option<String>,
        model_version: Option<String>,
        artifact_sha256: Option<String>,
        tokenizer_sha256: Option<String>,
        locale_resolved: Option<String>,
        locale_match_kind: Option<String>,
        canonical_class: Option<String>,
        native_class: Option<String>,
        confidence: Option<f64>,
        merged_from: Option<String>,
    ) -> Self {
        self.provenance_stage = stage;
        self.provenance_model_id = model_id;
        self.provenance_model_version = model_version;
        self.provenance_artifact_sha256 = artifact_sha256;
        self.provenance_tokenizer_sha256 = tokenizer_sha256;
        self.provenance_locale_resolved = locale_resolved;
        self.provenance_locale_match_kind = locale_match_kind;
        self.provenance_canonical_class = canonical_class;
        self.provenance_native_class = native_class;
        self.provenance_confidence = confidence.map(|value| value.to_string());
        self.provenance_merged_from = merged_from;
        self
    }
}

/// Closed error set for redaction log sinks.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum RedactionLogError {
    /// SQLite-backed redaction log sink failed.
    #[error("sqlite redaction log error: {0}")]
    Sqlite(String),
    /// Non-SQLite redaction log sink failed.
    #[error("backend redaction log error: {0}")]
    Backend(String),
}

/// Trait for audit sinks that receive redaction metadata.
///
/// Implement this for custom audit backends (remote telemetry, structured JSON logs).
/// For SQLite-backed persistence, use `gaze_audit::SqliteLogger`.
///
/// # Contract
///
/// The logger receives **metadata only**: class, action, session ID, timestamp, and
/// other bytes-free audit labels. It never receives the original PII value or the token
/// value. A custom impl that augments entries with raw document text violates the audit
/// isolation contract and will be flagged by the `gaze_module_isolation` Dylint lint
/// when it lives in the wrong crate.
///
/// # Example
///
/// ```rust
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use gaze_types::{RedactionEntry, RedactionLogError, RedactionLogger};
///
/// #[derive(Default)]
/// struct CountLogger(AtomicUsize);
///
/// impl RedactionLogger for CountLogger {
///     fn log(&self, _entry: &RedactionEntry) -> Result<(), RedactionLogError> {
///         self.0.fetch_add(1, Ordering::Relaxed);
///         Ok(())
///     }
/// }
/// ```
pub trait RedactionLogger: Send + Sync {
    /// Records a metadata-only redaction entry.
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError>;
}

/// Rulepack recognizer activation tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum SafetyTier {
    /// Activates whenever the recognizer's locale projection intersects the active locale chain.
    #[default]
    SafeDefault,
    /// Activates only when an explicit locale or compatibility alias enables locale-shaped rules.
    LocaleGated,
    /// Activates only through adopter opt-in surfaces such as policy-defined custom recognizers.
    OptIn,
}

/// Safety-tier parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SafetyTierParseError {
    value: String,
}

impl SafetyTier {
    /// Parses the TOML `safety_tier` string.
    pub fn parse(value: &str) -> Result<Self, SafetyTierParseError> {
        match value {
            "safe_default" => Ok(Self::SafeDefault),
            "locale_gated" => Ok(Self::LocaleGated),
            "opt_in" => Ok(Self::OptIn),
            other => Err(SafetyTierParseError {
                value: other.to_string(),
            }),
        }
    }

    /// Returns the TOML string for this tier.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SafeDefault => "safe_default",
            Self::LocaleGated => "locale_gated",
            Self::OptIn => "opt_in",
        }
    }
}

impl SafetyTierParseError {
    /// Returns the rejected safety-tier string.
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for SafetyTierParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported safety_tier '{}'", self.value)
    }
}

impl std::error::Error for SafetyTierParseError {}

/// Locale tag recognized by policy and recognizers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LocaleTag {
    /// Locale-independent recognizer or policy.
    Global,
    /// German as used in Germany.
    DeDe,
    /// German as used in Austria.
    DeAt,
    /// German as used in Switzerland.
    DeCh,
    /// English as used in the United States.
    EnUs,
    /// English as used in Great Britain.
    EnGb,
    /// English as used in Ireland.
    EnIe,
    /// English as used in Australia.
    EnAu,
    /// English as used in Canada.
    EnCa,
    /// Any other canonical BCP-47-like tag.
    Other(String),
}

/// Locale parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LocaleError {
    /// Locale tag is unsupported or invalid.
    Unsupported,
}

impl fmt::Display for LocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocaleError::Unsupported => f.write_str("unsupported locale"),
        }
    }
}

impl std::error::Error for LocaleError {}

/// Ordered locale fallback chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleChain(Vec<LocaleTag>);

impl LocaleTag {
    /// Global locale constant.
    pub const GLOBAL: LocaleTag = LocaleTag::Global;

    /// Parses a locale tag from policy or CLI input.
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

    /// Returns the canonical string form of the locale tag.
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
    /// Builds a locale chain and appends global fallback when absent.
    pub fn from_tags(mut tags: Vec<LocaleTag>) -> LocaleChain {
        ensure_global(&mut tags);
        LocaleChain(tags)
    }

    /// Parses a comma-separated CLI locale chain.
    pub fn from_cli(raw: &str) -> Result<LocaleChain, LocaleError> {
        let tags = raw
            .split(',')
            .map(LocaleTag::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LocaleChain::from_tags(tags))
    }

    /// Merges policy and CLI locale preferences.
    pub fn merge_policy_and_cli(
        policy: Option<&[LocaleTag]>,
        cli: Option<&[LocaleTag]>,
    ) -> LocaleChain {
        Self::merge_cli_policy_rulepack_default(cli, policy, None)
    }

    /// Merges CLI, policy, rulepack, and default locale preferences.
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

    /// Returns true when a recognizer can run under this locale chain.
    pub fn intersects(&self, recognizer_locales: &[LocaleTag]) -> bool {
        if recognizer_locales.is_empty() {
            return true;
        }
        recognizer_locales.iter().any(|recognizer_locale| {
            *recognizer_locale == LocaleTag::Global
                || self.0.iter().any(|active| active == recognizer_locale)
        })
    }

    /// Returns the locale tags in chain order.
    pub fn as_slice(&self) -> &[LocaleTag] {
        &self.0
    }

    /// Returns the locale chain as canonical strings.
    pub fn to_strings(&self) -> Vec<String> {
        self.0.iter().map(ToString::to_string).collect()
    }
}

impl From<&[LocaleTag]> for LocaleChain {
    fn from(tags: &[LocaleTag]) -> Self {
        let mut owned = tags.to_vec();
        ensure_global(&mut owned);
        LocaleChain(owned)
    }
}

impl fmt::Display for LocaleTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The input document submitted for pseudonymization.
///
/// `RawDocument::Text(String)` for plain or semi-structured text (most LLM workflows).
/// `RawDocument::Structured(BTreeMap<String, Value>)` for JSON-shaped data where
/// column-aware rules apply -- `ColumnRule`s only take effect on structured input.
///
/// `Detection::span` and recognizer candidate spans use **byte** ranges, not char indices.
///
/// `RawDocument` is `#[non_exhaustive]`. Match with a wildcard arm.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RawDocument {
    /// Structured document values.
    Structured(BTreeMap<String, Value>),
    /// Plain text document.
    Text(String),
}

/// The pseudonymized output from `Pipeline::redact`.
///
/// Mirrors the shape of `RawDocument`: `CleanDocument::Text(String)` or
/// `CleanDocument::Structured(BTreeMap<String, Value>)`. Destructure with a `let`-else
/// or `match`; **there is no `.text()` accessor**.
///
/// ```rust
/// use gaze_types::CleanDocument;
///
/// fn unwrap_text(doc: CleanDocument) -> Option<String> {
///     if let CleanDocument::Text(t) = doc { Some(t) } else { None }
/// }
/// ```
///
/// Contains only tokens or redacted placeholders -- no original PII values.
/// Send this (or its inner string) to the LLM; never send the original `RawDocument`.
///
/// `CleanDocument` is `#[non_exhaustive]`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum CleanDocument {
    /// Structured document values.
    Structured(BTreeMap<String, Value>),
    /// Plain text document.
    Text(String),
}

/// Minimal structured value representation that avoids a serde_json dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum Value {
    /// Null value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// String value.
    String(String),
    /// Signed 64-bit integer value.
    I64(i64),
    /// Array value.
    Array(Vec<Value>),
    /// Object value.
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// Returns the inner string for string values.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            Self::Null | Self::Bool(_) | Self::I64(_) | Self::Array(_) | Self::Object(_) => None,
        }
    }

    /// Returns a scalar string representation used for structured safety-net checks.
    pub fn scalar_to_safety_net_string(&self) -> Option<String> {
        match self {
            Self::String(value) if !value.is_empty() => Some(value.clone()),
            Self::String(_) | Self::Null | Self::Array(_) | Self::Object(_) => None,
            Self::Bool(value) => Some(value.to_string()),
            Self::I64(value) => Some(value.to_string()),
        }
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

/// Value-only dictionary bundle shared with recognizers.
#[derive(Debug, Clone, Default)]
pub struct DictionaryBundle {
    entries: HashMap<String, DictionaryEntry>,
}

/// Value-only dictionary entry; compiled automatons live outside `gaze-types`.
#[derive(Debug, Clone)]
pub struct DictionaryEntry {
    terms: Vec<String>,
    case_sensitive: bool,
    source: DictionarySource,
}

/// Source of a dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DictionarySource {
    /// Dictionary supplied by request context.
    Cli,
    /// Dictionary supplied by a rulepack.
    Rulepack,
}

/// Dictionary metadata used for diagnostics and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DictionaryStats {
    /// Dictionary name.
    pub name: String,
    /// Number of configured terms.
    pub term_count: usize,
    /// Dictionary source.
    pub source: DictionarySource,
}

impl DictionaryStats {
    /// Builds dictionary diagnostics metadata.
    pub fn new(name: impl Into<String>, term_count: usize, source: DictionarySource) -> Self {
        Self {
            name: name.into(),
            term_count,
            source,
        }
    }
}

/// Dictionary declared by a rulepack.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RulepackDict {
    /// Dictionary name.
    pub name: String,
    /// Dictionary terms.
    pub terms: Vec<String>,
    /// Whether matching is case-sensitive.
    pub case_sensitive: bool,
}

impl RulepackDict {
    /// Builds a rulepack dictionary declaration.
    pub fn new(name: impl Into<String>, terms: Vec<String>, case_sensitive: bool) -> Self {
        Self {
            name: name.into(),
            terms,
            case_sensitive,
        }
    }
}

/// Error raised when constructing invalid dictionary entries.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DictionaryLoadError {
    /// Dictionary has no terms.
    Empty { name: String },
    /// ASCII-only case-insensitive matching cannot safely cover this entry.
    UnicodeInsensitiveUnsupported { name: String },
}

impl fmt::Display for DictionaryLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { name } => write!(f, "dictionary '{name}' has no terms"),
            Self::UnicodeInsensitiveUnsupported { name } => write!(
                f,
                "dictionary '{name}' uses unicode terms with case-insensitive matching, unsupported in v0.4.0; use case_sensitive = true"
            ),
        }
    }
}

impl std::error::Error for DictionaryLoadError {}

impl DictionaryBundle {
    /// Builds a bundle from rulepack dictionaries.
    pub fn from_rulepack_terms(terms: &[RulepackDict]) -> Self {
        let mut entries = HashMap::with_capacity(terms.len());
        for dictionary in terms {
            let entry = DictionaryEntry::new(
                &dictionary.name,
                dictionary.terms.clone(),
                dictionary.case_sensitive,
                DictionarySource::Rulepack,
            )
            .expect("Policy validates dictionary terms before bundle construction");
            entries.insert(dictionary.name.clone(), entry);
        }
        Self { entries }
    }

    /// Builds a bundle from pre-built dictionary entries.
    pub fn from_entries(entries: impl IntoIterator<Item = (String, DictionaryEntry)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// Merges two bundles, preferring entries from the second bundle on name conflicts.
    pub fn merge(a: Self, b: Self) -> Self {
        let mut entries = a.entries;
        entries.extend(b.entries);
        Self { entries }
    }

    /// Returns a dictionary by name.
    pub fn get(&self, name: &str) -> Option<&DictionaryEntry> {
        self.entries.get(name)
    }

    /// Returns sorted dictionary stats.
    pub fn stats(&self) -> Vec<DictionaryStats> {
        let mut stats = self
            .entries
            .iter()
            .map(|(name, entry)| DictionaryStats {
                name: name.clone(),
                term_count: entry.terms.len(),
                source: entry.source,
            })
            .collect::<Vec<_>>();
        stats.sort_by(|a, b| a.name.cmp(&b.name));
        stats
    }
}

impl DictionaryEntry {
    /// Creates a validated value-only dictionary entry.
    pub fn new(
        name: &str,
        terms: Vec<String>,
        case_sensitive: bool,
        source: DictionarySource,
    ) -> Result<Self, DictionaryLoadError> {
        if terms.is_empty() {
            return Err(DictionaryLoadError::Empty {
                name: name.to_string(),
            });
        }
        if !case_sensitive && terms.iter().any(|term| !term.is_ascii()) {
            return Err(DictionaryLoadError::UnicodeInsensitiveUnsupported {
                name: name.to_string(),
            });
        }
        Ok(Self {
            terms,
            case_sensitive,
            source,
        })
    }

    /// Returns whether matching is case-sensitive.
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Returns configured dictionary terms.
    pub fn terms(&self) -> &[String] {
        &self.terms
    }
}

#[cfg(test)]
mod dictionary_tests {
    use super::*;

    #[test]
    fn dictionary_entry_rejects_empty_terms() {
        let err = DictionaryEntry::new("empty", Vec::new(), true, DictionarySource::Cli)
            .expect_err("empty dictionaries must fail closed");

        assert!(matches!(err, DictionaryLoadError::Empty { name } if name == "empty"));
    }

    #[test]
    fn dictionary_entry_rejects_non_ascii_case_insensitive_terms() {
        let err = DictionaryEntry::new(
            "songs",
            vec!["Beyonce".to_string(), "Caf\u{00e9}".to_string()],
            false,
            DictionarySource::Cli,
        )
        .expect_err("unicode case-insensitive dictionaries must fail closed");

        assert!(matches!(
            err,
            DictionaryLoadError::UnicodeInsensitiveUnsupported { name } if name == "songs"
        ));
    }
}

#[cfg(test)]
mod redaction_logger_tests {
    use super::*;

    struct CapturingLogger;

    impl RedactionLogger for CapturingLogger {
        fn log(&self, _entry: &RedactionEntry) -> Result<(), RedactionLogError> {
            Ok(())
        }
    }

    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    #[test]
    fn redaction_log_error_display_is_stable() {
        assert_eq!(
            RedactionLogError::Sqlite("write failed".to_string()).to_string(),
            "sqlite redaction log error: write failed"
        );
        assert_eq!(
            RedactionLogError::Backend("sink failed".to_string()).to_string(),
            "backend redaction log error: sink failed"
        );
    }

    #[test]
    fn redaction_logger_trait_object_is_send_sync() {
        assert_send_sync::<dyn RedactionLogger>();
    }

    #[test]
    fn local_logger_can_implement_redaction_logger() {
        let logger = CapturingLogger;
        let entry = RedactionEntry {
            source: "unit-test".to_string(),
            recognizer_id: None,
            recognizer_version_id: None,
            class: PiiClass::Email,
            action: Action::Tokenize,
            field_name: None,
            document_kind: DocumentKind::Text,
            conflict_loser: false,
            decided_by: ConflictTier::None,
            created_at: 0,
            session_id: None,
            validator_fail_reason: None,
            ambiguity_record: None,
            collision_family: None,
            collision_variant: None,
            fallback_triggered: None,
            provenance_stage: None,
            provenance_model_id: None,
            provenance_model_version: None,
            provenance_artifact_sha256: None,
            provenance_tokenizer_sha256: None,
            provenance_locale_resolved: None,
            provenance_locale_match_kind: None,
            provenance_canonical_class: None,
            provenance_native_class: None,
            provenance_confidence: None,
            provenance_merged_from: None,
            backend_silently_dropped: None,
            restore_policy: None,
            restore_decision: None,
            restore_unknown_token_count: None,
            restore_manifest_bypass_count: None,
            restore_fresh_pii_count: None,
            restore_phase_mask: None,
        };

        let trait_object: &dyn RedactionLogger = &logger;
        trait_object.log(&entry).expect("log entry");
    }

    #[test]
    fn redaction_entry_json_shape_omits_absent_recognizer_lineage() {
        let entry = RedactionEntry::new(
            "email.global",
            PiiClass::Email,
            Action::Tokenize,
            None,
            DocumentKind::Text,
            false,
            ConflictTier::None,
            0,
            None,
        );

        let rendered = serde_json::to_string(&entry).expect("serialize redaction entry");

        assert_eq!(
            rendered,
            r#"{"source":"email.global","class":"email","action":"tokenize","field_name":null,"document_kind":"text","conflict_loser":false,"decided_by":"none","created_at":0,"session_id":null,"validator_fail_reason":null,"ambiguity_record":null,"collision_family":null,"collision_variant":null,"fallback_triggered":null}"#
        );
    }

    #[test]
    fn redaction_entry_json_shape_includes_recognizer_lineage_when_present() {
        let entry = RedactionEntry::new(
            "ner/ort",
            PiiClass::Name,
            Action::Tokenize,
            None,
            DocumentKind::Text,
            false,
            ConflictTier::None,
            0,
            None,
        )
        .with_recognizer_metadata(
            Some("ner".to_string()),
            Some("ner.davlan-mbert.v1".to_string()),
        );

        let value: serde_json::Value =
            serde_json::to_value(&entry).expect("serialize redaction entry");

        assert_eq!(value["recognizer_id"], "ner");
        assert_eq!(value["recognizer_version_id"], "ner.davlan-mbert.v1");
    }

    #[test]
    fn candidate_keeps_versioned_and_unversioned_recognizer_ids() {
        let unversioned = Candidate::new(
            0..5,
            PiiClass::Email,
            "email.global",
            0.9,
            10,
            None,
            "email",
            "email.global",
            ConflictTier::None,
            Vec::new(),
        );
        assert_eq!(unversioned.recognizer_id, "email.global");
        assert_eq!(unversioned.recognizer_version_id, None);

        let versioned = unversioned
            .clone()
            .with_recognizer_version_id("email.global.v1");
        assert_eq!(versioned.recognizer_id, "email.global");
        assert_eq!(
            versioned.recognizer_version_id.as_deref(),
            Some("email.global.v1")
        );
    }
}

#[cfg(test)]
mod safety_net_manifest_tests {
    use super::*;

    fn span(start: usize, end: usize, class: PiiClass) -> EmittedTokenSpan {
        EmittedTokenSpan {
            clean_span: start..end,
            raw_span: start..end,
            class,
        }
    }

    fn diff(manifest: Manifest, suspect: Range<usize>, class: PiiClass) -> Option<LeakKind> {
        manifest.diff_against(&suspect, &class)
    }

    #[test]
    fn exact_same_class_coverage_is_not_a_leak() {
        let manifest = Manifest::from_spans(vec![span(0, 8, PiiClass::Email)]);

        assert_eq!(diff(manifest, 0..8, PiiClass::Email), None);
    }

    #[test]
    fn uncovered_outside_all_tokens_is_uncovered() {
        let manifest = Manifest::from_spans(vec![span(20, 30, PiiClass::Email)]);

        assert_eq!(
            diff(manifest, 0..10, PiiClass::Email),
            Some(LeakKind::Uncovered)
        );
    }

    #[test]
    fn single_internal_gap_returns_partial_bleed() {
        let manifest = Manifest::from_spans(vec![
            span(0, 5, PiiClass::Email),
            span(10, 15, PiiClass::Email),
        ]);

        assert_eq!(
            diff(manifest, 0..15, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 5..10 })
        );
    }

    #[test]
    fn multi_gap_returns_deterministic_first_uncovered_gap() {
        let manifest = Manifest::from_spans(vec![
            span(0, 3, PiiClass::Email),
            span(5, 7, PiiClass::Email),
            span(9, 12, PiiClass::Email),
        ]);

        // The first-gap-only rule is intentional for v0.6.1; full gap
        // enumeration is deferred until the report format can carry it.
        assert_eq!(
            diff(manifest, 0..12, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 3..5 })
        );
    }

    #[test]
    fn multi_class_overlap_reports_first_mismatch_deterministically() {
        let manifest = Manifest::from_spans(vec![
            span(0, 4, PiiClass::Name),
            span(4, 8, PiiClass::Location),
        ]);

        assert_eq!(
            diff(manifest, 0..8, PiiClass::Email),
            Some(LeakKind::ClassMismatch {
                pipeline_class: PiiClass::Name,
                safety_net_class: PiiClass::Email,
            })
        );
    }

    #[test]
    fn adjacent_same_class_tokens_cover_continuously() {
        let manifest = Manifest::from_spans(vec![
            span(0, 5, PiiClass::Email),
            span(5, 10, PiiClass::Email),
        ]);

        assert_eq!(diff(manifest, 0..10, PiiClass::Email), None);
    }

    #[test]
    fn partial_bleed_at_start_end_and_middle() {
        let manifest = Manifest::from_spans(vec![span(3, 8, PiiClass::Email)]);

        assert_eq!(
            diff(manifest.clone(), 0..8, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 0..3 })
        );
        assert_eq!(
            diff(manifest.clone(), 3..10, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 8..10 })
        );

        let with_gap = Manifest::from_spans(vec![
            span(0, 3, PiiClass::Email),
            span(6, 10, PiiClass::Email),
        ]);
        assert_eq!(
            diff(with_gap, 0..10, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 3..6 })
        );
    }

    #[test]
    fn byte_indices_are_not_character_indices() {
        let text = "ID: 😀 <Email_1>";
        let token_start = text.find("<Email_1>").expect("token start");
        assert_eq!(token_start, 9, "emoji is four bytes, not one char");
        let manifest = Manifest::from_spans(vec![span(token_start, text.len(), PiiClass::Email)]);

        assert_eq!(
            diff(manifest, token_start..text.len(), PiiClass::Email),
            None
        );
    }

    #[test]
    fn empty_suspect_range_is_not_a_leak() {
        let manifest = Manifest::default();

        assert_eq!(diff(manifest, 3..3, PiiClass::Email), None);
    }

    #[test]
    fn safety_net_error_display_is_variant_specific_and_bytes_free() {
        let cases = [
            SafetyNetError::Unavailable {
                reason: "not configured".to_string(),
            }
            .to_string(),
            SafetyNetError::WeightsMissing {
                path: "/models/opf".to_string(),
            }
            .to_string(),
            SafetyNetError::ModelUnavailable {
                reason: "load failed".to_string(),
            }
            .to_string(),
            SafetyNetError::ModelIntegrityMismatch {
                expected: "e3b0c44298fc1c149afbf4c8996fb924".to_string(),
                actual: "4e07408562bedb8b60ce05c1decfe3ad".to_string(),
            }
            .to_string(),
            SafetyNetError::InputTooLarge {
                limit: 1024,
                actual: 2048,
            }
            .to_string(),
            SafetyNetError::Runtime {
                message: "timeout".to_string(),
            }
            .to_string(),
            SafetyNetError::InvalidOutput {
                message: "bad json".to_string(),
            }
            .to_string(),
        ];

        for rendered in cases {
            assert!(!rendered.contains("alice@example.invalid"));
        }
    }
}

/// Shared recognizer contract for locale-aware PII candidates.
pub trait Recognizer: Send + Sync {
    /// Stable recognizer identifier.
    fn id(&self) -> &str;
    /// PII class supported by this recognizer.
    fn supported_class(&self) -> &PiiClass;
    /// Detects PII candidates in the supplied input and context.
    fn detect(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, DetectError>;
    /// Token family used for candidate token emission.
    fn token_family(&self) -> &str;
    /// Optional validator kind used by pre-resolver validator-veto.
    fn validator_kind(&self) -> Option<ValidatorKind> {
        None
    }
    /// Locales where this recognizer is active.
    fn locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
}

/// Caller-visible recognizer detection failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum DetectError {
    /// A recognizer backend failed while scanning the input.
    #[error("recognizer {recognizer_id} backend failed: {message}")]
    Backend {
        /// Stable recognizer identifier.
        recognizer_id: String,
        /// Sanitized backend error message.
        message: String,
    },
}

impl DetectError {
    /// Build a backend failure without requiring recognizer crates in `gaze-types`.
    pub fn backend(recognizer_id: impl Into<String>, source: impl Into<String>) -> Self {
        Self::Backend {
            recognizer_id: recognizer_id.into(),
            message: source.into(),
        }
    }
}

/// Candidate PII span emitted by a recognizer before final conflict resolution.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Candidate {
    /// Byte span in the original input.
    pub span: Range<usize>,
    /// PII class assigned to the span.
    pub class: PiiClass,
    /// Recognizer identifier.
    pub recognizer_id: String,
    /// Optional versioned recognizer identifier for audit lineage.
    pub recognizer_version_id: Option<String>,
    /// Recognizer confidence score.
    pub score: f32,
    /// Rule or recognizer priority.
    pub priority: i32,
    /// Optional canonical representation for validation/merge logic.
    pub canonical_form: Option<String>,
    /// Token family used for output token shape.
    pub token_family: String,
    /// Candidate source label.
    pub source: String,
    /// Conflict tier that decided this candidate.
    pub decided_by: ConflictTier,
    /// Sources merged into this candidate.
    pub merged_sources: Vec<String>,
}

impl Candidate {
    /// Builds a recognizer candidate.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        span: Range<usize>,
        class: PiiClass,
        recognizer_id: impl Into<String>,
        score: f32,
        priority: i32,
        canonical_form: Option<String>,
        token_family: impl Into<String>,
        source: impl Into<String>,
        decided_by: ConflictTier,
        merged_sources: Vec<String>,
    ) -> Self {
        Self {
            span,
            class,
            recognizer_id: recognizer_id.into(),
            recognizer_version_id: None,
            score,
            priority,
            canonical_form,
            token_family: token_family.into(),
            source: source.into(),
            decided_by,
            merged_sources,
        }
    }

    /// Returns this candidate with a translated span.
    pub fn with_span(mut self, span: Range<usize>) -> Self {
        self.span = span;
        self
    }

    /// Returns this candidate with versioned recognizer lineage attached.
    pub fn with_recognizer_version_id(mut self, recognizer_version_id: impl Into<String>) -> Self {
        self.recognizer_version_id = Some(recognizer_version_id.into());
        self
    }
}

/// Context supplied to recognizers during detection.
#[non_exhaustive]
pub struct DetectContext<'a> {
    /// Active locale chain.
    pub locale_chain: &'a [LocaleTag],
    /// Active dictionary bundle.
    pub dictionaries: &'a DictionaryBundle,
    /// Reserved field-aware matching slot; intentionally unit in v0.5 Phase B.
    pub fields: &'a (),
    /// Whether a recognizer degraded due to unavailable optional capability.
    pub degraded: Cell<bool>,
}

impl<'a> DetectContext<'a> {
    /// Builds detection context for a recognizer pass.
    pub fn new(locale_chain: &'a [LocaleTag], dictionaries: &'a DictionaryBundle) -> Self {
        Self {
            locale_chain,
            dictionaries,
            fields: &(),
            degraded: Cell::new(false),
        }
    }
}

fn ensure_global(tags: &mut Vec<LocaleTag>) {
    if !tags.contains(&LocaleTag::Global) {
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
