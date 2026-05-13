use std::path::Path;

use gaze_audit::{AuditFilter, SqliteLogger};
use gaze_types::{
    Action, AmbiguityReason, AmbiguityRecord, ConflictTier, DocumentKind, LosingCandidate,
    PiiClass, RedactionEntry, ValidatorFailReason,
};
use rusqlite::Connection;

#[test]
fn spike4_migration_adds_audit_metadata_columns_and_is_idempotent() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    create_pre_spike4_redaction_log(temp.path());

    let logger = SqliteLogger::new(temp.path()).expect("first migration");
    assert_spike4_columns(temp.path());

    let rows = logger.entries().expect("legacy entries");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].recognizer_id.as_deref(), Some("legacy_unversioned"));
    assert_eq!(rows[0].recognizer_version_id, None);
    assert_eq!(rows[0].validator_fail_reason, None);
    assert_eq!(rows[0].ambiguity_record, None);

    drop(logger);
    let logger = SqliteLogger::new(temp.path()).expect("second migration");
    assert_spike4_columns(temp.path());

    let record = AmbiguityRecord::new(
        PiiClass::Custom("postal_or_phone_de".to_string()),
        vec![LosingCandidate::new(
            PiiClass::Custom("postal_de".to_string()),
            "postal-de",
        )],
        AmbiguityReason::NoAnchor,
    );
    let entry = RedactionEntry::new(
        "regex",
        PiiClass::Email,
        Action::Tokenize,
        None,
        DocumentKind::Text,
        false,
        ConflictTier::None,
        200,
        Some("session-b".to_string()),
    )
    .with_validator_fail_reason(ValidatorFailReason::EmailRfcRejected)
    .with_ambiguity_record(record.clone());
    logger.log(&entry).expect("log new row");

    let entries = logger.entries().expect("entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[1].validator_fail_reason,
        Some(ValidatorFailReason::EmailRfcRejected)
    );
    assert_eq!(entries[1].ambiguity_record, Some(record));
}

#[test]
fn recognizer_lineage_round_trips_through_redaction_log_rows() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let logger = SqliteLogger::new(temp.path()).expect("fresh schema");
    let entry = RedactionEntry::new(
        "ner/ort",
        PiiClass::Name,
        Action::Tokenize,
        None,
        DocumentKind::Text,
        false,
        ConflictTier::None,
        300,
        Some("session-c".to_string()),
    )
    .with_recognizer_metadata(
        Some("ner".to_string()),
        Some("ner.davlan-mbert.v1".to_string()),
    );

    logger.log(&entry).expect("log entry");

    let entries = logger.entries().expect("entries");
    assert_eq!(entries[0].recognizer_id.as_deref(), Some("ner"));
    assert_eq!(
        entries[0].recognizer_version_id.as_deref(),
        Some("ner.davlan-mbert.v1")
    );

    let rows = SqliteLogger::query(temp.path(), &AuditFilter::default()).expect("query rows");
    assert_eq!(rows[0].recognizer_id.as_deref(), Some("ner"));
    assert_eq!(
        rows[0].recognizer_version_id.as_deref(),
        Some("ner.davlan-mbert.v1")
    );
}

#[test]
fn spike4_query_filters_ambiguity_reason_and_collision_metadata() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let _logger = SqliteLogger::new(temp.path()).expect("fresh schema");
    let conn = Connection::open(temp.path()).expect("open db");
    conn.execute(
        r#"INSERT INTO redaction_log
            (source, class, action, field_name, document_kind, conflict_loser, decided_by, created_at,
             session_id, validator_fail_reason, ambiguity_record, collision_family, collision_variant)
         VALUES
            ('regex', 'email', 'tokenize', NULL, 'text', 0, 'none', 100, NULL, NULL, NULL, NULL, NULL),
            ('hybrid', 'custom:postal_or_phone_de', 'tokenize', NULL, 'text', 0, 'validator', 200,
             NULL, '"luhn_failed"',
             '{"ambiguity_class":"custom:postal_or_phone_de","losing_candidates":[{"class":"custom:postal_de","recognizer_id":"postal-de"}],"reason":"no_anchor"}',
             'de-postal-phone', 'postal-de')"#,
        [],
    )
    .expect("insert rows");

    let rows = SqliteLogger::query(
        temp.path(),
        &AuditFilter {
            has_ambiguity: Some(true),
            ambiguity_reason: Some("no_anchor".to_string()),
            collision_family: Some("de-postal-phone".to_string()),
            collision_variant: Some("postal-de".to_string()),
            ..AuditFilter::default()
        },
    )
    .expect("filtered query");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, "hybrid");
    assert!(rows[0].ambiguity_record.is_some());
    assert_eq!(rows[0].collision_family.as_deref(), Some("de-postal-phone"));
    assert_eq!(rows[0].collision_variant.as_deref(), Some("postal-de"));
}

fn create_pre_spike4_redaction_log(path: &Path) {
    let conn = Connection::open(path).expect("legacy sqlite");
    conn.execute_batch(
        r#"
        CREATE TABLE redaction_log (
            source TEXT NOT NULL,
            class TEXT NOT NULL,
            action TEXT NOT NULL,
            field_name TEXT NULL,
            document_kind TEXT NOT NULL,
            conflict_loser INTEGER NOT NULL,
            decided_by TEXT NOT NULL DEFAULT 'none',
            created_at INTEGER NULL,
            session_id TEXT NULL,
            snapshot_scheme TEXT NOT NULL DEFAULT 'gaze.snapshot.v1.sha256-salted',
            snapshot_alg TEXT NOT NULL DEFAULT 'SHA-256',
            snapshot_key_version INTEGER NULL
        );
        INSERT INTO redaction_log
            (source, class, action, field_name, document_kind, conflict_loser, decided_by,
             created_at, session_id, snapshot_scheme, snapshot_alg, snapshot_key_version)
        VALUES
            ('legacy', 'name', 'tokenize', NULL, 'text', 0, 'none', 100, NULL,
             'gaze.snapshot.v1.sha256-salted', 'SHA-256', NULL);
        "#,
    )
    .expect("legacy schema and row");
}

fn assert_spike4_columns(path: &Path) {
    let conn = Connection::open(path).expect("open db");
    let mut stmt = conn
        .prepare("PRAGMA table_info(redaction_log)")
        .expect("table info");
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect columns");

    for column in [
        "validator_fail_reason",
        "ambiguity_record",
        "collision_family",
        "collision_variant",
        "recognizer_id",
        "recognizer_version_id",
    ] {
        assert!(columns.iter().any(|actual| actual == column), "{column}");
    }
}
