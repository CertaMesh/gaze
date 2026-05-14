use gaze_types::ValidatorKind;

/// Validates a German Steuer-ID with ISO 7064 MOD 11,10.
pub fn validate_de_steuer_id(digits: &str) -> bool {
    ValidatorKind::DeSteuerIdMod1110.validates(digits)
}

/// Validates a Dutch BSN with the 11-test.
pub fn validate_bsn(digits: &str) -> bool {
    ValidatorKind::BsnMod11.validates(digits)
}

/// Validates a Brazilian CPF with its two MOD-11 check digits.
pub fn validate_cpf(digits: &str) -> bool {
    ValidatorKind::CpfMod11.validates(digits)
}

/// Validates a Brazilian CNPJ with its two MOD-11 check digits.
pub fn validate_cnpj(digits: &str) -> bool {
    ValidatorKind::CnpjMod11.validates(digits)
}

/// Validates a UK NHS number with the MOD-11 checksum.
pub fn validate_uk_nhs(digits: &str) -> bool {
    ValidatorKind::UkNhsMod11.validates(digits)
}
