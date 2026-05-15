//! DistilBERT NER label vocabulary for the Kiji safety-net backend.
//!
//! Kiji wraps a DistilBERT NER head whose label set is the standard CoNLL-style
//! `PER` / `LOC` / `ORG` / `MISC` tags. The reference subprocess emits bare
//! upstream entity labels after BIO decoding; older wrappers may emit the
//! lower-case Gaze label ids from `labels.json`.

use gaze_types::{PiiClass, SafetyNetError, SafetyNetPiiClass};

const MAX_KIJI_LABEL_LEN: usize = 16;

/// Closed Kiji label set, mirroring the closed `OpenAiPrivateLabel` shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KijiLabel {
    Person,
    Location,
    Organization,
    Miscellaneous,
}

impl KijiLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Location => "location",
            Self::Organization => "organization",
            Self::Miscellaneous => "miscellaneous",
        }
    }
}

/// Validates Kiji label shape before any class mapping.
pub fn validate_kiji_label(label: &str) -> Result<(), SafetyNetError> {
    if label.is_empty() || label.len() > MAX_KIJI_LABEL_LEN {
        return Err(invalid_label());
    }

    if label.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_uppercase() || byte == b'_' || byte == b'-'
    }) {
        Ok(())
    } else {
        Err(invalid_label())
    }
}

/// Maps the four CoNLL-style Kiji labels into the closed Gaze NER vocabulary.
pub fn map_kiji_label(label: &str) -> Result<KijiLabel, SafetyNetError> {
    validate_kiji_label(label)?;
    match label {
        "person" | "PER" | "B-PER" | "I-PER" => Ok(KijiLabel::Person),
        "location" | "LOC" | "B-LOC" | "I-LOC" => Ok(KijiLabel::Location),
        "organization" | "ORG" | "B-ORG" | "I-ORG" => Ok(KijiLabel::Organization),
        "miscellaneous" | "MISC" | "B-MISC" | "I-MISC" => Ok(KijiLabel::Miscellaneous),
        _ => Err(SafetyNetError::InvalidOutput {
            message: "kiji returned unsupported label".to_string(),
        }),
    }
}

/// Maps a validated Kiji label into the closed safety-net PII vocabulary.
///
/// `Organization` and `Miscellaneous` map to `SafetyNetPiiClass::Name` so the
/// downstream manifest-diff stage classifies them as candidate Name leaks; the
/// safety-net contract does not (yet) distinguish org/misc spans from person
/// spans at the manifest layer.
pub fn kiji_label_to_safety_net_class(label: KijiLabel) -> SafetyNetPiiClass {
    match label {
        KijiLabel::Person => SafetyNetPiiClass::Name,
        KijiLabel::Location => SafetyNetPiiClass::Location,
        KijiLabel::Organization | KijiLabel::Miscellaneous => SafetyNetPiiClass::Name,
    }
}

/// Convenience: validated label -> Gaze `PiiClass`.
pub fn kiji_label_to_pii_class(label: KijiLabel) -> PiiClass {
    kiji_label_to_safety_net_class(label).to_pii_class()
}

fn invalid_label() -> SafetyNetError {
    SafetyNetError::InvalidOutput {
        message: "kiji returned invalid label".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_labels_map_to_closed_classes() {
        let cases = [
            ("person", PiiClass::Name),
            ("PER", PiiClass::Name),
            ("B-PER", PiiClass::Name),
            ("location", PiiClass::Location),
            ("LOC", PiiClass::Location),
            ("organization", PiiClass::Name),
            ("ORG", PiiClass::Name),
            ("miscellaneous", PiiClass::Name),
            ("MISC", PiiClass::Name),
        ];

        for (label, expected) in cases {
            let kiji_label = map_kiji_label(label).expect("official label");
            assert_eq!(kiji_label_to_pii_class(kiji_label), expected);
        }
    }

    #[test]
    fn unknown_and_invalid_labels_fail_closed() {
        assert!(map_kiji_label("PERSON").is_err());
        assert!(map_kiji_label("per-son").is_err());
        assert!(map_kiji_label("date").is_err());
        assert!(map_kiji_label("").is_err());
        assert!(map_kiji_label("organization_12345").is_err());
    }
}
