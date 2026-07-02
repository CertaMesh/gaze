use gaze_types::{OpenAiPrivateLabel, SafetyNetError, SafetyNetPiiClass};

const MAX_OPENAI_LABEL_LEN: usize = 32;

/// Validates OPF label shape before any class mapping.
///
/// The shape is intentionally narrower than `PiiClass::custom`: OPF labels are
/// an upstream wire contract, not adopter-defined policy names.
pub fn validate_openai_label(label: &str) -> Result<(), SafetyNetError> {
    if label.is_empty() || label.len() > MAX_OPENAI_LABEL_LEN {
        return Err(invalid_label());
    }

    if label
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    {
        Ok(())
    } else {
        Err(invalid_label())
    }
}

/// Maps the official eight OPF labels into the closed Gaze safety-net class set.
pub fn map_openai_label(label: &str) -> Result<OpenAiPrivateLabel, SafetyNetError> {
    validate_openai_label(label)?;

    match label {
        "private_person" => Ok(OpenAiPrivateLabel::PrivatePerson),
        "private_address" => Ok(OpenAiPrivateLabel::PrivateAddress),
        "private_email" => Ok(OpenAiPrivateLabel::PrivateEmail),
        "private_phone" => Ok(OpenAiPrivateLabel::PrivatePhone),
        "private_url" => Ok(OpenAiPrivateLabel::PrivateUrl),
        "private_date" => Ok(OpenAiPrivateLabel::PrivateDate),
        "account_number" => Ok(OpenAiPrivateLabel::AccountNumber),
        "secret" => Ok(OpenAiPrivateLabel::Secret),
        _ => Err(SafetyNetError::InvalidOutput {
            message: "opf returned unsupported label".to_string(),
        }),
    }
}

/// Maps an official OPF label into the closed safety-net PII vocabulary.
pub fn openai_label_to_safety_net_class(
    label: OpenAiPrivateLabel,
) -> Result<SafetyNetPiiClass, SafetyNetError> {
    Ok(match label {
        OpenAiPrivateLabel::PrivatePerson => SafetyNetPiiClass::Name,
        OpenAiPrivateLabel::PrivateAddress => SafetyNetPiiClass::Location,
        OpenAiPrivateLabel::PrivateEmail => SafetyNetPiiClass::Email,
        OpenAiPrivateLabel::PrivatePhone => SafetyNetPiiClass::Phone,
        OpenAiPrivateLabel::PrivateUrl => SafetyNetPiiClass::Url,
        OpenAiPrivateLabel::PrivateDate => SafetyNetPiiClass::Date,
        OpenAiPrivateLabel::AccountNumber => SafetyNetPiiClass::AccountNumber,
        OpenAiPrivateLabel::Secret => SafetyNetPiiClass::Secret,
        _ => {
            return Err(SafetyNetError::InvalidOutput {
                message: "opf returned unsupported label".to_string(),
            })
        }
    })
}

fn invalid_label() -> SafetyNetError {
    SafetyNetError::InvalidOutput {
        message: "opf returned invalid label".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use gaze_types::PiiClass;

    use super::*;

    #[test]
    fn official_labels_map_to_closed_classes() {
        let cases = [
            ("private_person", PiiClass::Name),
            ("private_address", PiiClass::Location),
            ("private_email", PiiClass::Email),
            ("private_phone", PiiClass::custom("phone")),
            ("private_url", PiiClass::custom("url")),
            ("private_date", PiiClass::custom("date")),
            ("account_number", PiiClass::custom("account_number")),
            ("secret", PiiClass::custom("secret")),
        ];

        for (label, expected) in cases {
            let private_label = map_openai_label(label).expect("official label");
            assert_eq!(private_label.as_str(), label);
            assert_eq!(
                openai_label_to_safety_net_class(private_label)
                    .expect("official label maps to safety-net class")
                    .to_pii_class(),
                expected
            );
        }
    }

    #[test]
    fn unknown_and_invalid_labels_fail_closed() {
        for label in [
            "organization",
            "private-email",
            "PRIVATE_EMAIL",
            "private_email_123",
            "",
            "abcdefghijklmnopqrstuvwxyzabcdefg",
        ] {
            assert!(matches!(
                map_openai_label(label),
                Err(SafetyNetError::InvalidOutput { .. })
            ));
        }
    }
}
