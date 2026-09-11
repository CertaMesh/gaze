use gaze::RestoreAssessment;

use crate::error::{CliError, RestoreMode, RestoreWarning};

/// Apply CLI exit/warning policy to the core's provenance-aware classification.
pub(crate) fn restore_pass2_validate(
    assessment: &RestoreAssessment,
    mode: RestoreMode,
) -> Result<Vec<RestoreWarning>, CliError> {
    let mut warnings = Vec::new();
    for token in assessment.unknown_tokens() {
        match mode {
            RestoreMode::Strict => {
                return Err(CliError::UnknownToken {
                    token: token.clone(),
                })
            }
            RestoreMode::Tolerant => warnings.push(RestoreWarning {
                variant: "UnknownToken".to_string(),
                token: token.clone(),
            }),
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze::{PiiClass, Scope, Session};

    #[test]
    fn pass2_accepts_dense_authorized_shapes_and_audits_bare_literals() {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let token = session
            .tokenize(&PiiClass::Name, "<deadbeef:Email_999>")
            .unwrap();
        let input = format!("{} Kunde_7", vec![token; 1_000].join(" "));
        let assessment = session.assess_restore_text(&input).unwrap();
        assert!(restore_pass2_validate(&assessment, RestoreMode::Strict)
            .unwrap()
            .is_empty());
        let telemetry = assessment.telemetry(gaze::RestorePolicy::Strict);
        assert_eq!(telemetry.unknown_token_count, 0);
        assert_eq!(telemetry.manifest_bypass_count, 1);
    }

    #[test]
    fn pass2_traps_adjacent_unknown_outside_authorized_output() {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let token = session.tokenize(&PiiClass::Name, "Alice_1").unwrap();
        let assessment = session
            .assess_restore_text(&format!("{token}<deadbeef:Email_999>"))
            .unwrap();
        match restore_pass2_validate(&assessment, RestoreMode::Strict) {
            Err(CliError::UnknownToken { token }) => assert_eq!(token, "<deadbeef:Email_999>"),
            _ => panic!("adjacent unresolved prefix must fail"),
        }
    }

    #[test]
    fn pass2_tolerant_reports_unknown_tokens_in_output_order() {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let assessment = session
            .assess_restore_text("Alice_1 <deadbeef:Email_999> <deadbeef:Name_100>")
            .unwrap();
        let warnings = restore_pass2_validate(&assessment, RestoreMode::Tolerant).unwrap();
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].variant, "UnknownToken");
        assert_eq!(warnings[0].token, "<deadbeef:Email_999>");
        assert_eq!(warnings[1].token, "<deadbeef:Name_100>");
    }
}
