use gaze_audit::{AuditFilter, SqliteLogger, AUDIT_RESTRICTED_COLUMNS};
use gaze_types::{
    Action, ConflictTier, DocumentKind, FieldEvidence, FieldPath, FieldProvenance, HintKind,
    JsonScalarType, NonSensitiveReason, PiiClass, PossiblySensitiveReason, ProtectionOutcome,
    RedactionEntry, UntransformableReason,
};
use rusqlite::Connection;

fn entry() -> RedactionEntry {
    RedactionEntry::new(
        "synthetic",
        PiiClass::Name,
        Action::Tokenize,
        None,
        DocumentKind::Structured,
        false,
        ConflictTier::None,
        123,
        None,
    )
}

fn legacy_db(path: &std::path::Path) {
    Connection::open(path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE redaction_log (source TEXT NOT NULL, class TEXT NOT NULL,
        action TEXT NOT NULL, field_name TEXT, document_kind TEXT NOT NULL,
        conflict_loser INTEGER NOT NULL);
        INSERT INTO redaction_log VALUES ('legacy', 'name', 'tokenize', NULL, 'text', 0);",
        )
        .unwrap();
}

#[test]
fn outcome_migration_preserves_legacy_rows_and_is_idempotent() {
    let file = tempfile::NamedTempFile::new().unwrap();
    legacy_db(file.path());
    // Read-only queries work before migration and do not label legacy rows protected.
    for migrated in [false, true, true] {
        if migrated {
            SqliteLogger::new(file.path()).unwrap();
        }
        let rows = SqliteLogger::query(file.path(), &AuditFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "legacy");
        assert_eq!(rows[0].outcome, None);
        assert_eq!(rows[0].outcome_reason, None);
        assert_eq!(rows[0].field_path_depth, None);
        assert_eq!(rows[0].field_evidence, None);
        for filter in [
            AuditFilter {
                outcome: Some("protected".into()),
                ..Default::default()
            },
            AuditFilter {
                outcome_reason: Some("label_value_shape_unclear".into()),
                ..Default::default()
            },
        ] {
            assert!(SqliteLogger::query(file.path(), &filter)
                .unwrap()
                .is_empty());
        }
        if migrated {
            let conn = Connection::open(file.path()).unwrap();
            let mut stmt = conn.prepare("PRAGMA table_info(redaction_log)").unwrap();
            let columns = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            for column in [
                "outcome",
                "outcome_reason",
                "field_path_depth",
                "field_evidence",
            ] {
                assert!(
                    columns.iter().any(|name| name == column),
                    "missing {column}"
                );
            }
        }
    }
    let logger = SqliteLogger::new(file.path()).unwrap();
    logger
        .log(&entry().with_protection_outcome(ProtectionOutcome::Protected))
        .unwrap();
    assert_eq!(logger.entries().unwrap().len(), 2);
}

#[test]
fn outcomes_round_trip_without_keys_or_scalar_values() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let logger = SqliteLogger::new(file.path()).unwrap();
    let outcomes = [
        ProtectionOutcome::Protected,
        ProtectionOutcome::NonSensitive(NonSensitiveReason::JsonKey),
        ProtectionOutcome::NonSensitive(NonSensitiveReason::JsonLiteral),
        ProtectionOutcome::NonSensitive(NonSensitiveReason::Placeholder),
        ProtectionOutcome::PossiblySensitive(PossiblySensitiveReason::LabelValueShapeUnclear),
        ProtectionOutcome::PossiblySensitive(PossiblySensitiveReason::LabelBindingUnproven),
        ProtectionOutcome::PossiblySensitive(PossiblySensitiveReason::OtherFieldRepeat),
        ProtectionOutcome::ConfirmedUntransformable(
            UntransformableReason::JsonNumberNoTextOnlyPath,
        ),
        ProtectionOutcome::ConfirmedUntransformable(UntransformableReason::EncodedCarrier),
        ProtectionOutcome::ConfirmedUntransformable(UntransformableReason::NoSafeRendering),
        ProtectionOutcome::ConfirmedUntransformable(UntransformableReason::CallerValueTooShort),
    ];
    let evidence = [
        FieldEvidence::CallerDeclared,
        FieldEvidence::ValidatedParse,
        FieldEvidence::Hint(HintKind::FenceLang),
        FieldEvidence::Hint(HintKind::FileExt),
        FieldEvidence::Hint(HintKind::ContentType),
        FieldEvidence::Hint(HintKind::ToolName),
    ];
    let mut expected = Vec::new();
    for (i, outcome) in outcomes.into_iter().enumerate() {
        let provenance = FieldProvenance::new(
            FieldPath::new(["synthetic-key@example.invalid".into(), "0".into()]),
            JsonScalarType::String,
            evidence[i % evidence.len()],
        );
        let row = entry()
            .with_protection_outcome(outcome)
            .with_field_provenance(&provenance);
        let json = serde_json::to_string(&row).unwrap();
        assert!(!json.contains("synthetic-key"));
        assert!(!format!("{row:?}").contains("synthetic-key"));
        logger.log(&row).unwrap();
        expected.push(row);
    }
    assert_eq!(logger.entries().unwrap(), expected);
    let rows = SqliteLogger::query(file.path(), &AuditFilter::default()).unwrap();
    assert_eq!(rows.len(), expected.len());
    assert_eq!(rows[0].outcome.as_deref(), Some("protected"));
    assert_eq!(rows[0].outcome_reason, None);
    assert_eq!(rows[4].outcome.as_deref(), Some("possibly_sensitive"));
    assert_eq!(
        rows[4].outcome_reason.as_deref(),
        Some("label_value_shape_unclear")
    );
    for row in rows {
        assert_eq!(row.field_path_depth, Some(2));
    }
    assert!(
        !String::from_utf8_lossy(&std::fs::read(file.path()).unwrap()).contains("synthetic-key")
    );
    assert!(!AUDIT_RESTRICTED_COLUMNS.contains(&"field_path"));
    assert!(AUDIT_RESTRICTED_COLUMNS.contains(&"field_path_depth"));
    assert!(!serde_json::to_string(&entry()).unwrap().contains("outcome"));
}

#[test]
fn outcome_filters_are_bound_and_distinguish_possible_from_confirmed() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let logger = SqliteLogger::new(file.path()).unwrap();
    for outcome in [
        ProtectionOutcome::Protected,
        ProtectionOutcome::PossiblySensitive(PossiblySensitiveReason::LabelValueShapeUnclear),
        ProtectionOutcome::PossiblySensitive(PossiblySensitiveReason::OtherFieldRepeat),
    ] {
        logger
            .log(&entry().with_protection_outcome(outcome))
            .unwrap();
    }
    for (outcome, reason, count) in [
        (Some("protected"), None, 1),
        (Some("possibly_sensitive"), None, 2),
        (None, Some("other_field_repeat"), 1),
        (Some("protected"), Some("other_field_repeat"), 0),
        (Some("protected' OR 1=1 --"), None, 0),
    ] {
        let rows = SqliteLogger::query(
            file.path(),
            &AuditFilter {
                outcome: outcome.map(str::to_owned),
                outcome_reason: reason.map(str::to_owned),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), count);
    }
}

#[test]
fn typed_entries_reject_unknown_outcomes_and_invalid_depth() {
    for (column, value) in [
        ("outcome", "'future_outcome'"),
        ("outcome_reason", "'future_reason'"),
        ("field_evidence", "'\"future_evidence\"'"),
        ("field_path_depth", "-1"),
        ("field_path_depth", "4294967296"),
    ] {
        let file = tempfile::NamedTempFile::new().unwrap();
        let logger = SqliteLogger::new(file.path()).unwrap();
        logger.log(&entry()).unwrap();
        Connection::open(file.path())
            .unwrap()
            .execute(&format!("UPDATE redaction_log SET {column} = {value}"), [])
            .unwrap();
        assert!(
            logger.entries().is_err(),
            "{column} must fail typed decoding"
        );
        assert_eq!(
            SqliteLogger::query(file.path(), &AuditFilter::default())
                .unwrap()
                .len(),
            1
        );
    }
}
