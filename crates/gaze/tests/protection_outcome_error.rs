use gaze::ProtectionError;
use gaze_types::{FieldPath, UntransformableReason};

#[test]
fn untransformable_error_reports_only_closed_reason_in_diagnostics() {
    for (reason, spelling) in [
        (
            UntransformableReason::JsonNumberNoTextOnlyPath,
            "json_number_no_text_only_path",
        ),
        (UntransformableReason::EncodedCarrier, "encoded_carrier"),
        (UntransformableReason::NoSafeRendering, "no_safe_rendering"),
        (
            UntransformableReason::CallerValueTooShort,
            "caller_value_too_short",
        ),
    ] {
        let error = ProtectionError::Untransformable {
            reason,
            field_path: FieldPath::new(["synthetic-key@example.invalid".into()]),
        };
        assert_eq!(error.as_str(), "Untransformable");
        assert_eq!(
            error.to_string(),
            format!("confirmed sensitive field cannot be transformed: {spelling}")
        );
        assert!(!format!("{error:?}").contains("synthetic-key"));
    }
}
