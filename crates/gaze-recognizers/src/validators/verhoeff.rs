use gaze_types::ValidatorKind;

/// Validates an Indian Aadhaar number with the Verhoeff checksum.
pub fn validate_aadhaar(digits: &str) -> bool {
    ValidatorKind::AadhaarVerhoeff.validates(digits)
}
