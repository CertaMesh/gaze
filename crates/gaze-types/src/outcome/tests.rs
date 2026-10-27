use super::*;
use serde::de::DeserializeOwned;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + fmt::Debug>(value: T, json: &str) {
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    assert_eq!(serde_json::from_str::<T>(json).unwrap(), value);
}

#[test]
fn every_outcome_reason_has_stable_value_free_serialization_and_diagnostics() {
    round_trip(ProtectionOutcome::Protected, "\"protected\"");
    assert_eq!(format!("{:?}", ProtectionOutcome::Protected), "Protected");
    assert_eq!(ProtectionOutcome::Protected.to_string(), "protected");
    for (reason, wire, debug) in [
        (NonSensitiveReason::JsonKey, "json_key", "JsonKey"),
        (
            NonSensitiveReason::JsonLiteral,
            "json_literal",
            "JsonLiteral",
        ),
        (
            NonSensitiveReason::Placeholder,
            "placeholder",
            "Placeholder",
        ),
    ] {
        round_trip(reason, &format!("\"{wire}\""));
        assert_eq!(reason.to_string(), wire);
        assert_eq!(format!("{reason:?}"), debug);
        let outcome = ProtectionOutcome::NonSensitive(reason);
        round_trip(outcome, &format!("{{\"non_sensitive\":\"{wire}\"}}"));
        assert_eq!(format!("{outcome:?}"), format!("NonSensitive({debug})"));
        assert_eq!(outcome.to_string(), format!("non_sensitive: {wire}"));
    }
    for (reason, wire, debug) in [
        (
            PossiblySensitiveReason::LabelValueShapeUnclear,
            "label_value_shape_unclear",
            "LabelValueShapeUnclear",
        ),
        (
            PossiblySensitiveReason::LabelBindingUnproven,
            "label_binding_unproven",
            "LabelBindingUnproven",
        ),
        (
            PossiblySensitiveReason::OtherFieldRepeat,
            "other_field_repeat",
            "OtherFieldRepeat",
        ),
    ] {
        round_trip(reason, &format!("\"{wire}\""));
        assert_eq!(reason.to_string(), wire);
        assert_eq!(format!("{reason:?}"), debug);
        let outcome = ProtectionOutcome::PossiblySensitive(reason);
        round_trip(outcome, &format!("{{\"possibly_sensitive\":\"{wire}\"}}"));
        assert_eq!(
            format!("{outcome:?}"),
            format!("PossiblySensitive({debug})")
        );
        assert_eq!(outcome.to_string(), format!("possibly_sensitive: {wire}"));
    }
    for (reason, wire, debug) in [
        (
            UntransformableReason::JsonNumberNoTextOnlyPath,
            "json_number_no_text_only_path",
            "JsonNumberNoTextOnlyPath",
        ),
        (
            UntransformableReason::EncodedCarrier,
            "encoded_carrier",
            "EncodedCarrier",
        ),
        (
            UntransformableReason::NoSafeRendering,
            "no_safe_rendering",
            "NoSafeRendering",
        ),
        (
            UntransformableReason::CallerValueTooShort,
            "caller_value_too_short",
            "CallerValueTooShort",
        ),
    ] {
        round_trip(reason, &format!("\"{wire}\""));
        assert_eq!(reason.to_string(), wire);
        assert_eq!(format!("{reason:?}"), debug);
        let outcome = ProtectionOutcome::ConfirmedUntransformable(reason);
        round_trip(
            outcome,
            &format!("{{\"confirmed_untransformable\":\"{wire}\"}}"),
        );
        assert_eq!(
            format!("{outcome:?}"),
            format!("ConfirmedUntransformable({debug})")
        );
        assert_eq!(
            outcome.to_string(),
            format!("confirmed_untransformable: {wire}")
        );
    }
}

#[test]
fn scalar_types_and_evidence_round_trip() {
    for (kind, wire) in [
        (JsonScalarType::String, "string"),
        (JsonScalarType::Number, "number"),
        (JsonScalarType::Bool, "bool"),
        (JsonScalarType::Null, "null"),
    ] {
        round_trip(kind, &format!("\"{wire}\""));
    }
    round_trip(FieldEvidence::CallerDeclared, "\"caller_declared\"");
    round_trip(FieldEvidence::ValidatedParse, "\"validated_parse\"");
    for (hint, wire) in [
        (HintKind::FenceLang, "fence_lang"),
        (HintKind::FileExt, "file_ext"),
        (HintKind::ContentType, "content_type"),
        (HintKind::ToolName, "tool_name"),
    ] {
        round_trip(hint, &format!("\"{wire}\""));
        round_trip(
            FieldEvidence::Hint(hint),
            &format!("{{\"hint\":\"{wire}\"}}"),
        );
    }
}

#[test]
fn path_preserves_unescaped_segments_but_hides_them_in_diagnostics() {
    let path = FieldPath::new([
        "synthetic-key@example.invalid".into(),
        "a/b~c".into(),
        "0".into(),
    ]);
    assert_eq!(
        path.segments(),
        ["synthetic-key@example.invalid", "a/b~c", "0"]
    );
    assert_eq!(format!("{path:?}"), "FieldPath { segment_count: 3, .. }");
    assert_eq!(path.to_string(), "field path (3 segments)");
    round_trip(
        path.clone(),
        "[\"synthetic-key@example.invalid\",\"a/b~c\",\"0\"]",
    );
    round_trip(FieldPath::default(), "[]");
    let field = FieldProvenance::new(path, JsonScalarType::Number, FieldEvidence::ValidatedParse);
    round_trip(field.clone(), "{\"path\":[\"synthetic-key@example.invalid\",\"a/b~c\",\"0\"],\"json_type\":\"number\",\"evidence\":\"validated_parse\"}");
    let spans = [(0..3, field.clone())];
    for context in [
        FieldContext::WholeInput(&field),
        FieldContext::Spans(&spans),
    ] {
        assert!(!format!("{context:?}").contains("synthetic-key"));
        assert!(!format!("{context:?}").contains("a/b~c"));
    }
}

#[test]
fn unknown_wire_variants_are_rejected_instead_of_becoming_protected() {
    assert!(serde_json::from_str::<ProtectionOutcome>("\"unknown\"").is_err());
    assert!(serde_json::from_str::<ProtectionOutcome>("{\"non_sensitive\":\"unknown\"}").is_err());
    assert!(
        serde_json::from_str::<ProtectionOutcome>("{\"possibly_sensitive\":\"unknown\"}").is_err()
    );
    assert!(serde_json::from_str::<ProtectionOutcome>(
        "{\"confirmed_untransformable\":\"unknown\"}"
    )
    .is_err());
    assert!(serde_json::from_str::<FieldEvidence>("{\"hint\":\"unknown\"}").is_err());
}

#[test]
fn audit_accessor_accepts_only_matching_outcome_reason_pairs() {
    use crate::{Action, ConflictTier, DocumentKind, PiiClass, RedactionEntry};
    use ProtectionOutcomeKind as Kind;
    use ProtectionOutcomeReason as Reason;
    let mut entry = RedactionEntry::new(
        "synthetic",
        PiiClass::Name,
        Action::Tokenize,
        None,
        DocumentKind::Structured,
        false,
        ConflictTier::None,
        0,
        None,
    );
    let kinds = [
        None,
        Some(Kind::Protected),
        Some(Kind::NonSensitive),
        Some(Kind::PossiblySensitive),
        Some(Kind::ConfirmedUntransformable),
    ];
    let reasons = [
        None,
        Some(Reason::NonSensitive(NonSensitiveReason::JsonKey)),
        Some(Reason::PossiblySensitive(
            PossiblySensitiveReason::OtherFieldRepeat,
        )),
        Some(Reason::Untransformable(
            UntransformableReason::EncodedCarrier,
        )),
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        for (j, reason) in reasons.into_iter().enumerate() {
            entry.outcome = kind;
            entry.outcome_reason = reason;
            let result = entry.protection_outcome();
            let valid = [(0, 0), (1, 0), (2, 1), (3, 2), (4, 3)].contains(&(i, j));
            assert_eq!(result.is_ok(), valid, "{kind:?} {reason:?}");
            if let Ok(Some(outcome)) = result {
                let (decoded_kind, decoded_reason) = outcome.audit_parts();
                assert_eq!(Some(decoded_kind), kind);
                assert_eq!(decoded_reason, reason);
            }
        }
    }
}

#[test]
fn field_path_children_borrow_ancestors_and_drop_deep_paths_safely() {
    let parent = FieldPath::new(["k".repeat(1024 * 1024)]);
    let child = parent.child("0".into());
    assert_eq!(parent.segments()[0].as_ptr(), child.segments()[0].as_ptr());
    assert_eq!(child, FieldPath::new(["k".repeat(1024 * 1024), "0".into()]));
    drop(FieldPath::new((0..100_000).map(|_| String::new())));
}
