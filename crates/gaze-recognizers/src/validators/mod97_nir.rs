use gaze_types::ValidatorKind;

/// Validates a French NIR using its two-digit MOD-97 key.
pub fn validate_fr_nir(digits: &str) -> bool {
    ValidatorKind::FrNirMod97.validates(digits)
}
