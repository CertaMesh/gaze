use assert_cmd::Command;
use gaze::{build_audit_query_sql, AuditFilter};
use rusqlite::Connection;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn current_schema_fixture_is_queryable() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("current.sqlite");
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
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = String::from_utf8(output.stdout).unwrap();
    let row: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(row["class"], "email");
    assert_eq!(row["source"], "email.global");
    assert_eq!(row["action"], "tokenize");
    assert_eq!(row["document_kind"], "text");
    assert_eq!(row["decided_by"], "recognizer_id");
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
    assert!(stdout.contains("class\tsource\taction\tdocument_kind\tdecided_by\n"));
    assert!(stdout.contains("custom:term\tdictionary:audit_terms[#0]\ttokenize\ttext\tnone"));
}

#[test]
fn audit_sql_uses_restricted_column_set() {
    let filter = AuditFilter {
        class: Some("email".to_string()),
        source: Some("email.global".to_string()),
        action: Some("tokenize".to_string()),
        document_kind: Some("text".to_string()),
    };
    let (current_sql, values) = build_audit_query_sql(&filter, true);
    assert_eq!(
        values,
        ["email", "email.global", "tokenize", "text"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
    assert_restricted_sql(&current_sql);

    let (legacy_sql, _) = build_audit_query_sql(&AuditFilter::default(), false);
    assert_restricted_sql(&legacy_sql);
    assert!(legacy_sql.contains("'none' AS decided_by"));
}

fn assert_restricted_sql(sql: &str) {
    let lower = sql.to_ascii_lowercase();
    assert!(
        !lower.contains("select *"),
        "audit SQL must never SELECT *: {sql}"
    );
    for sensitive in ["field_name", "conflict_loser", "raw", "value", "token"] {
        assert!(
            !lower.contains(sensitive),
            "audit SQL must not read sensitive column '{sensitive}': {sql}"
        );
    }
    assert!(lower.starts_with("select class, source, action, document_kind, "));
    assert!(lower.contains(" from redaction_log"));
}
