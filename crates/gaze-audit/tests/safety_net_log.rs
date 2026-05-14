use std::ops::Range;

use gaze_audit::{
    AuditFilter, AuditLogRow, LeakSuspectLogEntry, LeakSuspectLogger, SqliteLogger,
    DEFAULT_SNAPSHOT_ALG, DEFAULT_SNAPSHOT_SCHEME, SAFETY_NET_RESTRICTED_COLUMNS,
};
use gaze_types::{
    Action, ConflictTier, DocumentKind, LeakKind, LeakSuspect, PiiClass, RedactionEntry,
    RedactionLogger,
};
use rusqlite::Connection;
use tempfile::NamedTempFile;

#[test]
fn opens_v05_db_creates_safety_net_log_and_preserves_redaction_rows() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    create_v05_redaction_log(temp.path());
    let before = SqliteLogger::query(temp.path(), &AuditFilter::default()).expect("legacy query");

    let _logger = SqliteLogger::new(temp.path()).expect("migrate audit db");

    let conn = Connection::open(temp.path()).expect("open migrated db");
    let safety_net_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'safety_net_log'",
            [],
            |row| row.get(0),
        )
        .expect("table count");
    assert_eq!(safety_net_count, 1);

    let after = SqliteLogger::query(temp.path(), &AuditFilter::default()).expect("migrated query");
    assert!(before
        .iter()
        .all(|row| row.recognizer_id.is_none() && row.recognizer_version_id.is_none()));
    assert!(after.iter().all(|row| {
        row.recognizer_id.as_deref() == Some("legacy_unversioned")
            && row.recognizer_version_id.is_none()
    }));
    assert_eq!(after, legacy_rows());
}

#[test]
fn safety_net_log_insert_and_read_round_trips_bytes_free_metadata() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let logger = SqliteLogger::new(temp.path()).expect("sqlite logger");
    let suspect = leak_suspect(
        7..26,
        PiiClass::Email,
        LeakKind::ClassMismatch {
            pipeline_class: PiiClass::Name,
            safety_net_class: PiiClass::Email,
        },
    );
    let mut entry = LeakSuspectLogEntry::from_suspect(
        &suspect,
        DocumentKind::Structured,
        1_767_225_600_000,
        Some("session-safety-net".to_string()),
        Some("replay-sha256:6fb3f4".to_string()),
    );
    entry.backend_id = Some("opf-subprocess".to_string());
    entry.backend_version = Some("0.1.0".to_string());
    entry.decoding_params_hash = Some("decode-sha256:0f91".to_string());
    entry.telemetry_kind = Some("suspect".to_string());

    LeakSuspectLogger::log_leak_suspect(&logger, &entry).expect("log suspect");

    let rows = SqliteLogger::query_safety_net(temp.path(), &AuditFilter::default())
        .expect("query safety net");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, 1);
    assert_eq!(row.safety_net_id, "openai-privacy-filter");
    assert_eq!(row.raw_label, "private_email");
    assert_eq!(row.mapped_class, "email");
    assert_eq!(row.leak_kind, "class_mismatch");
    assert_eq!(row.span_len, 19);
    assert_eq!(row.document_kind, "structured");
    assert_eq!(row.field_path.as_deref(), Some("$.contact.email"));
    assert!((row.score.expect("score") - 0.91).abs() < 0.000_001);
    assert_eq!(row.created_at, 1_767_225_600_000);
    assert_eq!(row.session_id.as_deref(), Some("session-safety-net"));
    assert_eq!(row.pipeline_class.as_deref(), Some("name"));
    assert_eq!(
        row.safety_net_replay_hash.as_deref(),
        Some("replay-sha256:6fb3f4")
    );
    assert_eq!(row.backend_id.as_deref(), Some("opf-subprocess"));
    assert_eq!(row.backend_version.as_deref(), Some("0.1.0"));
    assert_eq!(
        row.decoding_params_hash.as_deref(),
        Some("decode-sha256:0f91")
    );
    assert_eq!(row.telemetry_kind.as_deref(), Some("suspect"));
}

#[test]
fn sqlite_logger_implements_gaze_types_redaction_logger_trait() {
    let temp = NamedTempFile::new().expect("temp db");
    let logger = SqliteLogger::new(temp.path()).expect("sqlite logger");
    let entry = RedactionEntry::new(
        "regex",
        PiiClass::Email,
        Action::Tokenize,
        Some("contact.email".to_string()),
        DocumentKind::Structured,
        false,
        ConflictTier::None,
        1_767_225_600_000,
        Some("session-redaction-log".to_string()),
    );

    let trait_object: &dyn RedactionLogger = &logger;
    trait_object.log(&entry).expect("log redaction entry");

    let rows = SqliteLogger::query(temp.path(), &AuditFilter::default()).expect("query rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].class, "email");
}

#[test]
fn safety_net_query_filter_maps_to_safety_net_columns() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let logger = SqliteLogger::new(temp.path()).expect("sqlite logger");
    let email = LeakSuspectLogEntry::from_suspect(
        &leak_suspect(0..5, PiiClass::Email, LeakKind::Uncovered),
        DocumentKind::Text,
        100,
        Some("session-a".to_string()),
        None,
    );
    let name = LeakSuspectLogEntry::from_suspect(
        &leak_suspect(
            10..20,
            PiiClass::Name,
            LeakKind::PartialBleed { uncovered: 10..12 },
        ),
        DocumentKind::Structured,
        200,
        Some("session-b".to_string()),
        None,
    );
    logger.log_safety_net(&email).expect("log email suspect");
    logger.log_safety_net(&name).expect("log name suspect");

    let rows = SqliteLogger::query_safety_net(
        temp.path(),
        &AuditFilter {
            class: Some("name".to_string()),
            source: Some("openai-privacy-filter".to_string()),
            action: Some("partial_bleed".to_string()),
            document_kind: Some("structured".to_string()),
            raw_label: None,
            field_path: None,
            from_epoch_ms: Some(150),
            to_epoch_ms: Some(250),
            session_id: Some("session-b".to_string()),
            snapshot_scheme: None,
            snapshot_alg: None,
            snapshot_key_version: None,
            has_ambiguity: None,
            ambiguity_reason: None,
            collision_family: None,
            collision_variant: None,
            recognizer_id: None,
            recognizer_version_id: None,
        },
    )
    .expect("filtered query");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].mapped_class, "name");
    assert_eq!(rows[0].leak_kind, "partial_bleed");
    assert_eq!(rows[0].session_id.as_deref(), Some("session-b"));
}

#[test]
fn restricted_columns_have_no_raw_payload_fields() {
    for column in SAFETY_NET_RESTRICTED_COLUMNS {
        assert!(
            !contains_forbidden_payload_term(column),
            "safety_net_log restricted column must stay bytes-free: {column}"
        );
    }

    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let _logger = SqliteLogger::new(temp.path()).expect("sqlite logger");
    let conn = Connection::open(temp.path()).expect("open db");
    let actual_columns = table_columns(&conn, "safety_net_log");
    assert_eq!(actual_columns, SAFETY_NET_RESTRICTED_COLUMNS);
}

#[test]
fn safety_net_log_does_not_persist_suspect_or_placeholder_bytes() {
    let temp = tempfile::NamedTempFile::new().expect("temp db");
    let logger = SqliteLogger::new(temp.path()).expect("sqlite logger");
    let raw_suspect_bytes = "alice@example.invalid";
    let placeholder_bytes = "<Email_1>";
    let suspect = leak_suspect(
        0..raw_suspect_bytes.len(),
        PiiClass::Email,
        LeakKind::Uncovered,
    );
    let entry = LeakSuspectLogEntry::from_suspect(
        &suspect,
        DocumentKind::Text,
        1_767_225_600_000,
        None,
        Some("replay-sha256:no-payload".to_string()),
    );

    logger.log_safety_net(&entry).expect("log suspect");

    let conn = Connection::open(temp.path()).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT safety_net_id, raw_label, mapped_class, leak_kind, document_kind, field_path,
                    session_id, pipeline_class, safety_net_replay_hash, backend_id,
                    backend_version, decoding_params_hash, telemetry_kind
             FROM safety_net_log",
        )
        .expect("prepare text scan");
    let rows = stmt
        .query_map([], |row| {
            let mut values = Vec::<Option<String>>::new();
            for index in 0..13 {
                values.push(row.get(index)?);
            }
            Ok(values)
        })
        .expect("scan rows");

    for row in rows {
        for value in row.expect("row values").into_iter().flatten() {
            assert_ne!(value, raw_suspect_bytes);
            assert_ne!(value, placeholder_bytes);
            assert!(!value.contains(raw_suspect_bytes));
            assert!(!value.contains(placeholder_bytes));
        }
    }
}

fn create_v05_redaction_log(path: &std::path::Path) {
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
            session_id TEXT NULL
        );
        INSERT INTO redaction_log
            (source, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id)
        VALUES
            ('regex', 'email', 'tokenize', 'contact.email', 'structured', 0, 'none', 100, 'session-a'),
            ('dictionary', 'name', 'preserve', NULL, 'text', 1, 'class_priority', 200, NULL);
        "#,
    )
    .expect("legacy schema and rows");
}

fn legacy_rows() -> Vec<AuditLogRow> {
    vec![
        AuditLogRow {
            source: "regex".to_string(),
            recognizer_id: Some("legacy_unversioned".to_string()),
            recognizer_version_id: None,
            class: "email".to_string(),
            action: "tokenize".to_string(),
            field_name: Some("contact.email".to_string()),
            document_kind: "structured".to_string(),
            conflict_loser: false,
            decided_by: "none".to_string(),
            created_at: Some(100),
            session_id: Some("session-a".to_string()),
            snapshot_scheme: DEFAULT_SNAPSHOT_SCHEME.to_string(),
            snapshot_alg: DEFAULT_SNAPSHOT_ALG.to_string(),
            snapshot_key_version: None,
            validator_fail_reason: None,
            ambiguity_record: None,
            collision_family: None,
            collision_variant: None,
            fallback_triggered: None,
        },
        AuditLogRow {
            source: "dictionary".to_string(),
            recognizer_id: Some("legacy_unversioned".to_string()),
            recognizer_version_id: None,
            class: "name".to_string(),
            action: "preserve".to_string(),
            field_name: None,
            document_kind: "text".to_string(),
            conflict_loser: true,
            decided_by: "class_priority".to_string(),
            created_at: Some(200),
            session_id: None,
            snapshot_scheme: DEFAULT_SNAPSHOT_SCHEME.to_string(),
            snapshot_alg: DEFAULT_SNAPSHOT_ALG.to_string(),
            snapshot_key_version: None,
            validator_fail_reason: None,
            ambiguity_record: None,
            collision_family: None,
            collision_variant: None,
            fallback_triggered: None,
        },
    ]
}

fn leak_suspect(span: Range<usize>, class: PiiClass, kind: LeakKind) -> LeakSuspect {
    LeakSuspect::new(
        span,
        class,
        "openai-privacy-filter",
        Some(0.91),
        kind,
        "private_email",
        Some("$.contact.email".to_string()),
    )
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table info");
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query columns");
    columns.map(|column| column.expect("column name")).collect()
}

fn contains_forbidden_payload_term(column: &str) -> bool {
    let lower = column.to_ascii_lowercase();
    [
        "text",
        "payload",
        "content",
        "bytes",
        "placeholder",
        "value",
    ]
    .iter()
    .any(|term| lower.contains(term))
}
