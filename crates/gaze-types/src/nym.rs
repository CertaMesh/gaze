//! Closed label vocabulary, class map and operating point for the Nym-small safety net.
//!
//! The Nym-small model (`Wismut/nym-pii-multilingual-small`) labels 40 entity types. Only the
//! labels that carry a Gaze class can ever be enabled, and every enabled label needs an explicit
//! threshold. Both rules are checked when the operating point is built, so a policy that names an
//! unknown or unmapped label fails at load time instead of being skipped at inference time.

use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

use crate::{PiiClass, SafetyNetPiiClass};

/// Stable safety-net identifier recorded on every Nym suspect and audit row.
pub const NYM_SAFETY_NET_ID: &str = "nym-small-int8";

/// Prefix of the recognizer id and candidate source of every Nym recognizer candidate
/// (`nym/LICENSE_PLATE`). The resolver reads it to place those candidates in the learned
/// evidence tier.
pub const NYM_RECOGNIZER_SOURCE_PREFIX: &str = "nym/";

macro_rules! nym_labels {
    ($($variant:ident => $name:literal,)+) => {
        /// The 40 entity types of the pinned Nym-small classifier, in `id2label` order.
        ///
        /// Classifier id `2k + 1` is `B-` and `2k + 2` is `I-` of the label at index `k`; id 0
        /// is `O`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum NymLabel {
            $(
                #[doc = concat!("`", $name, "`.")]
                $variant,
            )+
        }

        impl NymLabel {
            /// Every label in classifier order.
            pub const ALL: &'static [NymLabel] = &[$(Self::$variant,)+];

            /// Upstream label spelling.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            /// Parses an upstream label spelling. Matching is exact.
            pub fn parse(value: &str) -> Option<Self> {
                match value {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

nym_labels! {
    AccountNumber => "ACCOUNT_NUMBER",
    Age => "AGE",
    ApiKey => "API_KEY",
    BuildingNumber => "BUILDING_NUMBER",
    City => "CITY",
    CompanyName => "COMPANY_NAME",
    Country => "COUNTRY",
    CreditDebitCard => "CREDIT_DEBIT_CARD",
    CustomerId => "CUSTOMER_ID",
    Cvv => "CVV",
    Date => "DATE",
    DateOfBirth => "DATE_OF_BIRTH",
    DriversLicense => "DRIVERS_LICENSE",
    Email => "EMAIL",
    EmployeeId => "EMPLOYEE_ID",
    FaxNumber => "FAX_NUMBER",
    Gender => "GENDER",
    GivenName => "GIVEN_NAME",
    GovernmentId => "GOVERNMENT_ID",
    Iban => "IBAN",
    LicensePlate => "LICENSE_PLATE",
    MacAddress => "MAC_ADDRESS",
    MedicalRecordNumber => "MEDICAL_RECORD_NUMBER",
    Passport => "PASSPORT",
    Password => "PASSWORD",
    Phone => "PHONE",
    Pin => "PIN",
    RoutingNumber => "ROUTING_NUMBER",
    SecondaryAddress => "SECONDARY_ADDRESS",
    Ssn => "SSN",
    State => "STATE",
    StreetAddress => "STREET_ADDRESS",
    StreetName => "STREET_NAME",
    Surname => "SURNAME",
    SwiftBic => "SWIFT_BIC",
    TaxId => "TAX_ID",
    Time => "TIME",
    Url => "URL",
    Username => "USERNAME",
    ZipCode => "ZIP_CODE",
}

impl fmt::Display for NymLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Maps a Nym label into the closed safety-net class set.
///
/// Six labels carry a class. Every other label is `Unsupported`: it can never be enabled, and it
/// is never folded into a generic class such as `Name`.
pub fn nym_label_to_safety_net_class(label: NymLabel) -> Result<SafetyNetPiiClass, NymConfigError> {
    match label {
        NymLabel::BuildingNumber => Ok(SafetyNetPiiClass::BuildingNumber),
        NymLabel::LicensePlate => Ok(SafetyNetPiiClass::LicensePlate),
        NymLabel::Username => Ok(SafetyNetPiiClass::Username),
        NymLabel::DateOfBirth => Ok(SafetyNetPiiClass::Date),
        NymLabel::ZipCode => Ok(SafetyNetPiiClass::PostalCode),
        NymLabel::TaxId => Ok(SafetyNetPiiClass::TaxId),
        unsupported => Err(NymConfigError::UnsupportedLabel {
            label: unsupported.as_str().to_string(),
        }),
    }
}

/// Convenience: Nym label to Gaze `PiiClass`.
pub fn nym_label_to_pii_class(label: NymLabel) -> Result<PiiClass, NymConfigError> {
    nym_label_to_safety_net_class(label).map(SafetyNetPiiClass::to_pii_class)
}

/// Why a Nym operating point was rejected.
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum NymConfigError {
    /// The label is not one of the 40 Nym labels.
    #[error("unknown nym label `{label}`")]
    UnknownLabel {
        /// The rejected spelling.
        label: String,
    },
    /// The label exists but carries no Gaze class, so it cannot be enabled.
    #[error("nym label `{label}` has no gaze class and cannot be enabled")]
    UnsupportedLabel {
        /// The rejected label.
        label: String,
    },
    /// The label appears twice in the allowlist.
    #[error("nym label `{label}` is listed more than once")]
    DuplicateLabel {
        /// The repeated label.
        label: String,
    },
    /// An enabled label has no threshold.
    #[error("nym label `{label}` is enabled without a threshold")]
    MissingThreshold {
        /// The label without a threshold.
        label: String,
    },
    /// A threshold names a label that is not enabled.
    #[error("nym threshold for `{label}` names a label that is not in `labels`")]
    ThresholdWithoutLabel {
        /// The label with a stray threshold.
        label: String,
    },
    /// A threshold is not a finite number in `(0, 1]`.
    #[error("nym threshold for `{label}` must be in (0, 1], got {threshold}")]
    ThresholdOutOfRange {
        /// The label.
        label: String,
        /// The rejected threshold.
        threshold: f32,
    },
    /// No label is enabled, so the net could never report anything.
    #[error("nym allowlist is empty")]
    EmptyAllowlist,
}

/// Which Nym labels may produce a suspect, and the entity probability each one needs.
///
/// The allowlist is the key set. Built only through validating constructors, so every key maps to
/// a Gaze class and every threshold is in `(0, 1]`.
#[derive(Debug, Clone, PartialEq)]
pub struct NymOperatingPoint {
    thresholds: BTreeMap<NymLabel, f32>,
}

impl NymOperatingPoint {
    /// The shipped default, measured as op-B on the 2,910-document benchmark: building number,
    /// licence plate and username at 0.5, date of birth at 0.9. Tax ID and ZIP code stay off
    /// (tax ID precision was 0.23 to 0.26; ZIP code flagged the invalid-identifier decoys).
    pub fn op_b() -> Self {
        Self {
            thresholds: BTreeMap::from([
                (NymLabel::BuildingNumber, 0.5),
                (NymLabel::DateOfBirth, 0.9),
                (NymLabel::LicensePlate, 0.5),
                (NymLabel::Username, 0.5),
            ]),
        }
    }

    /// Builds an operating point from typed labels.
    pub fn new(
        thresholds: impl IntoIterator<Item = (NymLabel, f32)>,
    ) -> Result<Self, NymConfigError> {
        let mut out = BTreeMap::new();
        for (label, threshold) in thresholds {
            nym_label_to_safety_net_class(label)?;
            if !(threshold.is_finite() && threshold > 0.0 && threshold <= 1.0) {
                return Err(NymConfigError::ThresholdOutOfRange {
                    label: label.as_str().to_string(),
                    threshold,
                });
            }
            if out.insert(label, threshold).is_some() {
                return Err(NymConfigError::DuplicateLabel {
                    label: label.as_str().to_string(),
                });
            }
        }
        if out.is_empty() {
            return Err(NymConfigError::EmptyAllowlist);
        }
        Ok(Self { thresholds: out })
    }

    /// Builds an operating point from the policy spelling: an allowlist plus a threshold map
    /// that must name exactly the allowlisted labels.
    pub fn from_labels_and_thresholds(
        labels: &[String],
        thresholds: &BTreeMap<String, f32>,
    ) -> Result<Self, NymConfigError> {
        let parse = |raw: &str| {
            NymLabel::parse(raw).ok_or_else(|| NymConfigError::UnknownLabel {
                label: raw.to_string(),
            })
        };
        for raw in thresholds.keys() {
            parse(raw)?;
            if !labels.contains(raw) {
                return Err(NymConfigError::ThresholdWithoutLabel { label: raw.clone() });
            }
        }
        let mut pairs = Vec::with_capacity(labels.len());
        for raw in labels {
            let label = parse(raw)?;
            nym_label_to_safety_net_class(label)?;
            let threshold = *thresholds
                .get(raw)
                .ok_or_else(|| NymConfigError::MissingThreshold { label: raw.clone() })?;
            pairs.push((label, threshold));
        }
        Self::new(pairs)
    }

    /// Threshold for `label`, or `None` when the label is not enabled.
    pub fn threshold(&self, label: NymLabel) -> Option<f32> {
        self.thresholds.get(&label).copied()
    }

    /// Enabled labels with their thresholds, in label order.
    pub fn iter(&self) -> impl Iterator<Item = (NymLabel, f32)> + '_ {
        self.thresholds
            .iter()
            .map(|(label, threshold)| (*label, *threshold))
    }
}

impl Default for NymOperatingPoint {
    fn default() -> Self {
        Self::op_b()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_order_matches_the_pinned_id2label() {
        assert_eq!(NymLabel::ALL.len(), 40);
        let mut sorted = NymLabel::ALL.iter().map(|l| l.as_str()).collect::<Vec<_>>();
        sorted.sort_unstable();
        let actual = NymLabel::ALL.iter().map(|l| l.as_str()).collect::<Vec<_>>();
        assert_eq!(actual, sorted, "id2label lists the types alphabetically");
        for label in NymLabel::ALL {
            assert_eq!(NymLabel::parse(label.as_str()), Some(*label));
        }
        assert_eq!(NymLabel::parse("zip_code"), None);
    }

    #[test]
    fn class_map_is_closed_and_never_generic() {
        let mapped = NymLabel::ALL
            .iter()
            .filter_map(|label| {
                nym_label_to_pii_class(*label)
                    .ok()
                    .map(|class| (label.as_str(), class.to_canonical_str()))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            mapped,
            vec![
                ("BUILDING_NUMBER", "custom:building_number".to_string()),
                ("DATE_OF_BIRTH", "custom:date".to_string()),
                ("LICENSE_PLATE", "custom:license_plate".to_string()),
                ("TAX_ID", "custom:tax_id".to_string()),
                ("USERNAME", "custom:username".to_string()),
                ("ZIP_CODE", "custom:postal_code".to_string()),
            ]
        );
        assert_eq!(
            nym_label_to_safety_net_class(NymLabel::GivenName),
            Err(NymConfigError::UnsupportedLabel {
                label: "GIVEN_NAME".to_string()
            })
        );
    }

    #[test]
    fn op_b_is_the_default() {
        let op_b = NymOperatingPoint::default();
        assert_eq!(
            op_b.iter().collect::<Vec<_>>(),
            vec![
                (NymLabel::BuildingNumber, 0.5),
                (NymLabel::DateOfBirth, 0.9),
                (NymLabel::LicensePlate, 0.5),
                (NymLabel::Username, 0.5),
            ]
        );
        assert_eq!(op_b.threshold(NymLabel::TaxId), None);
        assert_eq!(op_b.threshold(NymLabel::ZipCode), None);
        assert_eq!(op_b.threshold(NymLabel::GivenName), None);
    }

    fn policy(
        labels: &[&str],
        thresholds: &[(&str, f32)],
    ) -> Result<NymOperatingPoint, NymConfigError> {
        NymOperatingPoint::from_labels_and_thresholds(
            &labels.iter().map(|l| l.to_string()).collect::<Vec<_>>(),
            &thresholds
                .iter()
                .map(|(l, t)| (l.to_string(), *t))
                .collect(),
        )
    }

    #[test]
    fn policy_spelling_validates_every_rule() {
        assert_eq!(
            policy(
                &["LICENSE_PLATE", "ZIP_CODE"],
                &[("LICENSE_PLATE", 0.5), ("ZIP_CODE", 0.7)]
            )
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
            vec![(NymLabel::LicensePlate, 0.5), (NymLabel::ZipCode, 0.7)]
        );
        assert!(matches!(
            policy(&["PLATE"], &[("PLATE", 0.5)]),
            Err(NymConfigError::UnknownLabel { .. })
        ));
        assert!(matches!(
            policy(&["GIVEN_NAME"], &[("GIVEN_NAME", 0.5)]),
            Err(NymConfigError::UnsupportedLabel { .. })
        ));
        assert!(matches!(
            policy(&["USERNAME"], &[]),
            Err(NymConfigError::MissingThreshold { .. })
        ));
        assert!(matches!(
            policy(&["USERNAME"], &[("USERNAME", 0.5), ("TAX_ID", 0.5)]),
            Err(NymConfigError::ThresholdWithoutLabel { .. })
        ));
        assert!(matches!(
            policy(&["USERNAME", "USERNAME"], &[("USERNAME", 0.5)]),
            Err(NymConfigError::DuplicateLabel { .. })
        ));
        for bad in [0.0, -0.1, 1.01, f32::NAN] {
            assert!(matches!(
                policy(&["USERNAME"], &[("USERNAME", bad)]),
                Err(NymConfigError::ThresholdOutOfRange { .. })
            ));
        }
        assert_eq!(policy(&[], &[]), Err(NymConfigError::EmptyAllowlist));
    }
}
