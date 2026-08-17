use std::collections::BTreeSet;
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
    PresentColumns, DEFAULT_SNAPSHOT_ALG, DEFAULT_SNAPSHOT_SCHEME,
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
/// in `docs/explanation/`.
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
            backend_id: Some(suspect.safety_net_id.clone()),
            backend_version: None,
            decoding_params_hash: None,
            telemetry_kind: None,
        }
    }
}

pub trait LeakSuspectLogger: Send + Sync {
    fn log_leak_suspect(&self, entry: &LeakSuspectLogEntry) -> Result<()>;
}

#[derive(Clone, Copy)]
enum ColumnConstraint {
    NotNull,
    Nullable,
    Default(&'static str),
}

struct ColumnSpec {
    name: &'static str,
    sql_type: &'static str,
    constraint: ColumnConstraint,
    migrate: bool,
    backfill: Option<&'static str>,
}

impl ColumnSpec {
    fn declaration(&self) -> String {
        match self.constraint {
            ColumnConstraint::NotNull => format!("{} NOT NULL", self.sql_type),
            ColumnConstraint::Nullable => format!("{} NULL", self.sql_type),
            ColumnConstraint::Default(value) => {
                format!("{} NOT NULL DEFAULT '{value}'", self.sql_type)
            }
        }
    }
}

const REDACTION_LOG_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec { name: "source", sql_type: "TEXT", constraint: ColumnConstraint::NotNull, migrate: false, backfill: None },
    ColumnSpec { name: "recognizer_id", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: Some("UPDATE redaction_log SET recognizer_id = 'legacy_unversioned' WHERE recognizer_id IS NULL") },
    ColumnSpec { name: "recognizer_version_id", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "class", sql_type: "TEXT", constraint: ColumnConstraint::NotNull, migrate: false, backfill: None },
    ColumnSpec { name: "action", sql_type: "TEXT", constraint: ColumnConstraint::NotNull, migrate: false, backfill: None },
    ColumnSpec { name: "field_name", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: false, backfill: None },
    ColumnSpec { name: "document_kind", sql_type: "TEXT", constraint: ColumnConstraint::NotNull, migrate: false, backfill: None },
    ColumnSpec { name: "conflict_loser", sql_type: "INTEGER", constraint: ColumnConstraint::NotNull, migrate: false, backfill: None },
    ColumnSpec { name: "decided_by", sql_type: "TEXT", constraint: ColumnConstraint::Default("none"), migrate: true, backfill: None },
    ColumnSpec { name: "created_at", sql_type: "INTEGER", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "session_id", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "snapshot_scheme", sql_type: "TEXT", constraint: ColumnConstraint::Default(DEFAULT_SNAPSHOT_SCHEME), migrate: true, backfill: None },
    ColumnSpec { name: "snapshot_alg", sql_type: "TEXT", constraint: ColumnConstraint::Default(DEFAULT_SNAPSHOT_ALG), migrate: true, backfill: None },
    ColumnSpec { name: "snapshot_key_version", sql_type: "INTEGER", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "validator_fail_reason", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "ambiguity_record", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "collision_family", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "collision_variant", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "fallback_triggered", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "backend_silently_dropped", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_stage", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_model_id", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_model_version", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_artifact_sha256", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_tokenizer_sha256", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_locale_resolved", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_locale_match_kind", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_canonical_class", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_native_class", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_confidence", sql_type: "REAL", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "provenance_merged_from", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "restore_policy", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "restore_decision", sql_type: "TEXT", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "restore_unknown_token_count", sql_type: "INTEGER", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "restore_manifest_bypass_count", sql_type: "INTEGER", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "restore_fresh_pii_count", sql_type: "INTEGER", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
    ColumnSpec { name: "restore_phase_mask", sql_type: "INTEGER", constraint: ColumnConstraint::Nullable, migrate: true, backfill: None },
];
impl SqliteLogger {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|err| AuditError::Sqlite(err.to_string()))?;
        let redaction_columns = REDACTION_LOG_COLUMNS
            .iter()
            .map(|column| format!("{} {}", column.name, column.declaration()))
            .collect::<Vec<_>>()
            .join(",\n                ");
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS redaction_log (\n                {redaction_columns}\n            );"
        ))
        .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        conn.execute_batch(
            r#"
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

        let columns = redaction_log_column_names(&conn)?;
        for column in REDACTION_LOG_COLUMNS
            .iter()
            .filter(|column| column.migrate && !columns.contains(column.name))
        {
            conn.execute(
                &format!(
                    "ALTER TABLE redaction_log ADD COLUMN {} {}",
                    column.name,
                    column.declaration()
                ),
                [],
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
            if let Some(backfill) = column.backfill {
                conn.execute(backfill, [])
                    .map_err(|err| AuditError::Sqlite(err.to_string()))?;
            }
        }

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn log(&self, entry: &RedactionEntry) -> Result<()> {
        let validator_fail_reason = serialize_json_column(entry.validator_fail_reason.as_ref())?;
        let ambiguity_record = serialize_json_column(entry.ambiguity_record.as_ref())?;
        let fallback_triggered = entry.fallback_triggered.map(fallback_reason_to_db);
        let provenance_confidence = entry
            .provenance_confidence
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok());
        let backend_silently_dropped =
            serialize_json_column(entry.backend_silently_dropped.as_ref())?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| AuditError::Sqlite("sqlite mutex poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO redaction_log (source, recognizer_id, recognizer_version_id, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id, validator_fail_reason, ambiguity_record, collision_family, collision_variant, fallback_triggered, provenance_stage, provenance_model_id, provenance_model_version, provenance_artifact_sha256, provenance_tokenizer_sha256, provenance_locale_resolved, provenance_locale_match_kind, provenance_canonical_class, provenance_native_class, provenance_confidence, provenance_merged_from, backend_silently_dropped, restore_policy, restore_decision, restore_unknown_token_count, restore_manifest_bypass_count, restore_fresh_pii_count, restore_phase_mask) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34)",
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
                entry.provenance_stage,
                entry.provenance_model_id,
                entry.provenance_model_version,
                entry.provenance_artifact_sha256,
                entry.provenance_tokenizer_sha256,
                entry.provenance_locale_resolved,
                entry.provenance_locale_match_kind,
                entry.provenance_canonical_class,
                entry.provenance_native_class,
                provenance_confidence,
                entry.provenance_merged_from,
                backend_silently_dropped,
                entry.restore_policy,
                entry.restore_decision,
                entry.restore_unknown_token_count.map(|value| value as i64),
                entry.restore_manifest_bypass_count.map(|value| value as i64),
                entry.restore_fresh_pii_count.map(|value| value as i64),
                entry.restore_phase_mask.map(i64::from),
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
                "SELECT source, recognizer_id, recognizer_version_id, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id, validator_fail_reason, ambiguity_record, collision_family, collision_variant, fallback_triggered, backend_silently_dropped, restore_policy, restore_decision, restore_unknown_token_count, restore_manifest_bypass_count, restore_fresh_pii_count, restore_phase_mask FROM redaction_log",
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
                if let Some(dropped) = deserialize_json_column::<Vec<String>>(row.get(16)?, 16)? {
                    entry = entry.with_backend_silently_dropped(dropped);
                }
                entry.restore_policy = row.get(17)?;
                entry.restore_decision = row.get(18)?;
                entry.restore_unknown_token_count =
                    row.get::<_, Option<i64>>(19)?.map(|value| value as u64);
                entry.restore_manifest_bypass_count =
                    row.get::<_, Option<i64>>(20)?.map(|value| value as u64);
                entry.restore_fresh_pii_count =
                    row.get::<_, Option<i64>>(21)?.map(|value| value as u64);
                entry.restore_phase_mask = row.get::<_, Option<i64>>(22)?.map(|value| value as u32);
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
        let present_columns = PresentColumns::new(redaction_log_column_names(&conn)?);
        let (sql, values) = build_audit_query_sql(filter, &present_columns);
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
                    provenance_stage: row.get(19)?,
                    provenance_model_id: row.get(20)?,
                    provenance_model_version: row.get(21)?,
                    provenance_artifact_sha256: row.get(22)?,
                    provenance_tokenizer_sha256: row.get(23)?,
                    provenance_locale_resolved: row.get(24)?,
                    provenance_locale_match_kind: row.get(25)?,
                    provenance_canonical_class: row.get(26)?,
                    provenance_native_class: row.get(27)?,
                    provenance_confidence: row.get(28)?,
                    provenance_merged_from: row.get(29)?,
                    restore_policy: row.get(30)?,
                    restore_decision: row.get(31)?,
                    restore_unknown_token_count: row.get(32)?,
                    restore_manifest_bypass_count: row.get(33)?,
                    restore_fresh_pii_count: row.get(34)?,
                    restore_phase_mask: row.get(35)?,
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
    tier.as_str()
}

fn fallback_reason_to_db(reason: FallbackReason) -> &'static str {
    reason.as_str()
}

fn fallback_reason_from_db(value: &str) -> std::result::Result<FallbackReason, rusqlite::Error> {
    FallbackReason::from_canonical_str(value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            18,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown fallback reason {value}"),
            )),
        )
    })
}

fn leak_kind_to_db(kind: &LeakKind) -> &'static str {
    kind.as_str()
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

fn redaction_log_column_names(conn: &Connection) -> Result<BTreeSet<String>> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(redaction_log)")
        .map_err(|err| AuditError::Sqlite(err.to_string()))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| AuditError::Sqlite(err.to_string()))?;
    let mut names = BTreeSet::new();
    for column in columns {
        names.insert(column.map_err(|err| AuditError::Sqlite(err.to_string()))?);
    }
    Ok(names)
}

fn conflict_tier_from_db(value: &str) -> std::result::Result<ConflictTier, rusqlite::Error> {
    ConflictTier::from_canonical_str(value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            6,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown conflict tier {value}"),
            )),
        )
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
    action.as_str()
}

fn action_from_db(value: &str) -> std::result::Result<Action, rusqlite::Error> {
    Action::from_canonical_str(value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown action {value}"),
            )),
        )
    })
}

fn document_kind_to_db(kind: &DocumentKind) -> &'static str {
    kind.as_str()
}

fn document_kind_from_db(value: &str) -> std::result::Result<DocumentKind, rusqlite::Error> {
    DocumentKind::from_canonical_str(value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown document kind {value}"),
            )),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_types::{
        RestoreDecision, RestorePolicy, RestoreTelemetry, RESTORE_PHASE_MANIFEST_BYPASS_SCAN,
        RESTORE_PHASE_MANIFEST_LOOKUP, RESTORE_PHASE_UNKNOWN_TOKEN_SCAN,
    };

    #[test]
    fn sqlite_sink_uses_each_audit_enums_canonical_spelling() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let logger = SqliteLogger::new(temp_db.path()).expect("logger");

        for action in [
            Action::Tokenize,
            Action::Redact,
            Action::FormatPreserve,
            Action::Generalize,
            Action::Preserve,
        ] {
            assert_logged_column(
                &logger,
                temp_db.path(),
                RedactionEntry::new(
                    format!("action:{}", action.as_str()),
                    PiiClass::Email,
                    action,
                    None,
                    DocumentKind::Text,
                    false,
                    ConflictTier::None,
                    0,
                    None,
                ),
                "action",
                action.as_str(),
            );
        }
        for tier in [
            ConflictTier::None,
            ConflictTier::ClassPriority,
            ConflictTier::RulePriority,
            ConflictTier::Score,
            ConflictTier::SpanLength,
            ConflictTier::Validator,
            ConflictTier::ValidatorVeto,
            ConflictTier::CollisionPolicy,
            ConflictTier::AnchoredContext,
            ConflictTier::RecognizerId,
            ConflictTier::Merged,
            ConflictTier::Redact,
            ConflictTier::Resolve,
            ConflictTier::Fallback,
        ] {
            assert_logged_column(
                &logger,
                temp_db.path(),
                RedactionEntry::new(
                    format!("tier:{}", tier.as_str()),
                    PiiClass::Email,
                    Action::Tokenize,
                    None,
                    DocumentKind::Text,
                    false,
                    tier,
                    0,
                    None,
                ),
                "decided_by",
                tier.as_str(),
            );
        }
        for kind in [DocumentKind::Structured, DocumentKind::Text] {
            assert_logged_column(
                &logger,
                temp_db.path(),
                RedactionEntry::new(
                    format!("kind:{}", kind.as_str()),
                    PiiClass::Email,
                    Action::Tokenize,
                    None,
                    kind,
                    false,
                    ConflictTier::None,
                    0,
                    None,
                ),
                "document_kind",
                kind.as_str(),
            );
        }
        for reason in [
            FallbackReason::OverlapConflict,
            FallbackReason::ValidatorVeto,
            FallbackReason::AnchorMissing,
            FallbackReason::ResidualSuspect,
        ] {
            assert_logged_column(
                &logger,
                temp_db.path(),
                RedactionEntry::new(
                    format!("fallback:{}", reason.as_str()),
                    PiiClass::Email,
                    Action::Tokenize,
                    None,
                    DocumentKind::Text,
                    false,
                    ConflictTier::None,
                    0,
                    None,
                )
                .with_fallback_triggered(reason),
                "fallback_triggered",
                reason.as_str(),
            );
        }
    }

    fn assert_logged_column(
        logger: &SqliteLogger,
        path: &Path,
        entry: RedactionEntry,
        column: &str,
        expected: &str,
    ) {
        let source = entry.source.clone();
        logger.log(&entry).expect("write audit row");
        let conn = Connection::open(path).expect("open audit db");
        let actual: String = conn
            .query_row(
                &format!("SELECT {column} FROM redaction_log WHERE source = ?1"),
                [&source],
                |row| row.get(0),
            )
            .expect("read canonical enum column");
        assert_eq!(actual, expected);
    }

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
    fn fresh_and_legacy_migrated_schemas_have_the_same_columns() {
        let fresh_db = tempfile::NamedTempFile::new().unwrap();
        let _fresh = SqliteLogger::new(fresh_db.path()).expect("fresh schema");

        let legacy_db = tempfile::NamedTempFile::new().unwrap();
        create_legacy_redaction_log(legacy_db.path());
        let _migrated = SqliteLogger::new(legacy_db.path()).expect("migrated schema");

        let fresh_columns = redaction_log_columns(fresh_db.path())
            .into_iter()
            .collect::<BTreeSet<_>>();
        let migrated_columns = redaction_log_columns(legacy_db.path())
            .into_iter()
            .collect::<BTreeSet<_>>();

        assert_eq!(migrated_columns, fresh_columns);
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

    #[test]
    fn restore_telemetry_persists_metadata_only_columns() {
        let temp_db = tempfile::NamedTempFile::new().unwrap();
        let logger = SqliteLogger::new(temp_db.path()).unwrap();
        let mut telemetry = RestoreTelemetry::new(RestorePolicy::Lenient);
        telemetry.unknown_token_count = 2;
        telemetry.manifest_bypass_count = 2;
        telemetry.fresh_pii_detected_count = 0;
        telemetry.restore_decision = RestoreDecision::Partial;
        telemetry.phase_execution_mask = RESTORE_PHASE_MANIFEST_LOOKUP
            | RESTORE_PHASE_UNKNOWN_TOKEN_SCAN
            | RESTORE_PHASE_MANIFEST_BYPASS_SCAN;

        logger
            .log(
                &RedactionEntry::new(
                    "restore",
                    PiiClass::Custom("restore.telemetry".to_string()),
                    Action::Preserve,
                    None,
                    DocumentKind::Text,
                    false,
                    ConflictTier::None,
                    0,
                    Some("audit-session".to_string()),
                )
                .with_restore_telemetry(telemetry.clone()),
            )
            .unwrap();

        let rows = SqliteLogger::query(
            temp_db.path(),
            &AuditFilter {
                restore_events_only: true,
                ..AuditFilter::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].restore_policy.as_deref(), Some("lenient"));
        assert_eq!(rows[0].restore_decision.as_deref(), Some("partial"));
        assert_eq!(rows[0].restore_unknown_token_count, Some(2));
        assert_eq!(rows[0].restore_manifest_bypass_count, Some(2));
        assert_eq!(rows[0].restore_fresh_pii_count, Some(0));
        assert_eq!(
            rows[0].restore_phase_mask,
            Some(i64::from(
                RESTORE_PHASE_MANIFEST_LOOKUP
                    | RESTORE_PHASE_UNKNOWN_TOKEN_SCAN
                    | RESTORE_PHASE_MANIFEST_BYPASS_SCAN
            ))
        );
        assert!(rows[0].field_name.is_none());
        assert_eq!(rows[0].source, "restore");
    }
}
