use std::path::Path;

use gaze_audit::{build_audit_query_sql, AuditFilter, SqliteLogger};
use rusqlite::types::Value as SqlValue;
use rusqlite::Connection;

const PROVENANCE_COLUMNS: &[&str] = &[
    "provenance_stage",
    "provenance_model_id",
    "provenance_model_version",
    "provenance_artifact_sha256",
    "provenance_tokenizer_sha256",
    "provenance_locale_resolved",
    "provenance_locale_match_kind",
    "provenance_canonical_class",
    "provenance_native_class",
    "provenance_confidence",
    "provenance_merged_from",
];

#[test]
fn ner_provenance_migration_reads_legacy_rows_as_null_and_is_idempotent() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    create_pre_provenance_redaction_log(temp.path());

    let _logger = SqliteLogger::new(temp.path()).expect("first migration");
    assert_provenance_columns(temp.path());

    let rows = SqliteLogger::query(temp.path(), &AuditFilter::default()).expect("query rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, "legacy");
    assert_eq!(rows[0].provenance_stage, None);
    assert_eq!(rows[0].provenance_model_id, None);
    assert_eq!(rows[0].provenance_model_version, None);
    assert_eq!(rows[0].provenance_artifact_sha256, None);
    assert_eq!(rows[0].provenance_tokenizer_sha256, None);
    assert_eq!(rows[0].provenance_locale_resolved, None);
    assert_eq!(rows[0].provenance_locale_match_kind, None);
    assert_eq!(rows[0].provenance_canonical_class, None);
    assert_eq!(rows[0].provenance_native_class, None);
    assert_eq!(rows[0].provenance_confidence, None);
    assert_eq!(rows[0].provenance_merged_from, None);

    let _logger = SqliteLogger::new(temp.path()).expect("second migration");
    assert_provenance_columns(temp.path());
}

#[test]
fn ner_provenance_row_round_trips_through_sqlite_logger_query() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let _logger = SqliteLogger::new(temp.path()).expect("fresh schema");
    let conn = Connection::open(temp.path()).expect("open db");
    conn.execute(
        r#"INSERT INTO redaction_log
            (source, recognizer_id, recognizer_version_id, class, action, field_name, document_kind,
             conflict_loser, decided_by, created_at, session_id, provenance_stage,
             provenance_model_id, provenance_model_version, provenance_artifact_sha256,
             provenance_tokenizer_sha256, provenance_locale_resolved, provenance_locale_match_kind,
             provenance_canonical_class, provenance_native_class, provenance_confidence,
             provenance_merged_from)
         VALUES
            ('ner/ort', 'ner', 'ner.model.v1', 'name', 'tokenize', NULL, 'text',
             0, 'none', 100, 'session-a', 'ner', 'davlan/mbert', 'v1',
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
             'de-DE', 'exact', 'name', 'PER', 0.875,
             '["regex.name","dictionary.employee"]')"#,
        [],
    )
    .expect("insert provenance row");

    let rows = SqliteLogger::query(temp.path(), &AuditFilter::default()).expect("query rows");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.provenance_stage.as_deref(), Some("ner"));
    assert_eq!(row.provenance_model_id.as_deref(), Some("davlan/mbert"));
    assert_eq!(row.provenance_model_version.as_deref(), Some("v1"));
    assert_eq!(
        row.provenance_artifact_sha256.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert_eq!(
        row.provenance_tokenizer_sha256.as_deref(),
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
    assert_eq!(row.provenance_locale_resolved.as_deref(), Some("de-DE"));
    assert_eq!(row.provenance_locale_match_kind.as_deref(), Some("exact"));
    assert_eq!(row.provenance_canonical_class.as_deref(), Some("name"));
    assert_eq!(row.provenance_native_class.as_deref(), Some("PER"));
    assert_eq!(row.provenance_confidence, Some(0.875));
    assert_eq!(
        row.provenance_merged_from.as_deref(),
        Some("[\"regex.name\",\"dictionary.employee\"]")
    );
}

#[test]
fn ner_provenance_filters_bind_expected_sql() {
    let cases = [
        (
            AuditFilter {
                provenance_stage: Some("ner".to_string()),
                ..AuditFilter::default()
            },
            "provenance_stage = ?",
            SqlValue::Text("ner".to_string()),
        ),
        (
            AuditFilter {
                provenance_model_id: Some("davlan/mbert".to_string()),
                ..AuditFilter::default()
            },
            "provenance_model_id = ?",
            SqlValue::Text("davlan/mbert".to_string()),
        ),
        (
            AuditFilter {
                provenance_model_version: Some("v1".to_string()),
                ..AuditFilter::default()
            },
            "provenance_model_version = ?",
            SqlValue::Text("v1".to_string()),
        ),
        (
            AuditFilter {
                provenance_artifact_sha256: Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                ),
                ..AuditFilter::default()
            },
            "provenance_artifact_sha256 = ?",
            SqlValue::Text(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ),
        ),
        (
            AuditFilter {
                provenance_tokenizer_sha256: Some(
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
                ),
                ..AuditFilter::default()
            },
            "provenance_tokenizer_sha256 = ?",
            SqlValue::Text(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            ),
        ),
        (
            AuditFilter {
                provenance_locale_resolved: Some("de-DE".to_string()),
                ..AuditFilter::default()
            },
            "provenance_locale_resolved = ?",
            SqlValue::Text("de-DE".to_string()),
        ),
        (
            AuditFilter {
                provenance_locale_match_kind: Some("exact".to_string()),
                ..AuditFilter::default()
            },
            "provenance_locale_match_kind = ?",
            SqlValue::Text("exact".to_string()),
        ),
        (
            AuditFilter {
                provenance_canonical_class: Some("name".to_string()),
                ..AuditFilter::default()
            },
            "provenance_canonical_class = ?",
            SqlValue::Text("name".to_string()),
        ),
        (
            AuditFilter {
                provenance_native_class: Some("PER".to_string()),
                ..AuditFilter::default()
            },
            "provenance_native_class = ?",
            SqlValue::Text("PER".to_string()),
        ),
        (
            AuditFilter {
                provenance_confidence: Some(0.875),
                ..AuditFilter::default()
            },
            "provenance_confidence = ?",
            SqlValue::Real(0.875),
        ),
        (
            AuditFilter {
                provenance_merged_from: Some("[\"regex.name\"]".to_string()),
                ..AuditFilter::default()
            },
            "provenance_merged_from = ?",
            SqlValue::Text("[\"regex.name\"]".to_string()),
        ),
    ];

    for (filter, predicate, expected_value) in cases {
        let (sql, values) = build_current_audit_query_sql(&filter);
        assert!(sql.contains(predicate), "{predicate}: {sql}");
        assert_eq!(values, vec![expected_value]);
    }
}

fn build_current_audit_query_sql(filter: &AuditFilter) -> (String, Vec<SqlValue>) {
    build_audit_query_sql(
        filter, true, true, true, true, true, true, true, true, true, true, true, true, true, true,
        true, true, true, true, true, true, true, true, true, true, true, true, true, true, true,
        true,
    )
}

fn create_pre_provenance_redaction_log(path: &Path) {
    let conn = Connection::open(path).expect("legacy sqlite");
    conn.execute_batch(
        r#"
        CREATE TABLE redaction_log (
            source TEXT NOT NULL,
            recognizer_id TEXT NULL,
            recognizer_version_id TEXT NULL,
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
            snapshot_key_version INTEGER NULL,
            validator_fail_reason TEXT NULL,
            ambiguity_record TEXT NULL,
            collision_family TEXT NULL,
            collision_variant TEXT NULL,
            fallback_triggered TEXT NULL
        );
        INSERT INTO redaction_log
            (source, recognizer_id, recognizer_version_id, class, action, field_name,
             document_kind, conflict_loser, decided_by, created_at, session_id,
             snapshot_scheme, snapshot_alg, snapshot_key_version)
        VALUES
            ('legacy', 'legacy_unversioned', NULL, 'name', 'tokenize', NULL,
             'text', 0, 'none', 100, NULL, 'gaze.snapshot.v1.sha256-salted',
             'SHA-256', NULL);
        "#,
    )
    .expect("legacy schema and row");
}

fn assert_provenance_columns(path: &Path) {
    let conn = Connection::open(path).expect("open db");
    let mut stmt = conn
        .prepare("PRAGMA table_info(redaction_log)")
        .expect("table info");
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect columns");

    for column in PROVENANCE_COLUMNS {
        assert!(columns.iter().any(|actual| actual == column), "{column}");
    }
}
