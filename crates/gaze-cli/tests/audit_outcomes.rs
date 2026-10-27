use assert_cmd::Command;
use gaze_audit::SqliteLogger;
use gaze_types::{
    Action, ConflictTier, DocumentKind, FieldEvidence, FieldPath, FieldProvenance, JsonScalarType,
    PiiClass, PossiblySensitiveReason, ProtectionOutcome, RedactionEntry,
};

#[test]
fn audit_query_and_export_filter_outcome_and_reason_without_disclosing_keys() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let logger = SqliteLogger::new(file.path()).unwrap();
    for (source, outcome) in [
        ("confirmed", ProtectionOutcome::Protected),
        (
            "unclear",
            ProtectionOutcome::PossiblySensitive(PossiblySensitiveReason::LabelValueShapeUnclear),
        ),
        (
            "repeat",
            ProtectionOutcome::PossiblySensitive(PossiblySensitiveReason::OtherFieldRepeat),
        ),
    ] {
        logger
            .log(
                &RedactionEntry::new(
                    source,
                    PiiClass::Name,
                    Action::Tokenize,
                    None,
                    DocumentKind::Structured,
                    false,
                    ConflictTier::None,
                    123,
                    None,
                )
                .with_protection_outcome(outcome)
                .with_field_provenance(&FieldProvenance::new(
                    FieldPath::new(["synthetic-key@example.invalid".into()]),
                    JsonScalarType::String,
                    FieldEvidence::ValidatedParse,
                )),
            )
            .unwrap();
    }
    for (flags, expected) in [
        (vec!["--outcome", "protected"], vec!["confirmed"]),
        (
            vec!["--outcome", "possibly-sensitive"],
            vec!["unclear", "repeat"],
        ),
        (
            vec!["--outcome-reason", "label-value-shape-unclear"],
            vec!["unclear"],
        ),
        (
            vec![
                "--outcome",
                "possibly_sensitive",
                "--outcome-reason",
                "other_field_repeat",
            ],
            vec!["repeat"],
        ),
        (
            vec![
                "--outcome",
                "protected",
                "--outcome-reason",
                "other_field_repeat",
            ],
            vec![],
        ),
    ] {
        for command in ["query", "export"] {
            let output = Command::cargo_bin("gaze")
                .unwrap()
                .args([
                    "audit",
                    command,
                    "--audit-db",
                    file.path().to_str().unwrap(),
                ])
                .args(&flags)
                .output()
                .unwrap();
            assert!(output.status.success(), "{command} {flags:?} failed");
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                !text.contains("synthetic-key"),
                "field key leaked into audit output"
            );
            let sources: Vec<String> = if command == "export" {
                text.lines()
                    .map(|line| {
                        let row: serde_json::Value = serde_json::from_str(line).unwrap();
                        assert_eq!(row["field_path_depth"], 1);
                        assert_eq!(row["field_evidence"], "validated_parse");
                        row["source"].as_str().unwrap().to_owned()
                    })
                    .collect()
            } else {
                let mut lines = text.lines();
                let header: Vec<_> = lines.next().unwrap().split('\t').collect();
                assert_eq!(header, gaze_audit::AUDIT_RESTRICTED_COLUMNS);
                lines
                    .map(|line| {
                        let fields: Vec<_> = line.split('\t').collect();
                        assert_eq!(fields.len(), header.len());
                        fields[0].to_owned()
                    })
                    .collect()
            };
            assert_eq!(sources, expected, "{command} {flags:?}");
        }
    }
}

#[test]
fn audit_outcome_filters_on_unmigrated_database_return_no_rows() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "CREATE TABLE redaction_log (source TEXT, class TEXT, action TEXT,
        field_name TEXT, document_kind TEXT, conflict_loser INTEGER);
        INSERT INTO redaction_log VALUES ('legacy', 'name', 'tokenize', NULL, 'text', 0);",
    )
    .unwrap();
    for (flag, value) in [
        ("--outcome", "protected"),
        ("--outcome-reason", "other_field_repeat"),
    ] {
        let output = Command::cargo_bin("gaze")
            .unwrap()
            .args([
                "audit",
                "query",
                "--audit-db",
                file.path().to_str().unwrap(),
                flag,
                value,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);
    }
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('redaction_log')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 6, "read-only query must not migrate the schema");
}

#[test]
fn audit_outcome_filters_reject_unknown_and_wrong_family_values() {
    for command in ["query", "export"] {
        for (flag, value, expected) in [
            ("--outcome", "possibly-sensitve", "protected"),
            ("--outcome", "other_field_repeat", "possibly_sensitive"),
            ("--outcome", "protected' OR 1=1 --", "non_sensitive"),
            ("--outcome-reason", "future_reason", "other_field_repeat"),
            ("--outcome-reason", "protected", "json_key"),
        ] {
            let output = Command::cargo_bin("gaze")
                .unwrap()
                .args(["audit", command, "--audit-db", "unused.sqlite", flag, value])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2), "{command} {flag} {value}");
            assert!(output.stdout.is_empty());
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains("possible values"), "{error}");
            assert!(error.contains(expected), "{error}");
            if matches!(
                value,
                "possibly-sensitve" | "protected' OR 1=1 --" | "future_reason"
            ) {
                assert!(!error.contains(value), "rejected value leaked into stderr");
            }
        }
    }
}
