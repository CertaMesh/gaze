use std::path::Path;
use std::sync::Mutex;

use gaze_types::{
    Action, AmbiguityRecord, ConflictTier, DocumentKind, FallbackReason, LeakKind, LeakSuspect,
    PiiClass, RedactionEntry, ValidatorFailReason,
};
use rusqlite::{params, params_from_iter, Connection, OpenFlags};
use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

use crate::query::{
    build_audit_query_sql, build_safety_net_query_sql, AuditFilter, AuditLogRow, LeakSuspectRow,
    DEFAULT_SNAPSHOT_ALG, DEFAULT_SNAPSHOT_SCHEME,
};

pub type Result<T> = std::result::Result<T, AuditError>;

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("sqlite error: {0}")]
    Sqlite(String),
}

/// SQLite-backed [`gaze_types::RedactionLogger`] implementation.
///
/// Appends redaction metadata rows to a local SQLite database. The schema is
/// append-only: rows are inserted and optionally purged by TTL, never updated
/// in place.
///
/// `SqliteLogger` is **not** `Clone`. Build it where the pipeline is
/// constructed and pass ownership directly. Querying happens through the static
/// [`SqliteLogger::query`] and [`SqliteLogger::query_safety_net`] functions.
/// They take a [`Path`], so the database file can be queried after the logger
/// has been moved into the pipeline.
///
/// # Audit log is metadata-only
///
/// Records class, action, source, field name, document kind, conflict status,
/// decision tier, timestamp, and session ID. It never stores original PII or
/// token values. **Do not use as a restore source**: restore requires the
/// `gaze::SensitiveSnapshot` exported from a `gaze::Session`.
///
/// # Isolation
///
/// `gaze` core has no compile-time dependency on `gaze-audit`. Wire
/// `SqliteLogger` in your application layer only. See the Dylint isolation gate
/// in `docs/architecture/`.
///
/// # Example
///
/// ```rust
/// use std::path::Path;
/// use gaze_audit::SqliteLogger;
///
/// let logger = SqliteLogger::new(Path::new("audit.db"))?;
/// # let _ = logger;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// In an application that depends on both `gaze` and `gaze-audit`, pass the
/// logger into the pipeline builder once:
///
/// ```rust,ignore
/// use std::path::Path;
/// use gaze::Pipeline;
/// use gaze_audit::SqliteLogger;
///
/// let logger = SqliteLogger::new(Path::new("audit.db"))?;
/// let pipeline = Pipeline::builder()
///     .redaction_logger(logger)
///     .build()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct SqliteLogger {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeakSuspectLogEntry {
    pub safety_net_id: String,
    pub raw_label: String,
    pub mapped_class: String,
    pub leak_kind: String,
    pub span_len: i64,
    pub document_kind: String,
    pub field_path: Option<String>,
    pub score: Option<f64>,
    pub created_at: i64,
    pub session_id: Option<String>,
    pub pipeline_class: Option<String>,
    pub safety_net_replay_hash: Option<String>,
    pub backend_id: Option<String>,
    pub backend_version: Option<String>,
    pub decoding_params_hash: Option<String>,
    pub telemetry_kind: Option<String>,
}

impl LeakSuspectLogEntry {
    pub fn from_suspect(
        suspect: &LeakSuspect,
        document_kind: DocumentKind,
        created_at: i64,
        session_id: Option<String>,
        safety_net_replay_hash: Option<String>,
    ) -> Self {
        Self {
            safety_net_id: suspect.safety_net_id.clone(),
            raw_label: suspect.raw_label.clone(),
            mapped_class: suspect.class.to_canonical_str(),
            leak_kind: leak_kind_to_db(&suspect.kind).to_string(),
            span_len: suspect.span.end.saturating_sub(suspect.span.start) as i64,
            document_kind: document_kind_to_db(&document_kind).to_string(),
            field_path: suspect.field_path.clone(),
            score: suspect.score.map(f64::from),
            created_at,
            session_id,
            pipeline_class: leak_kind_pipeline_class(&suspect.kind).map(PiiClass::to_canonical_str),
            safety_net_replay_hash,
            backend_id: None,
            backend_version: None,
            decoding_params_hash: None,
            telemetry_kind: None,
        }
    }
}

pub trait LeakSuspectLogger: Send + Sync {
    fn log_leak_suspect(&self, entry: &LeakSuspectLogEntry) -> Result<()>;
}

impl SqliteLogger {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|err| AuditError::Sqlite(err.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS redaction_log (
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

            CREATE TABLE IF NOT EXISTS safety_net_log (
                id INTEGER PRIMARY KEY,
                safety_net_id TEXT NOT NULL,
                raw_label TEXT NOT NULL,
                mapped_class TEXT NOT NULL,
                leak_kind TEXT NOT NULL,
                span_len INTEGER NOT NULL,
                document_kind TEXT NOT NULL,
                field_path TEXT NULL,
                score REAL NULL,
                created_at INTEGER NOT NULL,
                session_id TEXT NULL,
                pipeline_class TEXT NULL,
                safety_net_replay_hash TEXT NULL,
                backend_id TEXT NULL,
                backend_version TEXT NULL,
                decoding_params_hash TEXT NULL,
                telemetry_kind TEXT NULL
            );
            "#,
        )
        .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        let columns = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(redaction_log)")
                .map_err(|err| AuditError::Sqlite(err.to_string()))?;
            let columns = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|err| AuditError::Sqlite(err.to_string()))?;
            let mut names = Vec::new();
            for column in columns {
                names.push(column.map_err(|err| AuditError::Sqlite(err.to_string()))?);
            }
            names
        };
        if !columns.iter().any(|column| column == "decided_by") {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN decided_by TEXT NOT NULL DEFAULT 'none'",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "created_at") {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN created_at INTEGER NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "session_id") {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN session_id TEXT NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "snapshot_scheme") {
            conn.execute(
                &format!(
                    "ALTER TABLE redaction_log ADD COLUMN snapshot_scheme TEXT NOT NULL DEFAULT '{}'",
                    DEFAULT_SNAPSHOT_SCHEME
                ),
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "snapshot_alg") {
            conn.execute(
                &format!(
                    "ALTER TABLE redaction_log ADD COLUMN snapshot_alg TEXT NOT NULL DEFAULT '{}'",
                    DEFAULT_SNAPSHOT_ALG
                ),
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns
            .iter()
            .any(|column| column == "snapshot_key_version")
        {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN snapshot_key_version INTEGER NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns
            .iter()
            .any(|column| column == "validator_fail_reason")
        {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN validator_fail_reason TEXT NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "ambiguity_record") {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN ambiguity_record TEXT NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "collision_family") {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN collision_family TEXT NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "collision_variant") {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN collision_variant TEXT NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "fallback_triggered") {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN fallback_triggered TEXT NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns.iter().any(|column| column == "recognizer_id") {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN recognizer_id TEXT NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
            conn.execute(
                "UPDATE redaction_log SET recognizer_id = 'legacy_unversioned' WHERE recognizer_id IS NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        if !columns
            .iter()
            .any(|column| column == "recognizer_version_id")
        {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN recognizer_version_id TEXT NULL",
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn log(&self, entry: &RedactionEntry) -> Result<()> {
        let validator_fail_reason = serialize_json_column(entry.validator_fail_reason.as_ref())?;
        let ambiguity_record = serialize_json_column(entry.ambiguity_record.as_ref())?;
        let fallback_triggered = entry.fallback_triggered.map(fallback_reason_to_db);
        let conn = self
            .conn
            .lock()
            .map_err(|_| AuditError::Sqlite("sqlite mutex poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO redaction_log (source, recognizer_id, recognizer_version_id, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id, validator_fail_reason, ambiguity_record, collision_family, collision_variant, fallback_triggered) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                entry.source,
                entry.recognizer_id,
                entry.recognizer_version_id,
                entry.class.to_canonical_str(),
                action_to_db(entry.action),
                entry.field_name,
                document_kind_to_db(&entry.document_kind),
                if entry.conflict_loser { 1 } else { 0 },
                conflict_tier_to_db(entry.decided_by),
                entry.created_at,
                entry.session_id,
                validator_fail_reason,
                ambiguity_record,
                entry.collision_family,
                entry.collision_variant,
                fallback_triggered,
            ],
        )
        .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        Ok(())
    }

    pub fn entries(&self) -> Result<Vec<RedactionEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AuditError::Sqlite("sqlite mutex poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT source, recognizer_id, recognizer_version_id, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id, validator_fail_reason, ambiguity_record, collision_family, collision_variant, fallback_triggered FROM redaction_log",
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let mut entry = RedactionEntry::new(
                    row.get::<_, String>(0)?,
                    pii_class_from_db(&row.get::<_, String>(3)?)?,
                    action_from_db(&row.get::<_, String>(4)?)?,
                    row.get(5)?,
                    document_kind_from_db(&row.get::<_, String>(6)?)?,
                    row.get::<_, i64>(7)? != 0,
                    conflict_tier_from_db(&row.get::<_, String>(8)?)?,
                    row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    row.get(10)?,
                )
                .with_recognizer_metadata(row.get(1)?, row.get(2)?);
                if let Some(reason) =
                    deserialize_json_column::<ValidatorFailReason>(row.get(11)?, 11)?
                {
                    entry = entry.with_validator_fail_reason(reason);
                }
                if let Some(record) = deserialize_json_column::<AmbiguityRecord>(row.get(12)?, 12)?
                {
                    entry = entry.with_ambiguity_record(record);
                }
                entry = entry.with_collision_metadata(row.get(13)?, row.get(14)?);
                if let Some(reason) = row
                    .get::<_, Option<String>>(15)?
                    .map(|value| fallback_reason_from_db(&value))
                    .transpose()?
                {
                    entry = entry.with_fallback_triggered(reason);
                }
                Ok(entry)
            })
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|err| AuditError::Sqlite(err.to_string()))?);
        }
        Ok(entries)
    }

    /// Queries metadata-only redaction audit rows from a SQLite database file.
    ///
    /// This is a static function, not a method on `SqliteLogger`. Call it with
    /// the database [`Path`] and an [`AuditFilter`] after the logger has been
    /// moved into a pipeline or dropped.
    pub fn query(path: &Path, filter: &AuditFilter) -> Result<Vec<AuditLogRow>> {
        let conn = open_audit_query_connection(path)?;
        let has_decided_by = table_has_column(&conn, "decided_by")?;
        let has_created_at = table_has_column(&conn, "created_at")?;
        let has_session_id = table_has_column(&conn, "session_id")?;
        let has_snapshot_scheme = table_has_column(&conn, "snapshot_scheme")?;
        let has_snapshot_alg = table_has_column(&conn, "snapshot_alg")?;
        let has_snapshot_key_version = table_has_column(&conn, "snapshot_key_version")?;
        let has_validator_fail_reason = table_has_column(&conn, "validator_fail_reason")?;
        let has_ambiguity_record = table_has_column(&conn, "ambiguity_record")?;
        let has_collision_family = table_has_column(&conn, "collision_family")?;
        let has_collision_variant = table_has_column(&conn, "collision_variant")?;
        let has_fallback_triggered = table_has_column(&conn, "fallback_triggered")?;
        let has_recognizer_id = table_has_column(&conn, "recognizer_id")?;
        let has_recognizer_version_id = table_has_column(&conn, "recognizer_version_id")?;
        let (sql, values) = build_audit_query_sql(
            filter,
            has_decided_by,
            has_created_at,
            has_session_id,
            has_snapshot_scheme,
            has_snapshot_alg,
            has_snapshot_key_version,
            has_validator_fail_reason,
            has_ambiguity_record,
            has_collision_family,
            has_collision_variant,
            has_fallback_triggered,
            has_recognizer_id,
            has_recognizer_version_id,
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        let rows = stmt
            .query_map(params_from_iter(values.iter()), |row| {
                Ok(AuditLogRow {
                    source: row.get(0)?,
                    recognizer_id: row.get(1)?,
                    recognizer_version_id: row.get(2)?,
                    class: row.get(3)?,
                    action: row.get(4)?,
                    field_name: row.get(5)?,
                    document_kind: row.get(6)?,
                    conflict_loser: row.get::<_, i64>(7)? != 0,
                    decided_by: row.get(8)?,
                    created_at: row.get(9)?,
                    session_id: row.get(10)?,
                    snapshot_scheme: row.get(11)?,
                    snapshot_alg: row.get(12)?,
                    snapshot_key_version: row.get(13)?,
                    validator_fail_reason: row.get(14)?,
                    ambiguity_record: row.get(15)?,
                    collision_family: row.get(16)?,
                    collision_variant: row.get(17)?,
                    fallback_triggered: row.get(18)?,
                })
            })
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|err| AuditError::Sqlite(err.to_string()))?);
        }
        Ok(entries)
    }

    /// Queries metadata-only safety-net audit rows from a SQLite database file.
    ///
    /// Returned rows include labels, classes, span lengths, and replay hashes,
    /// never suspect text bytes or emitted placeholder bytes.
    pub fn query_safety_net(path: &Path, filter: &AuditFilter) -> Result<Vec<LeakSuspectRow>> {
        let conn = open_audit_query_connection(path)?;
        let (sql, values) = build_safety_net_query_sql(filter);
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        let rows = stmt
            .query_map(params_from_iter(values.iter()), |row| {
                Ok(LeakSuspectRow {
                    id: row.get(0)?,
                    safety_net_id: row.get(1)?,
                    raw_label: row.get(2)?,
                    mapped_class: row.get(3)?,
                    leak_kind: row.get(4)?,
                    span_len: row.get(5)?,
                    document_kind: row.get(6)?,
                    field_path: row.get(7)?,
                    score: row.get(8)?,
                    created_at: row.get(9)?,
                    session_id: row.get(10)?,
                    pipeline_class: row.get(11)?,
                    safety_net_replay_hash: row.get(12)?,
                    backend_id: row.get(13)?,
                    backend_version: row.get(14)?,
                    decoding_params_hash: row.get(15)?,
                    telemetry_kind: row.get(16)?,
                })
            })
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|err| AuditError::Sqlite(err.to_string()))?);
        }
        Ok(entries)
    }

    fn insert_leak_suspect(&self, entry: &LeakSuspectLogEntry) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AuditError::Sqlite("sqlite mutex poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO safety_net_log (safety_net_id, raw_label, mapped_class, leak_kind, span_len, document_kind, field_path, score, created_at, session_id, pipeline_class, safety_net_replay_hash, backend_id, backend_version, decoding_params_hash, telemetry_kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                entry.safety_net_id,
                entry.raw_label,
                entry.mapped_class,
                entry.leak_kind,
                entry.span_len,
                entry.document_kind,
                entry.field_path,
                entry.score,
                entry.created_at,
                entry.session_id,
                entry.pipeline_class,
                entry.safety_net_replay_hash,
                entry.backend_id,
                entry.backend_version,
                entry.decoding_params_hash,
                entry.telemetry_kind,
            ],
        )
        .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        Ok(())
    }

    pub fn count_before(&self, before_epoch_ms: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AuditError::Sqlite("sqlite mutex poisoned".to_string()))?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM redaction_log WHERE created_at < ?1",
                params![before_epoch_ms],
                |row| row.get(0),
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        Ok(count as usize)
    }

    pub fn purge_before(&self, before_epoch_ms: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AuditError::Sqlite("sqlite mutex poisoned".to_string()))?;
        let deleted = conn
            .execute(
                "DELETE FROM redaction_log WHERE created_at < ?1",
                params![before_epoch_ms],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        Ok(deleted)
    }
}

impl LeakSuspectLogger for SqliteLogger {
    fn log_leak_suspect(&self, entry: &LeakSuspectLogEntry) -> Result<()> {
        self.insert_leak_suspect(entry)
    }
}

impl gaze_types::RedactionLogger for SqliteLogger {
    fn log(
        &self,
        entry: &gaze_types::RedactionEntry,
    ) -> std::result::Result<(), gaze_types::RedactionLogError> {
        SqliteLogger::log(self, entry)
            .map_err(|err| gaze_types::RedactionLogError::Sqlite(err.to_string()))
    }
}

fn conflict_tier_to_db(tier: ConflictTier) -> &'static str {
    match tier {
        ConflictTier::None => "none",
        ConflictTier::ClassPriority => "class_priority",
        ConflictTier::RulePriority => "rule_priority",
        ConflictTier::Score => "score",
        ConflictTier::SpanLength => "span_length",
        ConflictTier::Validator => "validator",
        ConflictTier::ValidatorVeto => "validator_veto",
        ConflictTier::CollisionPolicy => "collision_policy",
        ConflictTier::AnchoredContext => "anchored_context",
        ConflictTier::RecognizerId => "recognizer_id",
        ConflictTier::Merged => "merged",
        ConflictTier::Redact => "redact",
        ConflictTier::Resolve => "resolve",
        ConflictTier::Fallback => "fallback",
        _ => panic!("unknown variant in audit serialization - update sqlite.rs for new {tier:?}"),
    }
}

fn fallback_reason_to_db(reason: FallbackReason) -> &'static str {
    match reason {
        FallbackReason::OverlapConflict => "overlap_conflict",
        FallbackReason::ValidatorVeto => "validator_veto",
        FallbackReason::AnchorMissing => "anchor_missing",
        FallbackReason::ResidualSuspect => "residual_suspect",
        _ => panic!("unknown variant in audit serialization - update sqlite.rs for new {reason:?}"),
    }
}

fn fallback_reason_from_db(value: &str) -> std::result::Result<FallbackReason, rusqlite::Error> {
    Ok(match value {
        "overlap_conflict" => FallbackReason::OverlapConflict,
        "validator_veto" => FallbackReason::ValidatorVeto,
        "anchor_missing" => FallbackReason::AnchorMissing,
        "residual_suspect" => FallbackReason::ResidualSuspect,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                18,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown fallback reason {other}"),
                )),
            ))
        }
    })
}

fn leak_kind_to_db(kind: &LeakKind) -> &'static str {
    match kind {
        LeakKind::Uncovered => "uncovered",
        LeakKind::PartialBleed { .. } => "partial_bleed",
        LeakKind::ClassMismatch { .. } => "class_mismatch",
        _ => panic!("unknown variant in audit serialization - update sqlite.rs for new {kind:?}"),
    }
}

fn leak_kind_pipeline_class(kind: &LeakKind) -> Option<&PiiClass> {
    match kind {
        LeakKind::ClassMismatch { pipeline_class, .. } => Some(pipeline_class),
        LeakKind::Uncovered | LeakKind::PartialBleed { .. } => None,
        _ => None,
    }
}

fn open_audit_query_connection(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|err| AuditError::Sqlite(err.to_string()))
}

fn table_has_column(conn: &Connection, name: &str) -> Result<bool> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(redaction_log)")
        .map_err(|err| AuditError::Sqlite(err.to_string()))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| AuditError::Sqlite(err.to_string()))?;
    for column in columns {
        if column.map_err(|err| AuditError::Sqlite(err.to_string()))? == name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn conflict_tier_from_db(value: &str) -> std::result::Result<ConflictTier, rusqlite::Error> {
    Ok(match value {
        "none" => ConflictTier::None,
        "class_priority" => ConflictTier::ClassPriority,
        "rule_priority" => ConflictTier::RulePriority,
        "score" => ConflictTier::Score,
        "span_length" => ConflictTier::SpanLength,
        "validator" => ConflictTier::Validator,
        "validator_veto" => ConflictTier::ValidatorVeto,
        "collision_policy" => ConflictTier::CollisionPolicy,
        "anchored_context" => ConflictTier::AnchoredContext,
        "recognizer_id" => ConflictTier::RecognizerId,
        "merged" => ConflictTier::Merged,
        "redact" => ConflictTier::Redact,
        "resolve" => ConflictTier::Resolve,
        "fallback" => ConflictTier::Fallback,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown conflict tier {other}"),
                )),
            ))
        }
    })
}

fn pii_class_from_db(value: &str) -> std::result::Result<PiiClass, rusqlite::Error> {
    PiiClass::from_canonical_str(value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown class {value}"),
            )),
        )
    })
}

fn serialize_json_column<T: Serialize>(value: Option<&T>) -> Result<Option<String>> {
    value
        .map(serde_json::to_string)
        .transpose()
        .map_err(|err| AuditError::Sqlite(err.to_string()))
}

fn deserialize_json_column<T: DeserializeOwned>(
    value: Option<String>,
    column: usize,
) -> std::result::Result<Option<T>, rusqlite::Error> {
    value
        .map(|json| {
            serde_json::from_str(&json).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })
        })
        .transpose()
}

fn action_to_db(action: Action) -> &'static str {
    match action {
        Action::Tokenize => "tokenize",
        Action::Redact => "redact",
        Action::FormatPreserve => "format_preserve",
        Action::Generalize => "generalize",
        Action::Preserve => "preserve",
        _ => panic!("unknown variant in audit serialization - update sqlite.rs for new {action:?}"),
    }
}

fn action_from_db(value: &str) -> std::result::Result<Action, rusqlite::Error> {
    Ok(match value {
        "tokenize" => Action::Tokenize,
        "redact" => Action::Redact,
        "format_preserve" => Action::FormatPreserve,
        "generalize" => Action::Generalize,
        "preserve" => Action::Preserve,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown action {other}"),
                )),
            ))
        }
    })
}

fn document_kind_to_db(kind: &DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Structured => "structured",
        DocumentKind::Text => "text",
        _ => panic!("unknown variant in audit serialization - update sqlite.rs for new {kind:?}"),
    }
}

fn document_kind_from_db(value: &str) -> std::result::Result<DocumentKind, rusqlite::Error> {
    Ok(match value {
        "structured" => DocumentKind::Structured,
        "text" => DocumentKind::Text,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown document kind {other}"),
                )),
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_legacy_redaction_log(path: &Path) {
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
                ('regex', 'email', 'tokenize', 'contact.email', 'structured', 0, 'none', 100, 'session-a');
            "#,
        )
        .expect("legacy schema");
    }

    fn redaction_log_columns(path: &Path) -> Vec<String> {
        let conn = Connection::open(path).expect("sqlite");
        let mut stmt = conn
            .prepare("PRAGMA table_info(redaction_log)")
            .expect("table info");
        stmt.query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("column names")
    }

    #[test]
    fn migration_adds_snapshot_metadata_defaults_to_existing_rows() {
        let temp_db = tempfile::NamedTempFile::new().unwrap();
        create_legacy_redaction_log(temp_db.path());

        let _logger = SqliteLogger::new(temp_db.path()).expect("migrate audit db");

        let conn = Connection::open(temp_db.path()).expect("sqlite");
        let (scheme, alg, key_version): (String, String, Option<i64>) = conn
            .query_row(
                "SELECT snapshot_scheme, snapshot_alg, snapshot_key_version FROM redaction_log",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("snapshot metadata");

        assert_eq!(scheme, DEFAULT_SNAPSHOT_SCHEME);
        assert_eq!(alg, DEFAULT_SNAPSHOT_ALG);
        assert_eq!(key_version, None);
    }

    #[test]
    fn migration_is_idempotent_and_preserves_existing_rows() {
        let temp_db = tempfile::NamedTempFile::new().unwrap();
        create_legacy_redaction_log(temp_db.path());

        let _first = SqliteLogger::new(temp_db.path()).expect("first migration");
        drop(_first);
        let columns_after_first = redaction_log_columns(temp_db.path());

        let _second = SqliteLogger::new(temp_db.path()).expect("second migration");
        drop(_second);
        let columns_after_second = redaction_log_columns(temp_db.path());

        assert_eq!(columns_after_second, columns_after_first);
        assert_eq!(
            columns_after_second
                .iter()
                .filter(|column| column.as_str() == "snapshot_scheme")
                .count(),
            1
        );
        assert_eq!(
            columns_after_second
                .iter()
                .filter(|column| column.as_str() == "snapshot_alg")
                .count(),
            1
        );
        assert_eq!(
            columns_after_second
                .iter()
                .filter(|column| column.as_str() == "snapshot_key_version")
                .count(),
            1
        );

        let conn = Connection::open(temp_db.path()).expect("sqlite");
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM redaction_log", [], |row| row.get(0))
            .expect("row count");
        assert_eq!(row_count, 1);
    }

    #[test]
    fn s4_audit_query_db_is_readonly() {
        let temp_db = tempfile::NamedTempFile::new().unwrap();
        {
            let logger = SqliteLogger::new(temp_db.path()).unwrap();
            logger
                .log(&RedactionEntry::new(
                    "regex",
                    PiiClass::Email,
                    Action::Tokenize,
                    None,
                    DocumentKind::Text,
                    false,
                    ConflictTier::None,
                    0,
                    None,
                ))
                .unwrap();
        }

        let conn = open_audit_query_connection(temp_db.path()).unwrap();
        let err = conn
            .execute(
                "INSERT INTO redaction_log (source, class, action, field_name, document_kind, conflict_loser, decided_by) VALUES ('regex', 'email', 'tokenize', NULL, 'text', 0, 'none')",
                [],
            )
            .expect_err("audit query connection must reject writes");

        assert_eq!(err.sqlite_error_code(), Some(rusqlite::ErrorCode::ReadOnly));
    }
}
