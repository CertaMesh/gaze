use assert_cmd::Command;
use gaze_audit::{
    build_audit_query_sql, AuditFilter, PresentColumns, AUDIT_RESTRICTED_COLUMNS,
    DEFAULT_SNAPSHOT_ALG, DEFAULT_SNAPSHOT_SCHEME,
};
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
    assert_eq!(row["session_id"], Value::Null);
    assert_eq!(row["snapshot_scheme"], DEFAULT_SNAPSHOT_SCHEME);
    assert_eq!(row["snapshot_alg"], DEFAULT_SNAPSHOT_ALG);
    assert_eq!(row["snapshot_key_version"], Value::Null);
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
    assert_eq!(rows[0]["session_id"], Value::Null);
    assert_eq!(rows[0]["snapshot_scheme"], DEFAULT_SNAPSHOT_SCHEME);
    assert_eq!(rows[0]["snapshot_alg"], DEFAULT_SNAPSHOT_ALG);
    assert_eq!(rows[0]["snapshot_key_version"], Value::Null);
}

#[test]
fn v0_4_5_shape_with_session_id_is_queryable_and_session_filtered() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("v0.4.5.sqlite");
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
            created_at INTEGER NULL,
            session_id TEXT NULL
        );
        INSERT INTO redaction_log
            (source, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id)
        VALUES
            ('email.global', 'email', 'tokenize', NULL, 'text', 0, 'recognizer_id', 1700000000000, '018bcfe5-6800-7a2f-9d1b-47b7565b2d10'),
            ('phone.global', 'custom:phone', 'tokenize', NULL, 'text', 0, 'recognizer_id', 1700001000000, '018bcff4-aa40-7b01-9fcb-0c98049b2a02'),
            ('legacy.global', 'custom:legacy', 'tokenize', NULL, 'text', 0, 'recognizer_id', 1700002000000, NULL);
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
            "--session",
            "018bcfe5-6800-7a2f-9d1b-47b7565b2d10",
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
    assert_eq!(
        rows[0]["session_id"],
        "018bcfe5-6800-7a2f-9d1b-47b7565b2d10"
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
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "audit export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 3);
}

#[test]
fn pre_spike_4_shape_without_ambiguity_columns_is_queryable() {
    let dir = tempdir().unwrap();
    let audit_path = dir.path().join("pre-spike-4.sqlite");
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
            ('email.global', 'email', 'tokenize', NULL, 'text', 0, 'recognizer_id',
             1700000000000, 'session-a', 'gaze.snapshot.v1.sha256-salted', 'SHA-256', NULL),
            ('cards.global', 'credit_card', 'tokenize', NULL, 'text', 1, 'validator_veto',
             1700000000001, 'session-a', 'gaze.snapshot.v1.sha256-salted', 'SHA-256', NULL),
            ('iban.global', 'custom:iban', 'tokenize', NULL, 'text', 1, 'collision_policy',
             1700000000002, 'session-a', 'gaze.snapshot.v1.sha256-salted', 'SHA-256', NULL),
            ('iban.anchor', 'custom:family:payment-card-or-iban', 'tokenize', NULL, 'text', 0, 'anchored_context',
             1700000000003, 'session-a', 'gaze.snapshot.v1.sha256-salted', 'SHA-256', NULL);
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
    let stdout = String::from_utf8(output.stdout).unwrap();
    let row: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(row["validator_fail_reason"], Value::Null);
    assert_eq!(row["ambiguity_record"], Value::Null);
    assert_eq!(row["collision_family"], Value::Null);
    assert_eq!(row["collision_variant"], Value::Null);

    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--source",
            "cards.global",
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
    assert_eq!(row["decided_by"], "validator_veto");
    assert_eq!(row["conflict_loser"], true);

    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--source",
            "iban.global",
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
    assert_eq!(row["decided_by"], "collision_policy");
    assert_eq!(row["conflict_loser"], true);

    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--source",
            "iban.anchor",
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
    assert_eq!(row["decided_by"], "anchored_context");
    assert_eq!(row["conflict_loser"], false);

    let output = Command::cargo_bin("gaze")
        .unwrap()
        .args([
            "audit",
            "export",
            "--audit-db",
            audit_path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--has-ambiguity",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
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
        "source\trecognizer_id\trecognizer_version_id\tclass\taction\tfield_name\tdocument_kind\tconflict_loser\tdecided_by\tcreated_at\tsession_id\tsnapshot_scheme\tsnapshot_alg\tsnapshot_key_version\tvalidator_fail_reason\tambiguity_record\tcollision_family\tcollision_variant\tfallback_triggered\tprovenance_stage\tprovenance_model_id\tprovenance_model_version\tprovenance_artifact_sha256\tprovenance_tokenizer_sha256\tprovenance_locale_resolved\tprovenance_locale_match_kind\tprovenance_canonical_class\tprovenance_native_class\tprovenance_confidence\tprovenance_merged_from\trestore_policy\trestore_decision\trestore_unknown_token_count\trestore_manifest_bypass_count\trestore_fresh_pii_count\trestore_phase_mask\n"
    ));
    assert!(stdout.contains("dictionary:audit_terms[#0]"));
    assert!(stdout.contains("custom:term"));
    assert!(stdout.contains("tokenize"));
}

#[test]
fn audit_sql_uses_restricted_column_set() {
    let filter = AuditFilter {
        class: Some("email".to_string()),
        source: Some("email.global".to_string()),
        action: Some("tokenize".to_string()),
        document_kind: Some("text".to_string()),
        raw_label: None,
        field_path: None,
        from_epoch_ms: Some(1_700_000_000_000),
        to_epoch_ms: Some(1_700_000_010_000),
        session_id: Some("018bcfe5-6800-7a2f-9d1b-47b7565b2d10".to_string()),
        snapshot_scheme: Some(DEFAULT_SNAPSHOT_SCHEME.to_string()),
        snapshot_alg: Some(DEFAULT_SNAPSHOT_ALG.to_string()),
        snapshot_key_version: None,
        has_ambiguity: None,
        ambiguity_reason: None,
        collision_family: None,
        collision_variant: None,
        recognizer_id: None,
        recognizer_version_id: None,
        provenance_stage: None,
        provenance_model_id: None,
        provenance_model_version: None,
        provenance_artifact_sha256: None,
        provenance_tokenizer_sha256: None,
        provenance_locale_resolved: None,
        provenance_locale_match_kind: None,
        provenance_canonical_class: None,
        provenance_native_class: None,
        provenance_confidence: None,
        provenance_merged_from: None,
        restore_events_only: false,
    };
    let current_columns = PresentColumns::new(
        AUDIT_RESTRICTED_COLUMNS
            .iter()
            .map(|column| (*column).to_string())
            .collect(),
    );
    let (current_sql, values) = build_audit_query_sql(&filter, &current_columns);
    assert_eq!(
        values,
        [
            SqlValue::Text("email".to_string()),
            SqlValue::Text("email.global".to_string()),
            SqlValue::Text("tokenize".to_string()),
            SqlValue::Text("text".to_string()),
            SqlValue::Integer(1_700_000_000_000),
            SqlValue::Integer(1_700_000_010_000),
            SqlValue::Text("018bcfe5-6800-7a2f-9d1b-47b7565b2d10".to_string()),
            SqlValue::Text(DEFAULT_SNAPSHOT_SCHEME.to_string()),
            SqlValue::Text(DEFAULT_SNAPSHOT_ALG.to_string()),
        ]
        .into_iter()
        .collect::<Vec<_>>()
    );
    assert_restricted_sql(&current_sql);
    assert!(current_sql.contains("created_at >= ?"));
    assert!(current_sql.contains("created_at <= ?"));
    assert!(current_sql.contains("session_id = ?"));
    assert!(current_sql.contains("snapshot_scheme = ?"));
    assert!(current_sql.contains("snapshot_alg = ?"));
    assert!(!current_sql.contains("created_at IS NULL"));

    let legacy_filter = AuditFilter {
        from_epoch_ms: Some(1_700_000_000_000),
        to_epoch_ms: Some(1_700_000_010_000),
        session_id: Some("018bcfe5-6800-7a2f-9d1b-47b7565b2d10".to_string()),
        snapshot_scheme: Some(DEFAULT_SNAPSHOT_SCHEME.to_string()),
        snapshot_alg: Some(DEFAULT_SNAPSHOT_ALG.to_string()),
        snapshot_key_version: Some(1),
        ..AuditFilter::default()
    };
    let legacy_columns = PresentColumns::default();
    let (legacy_sql, legacy_values) = build_audit_query_sql(&legacy_filter, &legacy_columns);
    assert_restricted_sql(&legacy_sql);
    assert!(legacy_sql.contains("'none' AS decided_by"));
    assert!(legacy_sql.contains("NULL AS created_at"));
    assert!(legacy_sql.contains("NULL AS session_id"));
    assert!(legacy_sql.contains("'gaze.snapshot.v1.sha256-salted' AS snapshot_scheme"));
    assert!(legacy_sql.contains("'SHA-256' AS snapshot_alg"));
    assert!(legacy_sql.contains("NULL AS snapshot_key_version"));
    assert!(legacy_sql.contains("NULL >= ?"));
    assert!(legacy_sql.contains("NULL <= ?"));
    assert!(legacy_sql.contains("NULL = ?"));
    assert!(legacy_sql.contains("'gaze.snapshot.v1.sha256-salted' = ?"));
    assert!(legacy_sql.contains("'SHA-256' = ?"));
    assert_eq!(
        legacy_values,
        [
            SqlValue::Integer(1_700_000_000_000),
            SqlValue::Integer(1_700_000_010_000),
            SqlValue::Text("018bcfe5-6800-7a2f-9d1b-47b7565b2d10".to_string()),
            SqlValue::Text(DEFAULT_SNAPSHOT_SCHEME.to_string()),
            SqlValue::Text(DEFAULT_SNAPSHOT_ALG.to_string()),
            SqlValue::Integer(1),
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
    for sensitive in ["raw_value", "token_value", "original_value"] {
        assert!(
            !lower.contains(sensitive),
            "audit SQL must not read sensitive column '{sensitive}': {sql}"
        );
    }
    assert!(lower.starts_with("select source, "));
    assert!(lower.contains("recognizer_id"));
    assert!(lower.contains("recognizer_version_id"));
    assert!(lower.contains(" class, action, field_name, document_kind, conflict_loser, "));
    assert!(lower.contains(" from redaction_log"));
}
