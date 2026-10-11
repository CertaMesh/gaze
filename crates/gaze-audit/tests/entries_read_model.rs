use gaze_audit::{AuditFilter, SqliteLogger};
use gaze_types::{Action, ConflictTier, DocumentKind, PiiClass, RedactionEntry};

#[test]
fn entries_and_query_retain_every_logged_provenance_field() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let logger = SqliteLogger::new(temp.path()).unwrap();
    let entry = RedactionEntry::new(
        "synthetic",
        PiiClass::Name,
        Action::Tokenize,
        None,
        DocumentKind::Text,
        false,
        ConflictTier::None,
        123,
        None,
    )
    .with_provenance_metadata(
        Some("ner".into()),
        Some("model-a".into()),
        Some("version-b".into()),
        Some("a".repeat(64)),
        Some("b".repeat(64)),
        Some("de-DE".into()),
        Some("exact".into()),
        Some("name".into()),
        Some("PER".into()),
        Some(0.875),
        Some("[\"rule-c\",\"dictionary-d\"]".into()),
    )
    .with_backend_silently_dropped(vec!["private-backend".into()]);
    logger.log(&entry).unwrap();
    let entries = logger.entries().unwrap();
    let rows = SqliteLogger::query(temp.path(), &AuditFilter::default()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(rows.len(), 1);
    macro_rules! retained {
        ($($field:ident),+ $(,)?) => {$(
            assert_eq!(entries[0].$field, entry.$field, stringify!($field));
            assert_eq!(rows[0].$field, entry.$field, stringify!($field));
        )+};
    }
    retained!(
        provenance_stage,
        provenance_model_id,
        provenance_model_version,
        provenance_artifact_sha256,
        provenance_tokenizer_sha256,
        provenance_locale_resolved,
        provenance_locale_match_kind,
        provenance_canonical_class,
        provenance_native_class,
        provenance_merged_from
    );
    assert_eq!(entries[0].provenance_confidence.as_deref(), Some("0.875"));
    assert_eq!(rows[0].provenance_confidence, Some(0.875));
    assert_eq!(
        entries[0].backend_silently_dropped,
        entry.backend_silently_dropped
    );
}

fn log_basic(logger: &SqliteLogger) {
    logger
        .log(&RedactionEntry::new(
            "synthetic",
            PiiClass::Name,
            Action::Tokenize,
            None,
            DocumentKind::Text,
            false,
            ConflictTier::None,
            123,
            None,
        ))
        .unwrap();
}

#[test]
fn raw_rows_preserve_unknown_enums_and_json_while_entries_reject_them() {
    for column in [
        "class",
        "action",
        "document_kind",
        "decided_by",
        "fallback_triggered",
        "validator_fail_reason",
        "ambiguity_record",
        "labelled_value_scan_reason",
    ] {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let logger = SqliteLogger::new(temp.path()).unwrap();
        log_basic(&logger);
        rusqlite::Connection::open(temp.path())
            .unwrap()
            .execute(
                &format!("UPDATE redaction_log SET {column} = ?1"),
                ["future-or-malformed"],
            )
            .unwrap();
        let rows = SqliteLogger::query(temp.path(), &AuditFilter::default()).unwrap();
        let raw = &rows[0];
        let value = match column {
            "class" => Some(raw.class.as_str()),
            "action" => Some(raw.action.as_str()),
            "document_kind" => Some(raw.document_kind.as_str()),
            "decided_by" => Some(raw.decided_by.as_str()),
            "fallback_triggered" => raw.fallback_triggered.as_deref(),
            "validator_fail_reason" => raw.validator_fail_reason.as_deref(),
            "ambiguity_record" => raw.ambiguity_record.as_deref(),
            "labelled_value_scan_reason" => raw.labelled_value_scan_reason.as_deref(),
            _ => unreachable!(),
        };
        assert_eq!(value, Some("future-or-malformed"), "{column}");
        let error = logger.entries().unwrap_err().to_string();
        let index = gaze_audit::AUDIT_RESTRICTED_COLUMNS
            .iter()
            .position(|c| *c == column)
            .unwrap();
        assert!(
            error.contains(&format!("at index: {index},")),
            "{column}: {error}"
        );
    }
}

#[test]
fn signed_raw_counters_require_checked_typed_ranges() {
    for column in [
        "restore_unknown_token_count",
        "restore_manifest_bypass_count",
        "restore_fresh_pii_count",
        "restore_trap_shape_count",
        "restore_phase_mask",
    ] {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let logger = SqliteLogger::new(temp.path()).unwrap();
        log_basic(&logger);
        let conn = rusqlite::Connection::open(temp.path()).unwrap();
        let sql = format!("UPDATE redaction_log SET {column} = ?1");
        for invalid in if column == "restore_phase_mask" {
            vec![-1, i64::from(u32::MAX) + 1]
        } else {
            vec![-1]
        } {
            conn.execute(&sql, [invalid]).unwrap();
            let rows = SqliteLogger::query(temp.path(), &AuditFilter::default()).unwrap();
            let raw = &rows[0];
            let actual = match column {
                "restore_unknown_token_count" => raw.restore_unknown_token_count,
                "restore_manifest_bypass_count" => raw.restore_manifest_bypass_count,
                "restore_fresh_pii_count" => raw.restore_fresh_pii_count,
                "restore_trap_shape_count" => raw.restore_trap_shape_count,
                "restore_phase_mask" => raw.restore_phase_mask,
                _ => unreachable!(),
            };
            assert_eq!(actual, Some(invalid));
            assert!(logger.entries().is_err(), "{column}: {invalid}");
        }
        let maximum = if column == "restore_phase_mask" {
            i64::from(u32::MAX)
        } else {
            i64::MAX
        };
        for valid in [0, maximum] {
            conn.execute(&sql, [valid]).unwrap();
            let entry = logger.entries().unwrap().remove(0);
            let actual = match column {
                "restore_unknown_token_count" => entry.restore_unknown_token_count,
                "restore_manifest_bypass_count" => entry.restore_manifest_bypass_count,
                "restore_fresh_pii_count" => entry.restore_fresh_pii_count,
                "restore_trap_shape_count" => entry.restore_trap_shape_count,
                "restore_phase_mask" => entry.restore_phase_mask.map(u64::from),
                _ => unreachable!(),
            };
            assert_eq!(actual, Some(u64::try_from(valid).unwrap()));
        }
    }
}

#[test]
fn sparse_legacy_rows_keep_nulls_and_entries_default_timestamp_only() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    rusqlite::Connection::open(temp.path())
        .unwrap()
        .execute_batch(
            "CREATE TABLE redaction_log (source TEXT, class TEXT, action TEXT, field_name TEXT,
        document_kind TEXT, conflict_loser INTEGER);
        INSERT INTO redaction_log VALUES ('legacy', 'name', 'tokenize', NULL, 'text', 0);",
        )
        .unwrap();
    let before = SqliteLogger::query(temp.path(), &AuditFilter::default())
        .unwrap()
        .remove(0);
    assert_eq!(before.created_at, None);
    assert_eq!(before.provenance_stage, None);
    assert_eq!(before.provenance_confidence, None);
    assert_eq!(before.restore_unknown_token_count, None);
    let logger = SqliteLogger::new(temp.path()).unwrap();
    let raw = SqliteLogger::query(temp.path(), &AuditFilter::default())
        .unwrap()
        .remove(0);
    assert_eq!(raw.created_at, None);
    let entry = logger.entries().unwrap().remove(0);
    assert_eq!(entry.created_at, 0);
    assert_eq!(entry.provenance_stage, None);
    assert_eq!(entry.provenance_merged_from, None);
    assert_eq!(entry.backend_silently_dropped, None);
    assert_eq!(entry.restore_phase_mask, None);
}

#[test]
fn malformed_private_sidecar_is_rejected_only_by_typed_entries() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let logger = SqliteLogger::new(temp.path()).unwrap();
    log_basic(&logger);
    rusqlite::Connection::open(temp.path())
        .unwrap()
        .execute(
            "UPDATE redaction_log SET backend_silently_dropped = 'not-json'",
            [],
        )
        .unwrap();
    assert_eq!(
        SqliteLogger::query(temp.path(), &AuditFilter::default())
            .unwrap()
            .len(),
        1
    );
    assert!(logger.entries().is_err());
    assert!(!gaze_audit::AUDIT_RESTRICTED_COLUMNS.contains(&"backend_silently_dropped"));
}
