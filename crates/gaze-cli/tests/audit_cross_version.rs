use assert_cmd::Command;
use gaze::{build_audit_query_sql, AuditFilter};
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn v0_4_3_shape_without_created_at_is_queryable_but_time_filters_omit_nulls() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("v0.4.3.sqlite");
    let conn = Connection::open(&audit_path).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE redaction_log (
            source TEXT NOT NULL,
            class TEXT NOT NULL,
            action TEXT NOT NULL,
            field_name TEXT NULL,
            document_kind TEXT NOT NULL,
            conflict_loser INTEGER NOT NULL,
            decided_by TEXT NOT NULL DEFAULT 'none'
        );
        INSERT INTO redaction_log
            (source, class, action, field_name, document_kind, conflict_loser, decided_by)
        VALUES
            ('email.global', 'email', 'tokenize', NULL, 'text', 0, 'recognizer_id');
        "#,
    )
    .unwrap();

    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--class",
            "email",
            "--from",
            "2099-01-01T00:00:00Z",
            "--to",
            "2099-01-02T00:00:00Z",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "filtered legacy rows with NULL created_at must be omitted: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--class",
            "email",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let row: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(row["class"], "email");
    assert_eq!(row["source"], "email.global");
    assert_eq!(row["action"], "tokenize");
    assert_eq!(row["field_name"], Value::Null);
    assert_eq!(row["document_kind"], "text");
    assert_eq!(row["conflict_loser"], false);
    assert_eq!(row["decided_by"], "recognizer_id");
    assert_eq!(row["created_at"], Value::Null);
}

#[test]
fn v0_4_4_shape_with_created_at_is_queryable_and_time_filtered() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("v0.4.4.sqlite");
    let conn = Connection::open(&audit_path).unwrap();
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
            created_at INTEGER NULL
        );
        INSERT INTO redaction_log
            (source, class, action, field_name, document_kind, conflict_loser, decided_by, created_at)
        VALUES
            ('email.global', 'email', 'tokenize', NULL, 'text', 0, 'recognizer_id', 1700000000000),
            ('phone.global', 'custom:phone', 'tokenize', NULL, 'text', 0, 'recognizer_id', 1700001000000);
        "#,
    )
    .unwrap();

    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--from",
            "2023-11-14T22:13:20Z",
            "--to",
            "2023-11-14T22:13:20Z",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["source"], "email.global");
    assert_eq!(rows[0]["created_at"], 1700000000000_i64);
}

#[test]
fn legacy_schema_without_decided_by_is_queryable() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("legacy.sqlite");
    let conn = Connection::open(&audit_path).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE redaction_log (
            source TEXT NOT NULL,
            class TEXT NOT NULL,
            action TEXT NOT NULL,
            field_name TEXT NULL,
            document_kind TEXT NOT NULL,
            conflict_loser INTEGER NOT NULL
        );
        INSERT INTO redaction_log
            (source, class, action, field_name, document_kind, conflict_loser)
        VALUES
            ('dictionary:audit_terms[#0]', 'custom:term', 'tokenize', NULL, 'text', 0);
        "#,
    )
    .unwrap();

    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "query",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--source",
            "dictionary:audit_terms[#0]",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "audit query failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(
        "source\tclass\taction\tfield_name\tdocument_kind\tconflict_loser\tdecided_by\tcreated_at\n"
    ));
    assert!(
        stdout.contains("dictionary:audit_terms[#0]\tcustom:term\ttokenize\t\ttext\tfalse\tnone")
    );
}

#[test]
fn audit_sql_uses_restricted_column_set() {
    let filter = AuditFilter {
        class: Some("email".to_string()),
        source: Some("email.global".to_string()),
        action: Some("tokenize".to_string()),
        document_kind: Some("text".to_string()),
        from_epoch_ms: Some(1_700_000_000_000),
        to_epoch_ms: Some(1_700_000_010_000),
    };
    let (current_sql, values) = build_audit_query_sql(&filter, true, true);
    assert_eq!(
        values,
        [
            SqlValue::Text("email".to_string()),
            SqlValue::Text("email.global".to_string()),
            SqlValue::Text("tokenize".to_string()),
            SqlValue::Text("text".to_string()),
            SqlValue::Integer(1_700_000_000_000),
            SqlValue::Integer(1_700_000_010_000),
        ]
        .into_iter()
        .collect::<Vec<_>>()
    );
    assert_restricted_sql(&current_sql);
    assert!(current_sql.contains("created_at >= ?"));
    assert!(current_sql.contains("created_at <= ?"));
    assert!(!current_sql.contains("created_at IS NULL"));

    let legacy_filter = AuditFilter {
        from_epoch_ms: Some(1_700_000_000_000),
        to_epoch_ms: Some(1_700_000_010_000),
        ..AuditFilter::default()
    };
    let (legacy_sql, legacy_values) = build_audit_query_sql(&legacy_filter, false, false);
    assert_restricted_sql(&legacy_sql);
    assert!(legacy_sql.contains("'none' AS decided_by"));
    assert!(legacy_sql.contains("NULL AS created_at"));
    assert!(legacy_sql.contains("NULL >= ?"));
    assert!(legacy_sql.contains("NULL <= ?"));
    assert_eq!(
        legacy_values,
        [
            SqlValue::Integer(1_700_000_000_000),
            SqlValue::Integer(1_700_000_010_000),
        ]
        .into_iter()
        .collect::<Vec<_>>()
    );
}

fn assert_restricted_sql(sql: &str) {
    let lower = sql.to_ascii_lowercase();
    assert!(
        !lower.contains("select *"),
        "audit SQL must never SELECT *: {sql}"
    );
    for sensitive in ["raw", "value", "token"] {
        assert!(
            !lower.contains(sensitive),
            "audit SQL must not read sensitive column '{sensitive}': {sql}"
        );
    }
    assert!(lower
        .starts_with("select source, class, action, field_name, document_kind, conflict_loser, "));
    assert!(lower.contains(" from redaction_log"));
}
