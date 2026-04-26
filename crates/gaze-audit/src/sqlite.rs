use std::path::Path;
use std::sync::Mutex;

use gaze_types::{Action, ConflictTier, DocumentKind, PiiClass, RedactionEntry};
use rusqlite::{params, params_from_iter, Connection, OpenFlags};
use thiserror::Error;

use crate::query::{build_audit_query_sql, AuditFilter, AuditLogRow};

pub type Result<T> = std::result::Result<T, AuditError>;

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("sqlite error: {0}")]
    Sqlite(String),
}

pub struct SqliteLogger {
    conn: Mutex<Connection>,
}

impl SqliteLogger {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|err| AuditError::Sqlite(err.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS redaction_log (
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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn log(&self, entry: &RedactionEntry) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AuditError::Sqlite("sqlite mutex poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO redaction_log (source, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.source,
                pii_class_to_db(&entry.class),
                action_to_db(entry.action),
                entry.field_name,
                document_kind_to_db(&entry.document_kind),
                if entry.conflict_loser { 1 } else { 0 },
                conflict_tier_to_db(entry.decided_by),
                entry.created_at,
                entry.session_id,
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
                "SELECT source, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id FROM redaction_log",
            )
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RedactionEntry {
                    source: row.get(0)?,
                    class: pii_class_from_db(&row.get::<_, String>(1)?)?,
                    action: action_from_db(&row.get::<_, String>(2)?)?,
                    field_name: row.get(3)?,
                    document_kind: document_kind_from_db(&row.get::<_, String>(4)?)?,
                    conflict_loser: row.get::<_, i64>(5)? != 0,
                    decided_by: conflict_tier_from_db(&row.get::<_, String>(6)?)?,
                    created_at: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    session_id: row.get(8)?,
                })
            })
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|err| AuditError::Sqlite(err.to_string()))?);
        }
        Ok(entries)
    }

    pub fn query(path: &Path, filter: &AuditFilter) -> Result<Vec<AuditLogRow>> {
        let conn = open_audit_query_connection(path)?;
        let has_decided_by = table_has_column(&conn, "decided_by")?;
        let has_created_at = table_has_column(&conn, "created_at")?;
        let has_session_id = table_has_column(&conn, "session_id")?;
        let (sql, values) =
            build_audit_query_sql(filter, has_decided_by, has_created_at, has_session_id);
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;
        let rows = stmt
            .query_map(params_from_iter(values.iter()), |row| {
                Ok(AuditLogRow {
                    source: row.get(0)?,
                    class: row.get(1)?,
                    action: row.get(2)?,
                    field_name: row.get(3)?,
                    document_kind: row.get(4)?,
                    conflict_loser: row.get::<_, i64>(5)? != 0,
                    decided_by: row.get(6)?,
                    created_at: row.get(7)?,
                    session_id: row.get(8)?,
                })
            })
            .map_err(|err| AuditError::Sqlite(err.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|err| AuditError::Sqlite(err.to_string()))?);
        }
        Ok(entries)
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

fn conflict_tier_to_db(tier: ConflictTier) -> &'static str {
    match tier {
        ConflictTier::None => "none",
        ConflictTier::ClassPriority => "class_priority",
        ConflictTier::RulePriority => "rule_priority",
        ConflictTier::Score => "score",
        ConflictTier::SpanLength => "span_length",
        ConflictTier::Validator => "validator",
        ConflictTier::RecognizerId => "recognizer_id",
        ConflictTier::Merged => "merged",
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
        "recognizer_id" => ConflictTier::RecognizerId,
        "merged" => ConflictTier::Merged,
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

fn pii_class_to_db(class: &PiiClass) -> String {
    match class {
        PiiClass::Email => "email".to_string(),
        PiiClass::Name => "name".to_string(),
        PiiClass::Location => "location".to_string(),
        PiiClass::Organization => "organization".to_string(),
        PiiClass::Custom(name) => format!("custom:{name}"),
    }
}

fn pii_class_from_db(value: &str) -> std::result::Result<PiiClass, rusqlite::Error> {
    Ok(match value {
        "email" => PiiClass::Email,
        "name" => PiiClass::Name,
        "location" => PiiClass::Location,
        "organization" => PiiClass::Organization,
        custom if custom.starts_with("custom:") => PiiClass::Custom(custom[7..].to_string()),
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown class {other}"),
                )),
            ))
        }
    })
}

fn action_to_db(action: Action) -> &'static str {
    match action {
        Action::Tokenize => "tokenize",
        Action::Redact => "redact",
        Action::FormatPreserve => "format_preserve",
        Action::Generalize => "generalize",
        Action::Preserve => "preserve",
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

    #[test]
    fn s4_audit_query_db_is_readonly() {
        let temp_db = tempfile::NamedTempFile::new().unwrap();
        {
            let logger = SqliteLogger::new(temp_db.path()).unwrap();
            logger
                .log(&RedactionEntry {
                    source: "regex".to_string(),
                    class: PiiClass::Email,
                    action: Action::Tokenize,
                    field_name: None,
                    document_kind: DocumentKind::Text,
                    conflict_loser: false,
                    decided_by: ConflictTier::None,
                    created_at: 0,
                    session_id: None,
                })
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
