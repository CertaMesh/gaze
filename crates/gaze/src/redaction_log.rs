use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, params_from_iter, Connection, OpenFlags};

use crate::detector::PiiClass;
use crate::rule::Action;
use crate::Result;

pub trait RedactionLogger: Send + Sync {
    fn log(&self, entry: &RedactionEntry) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentKind {
    Structured,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionEntry {
    pub source: String,
    pub class: PiiClass,
    pub action: Action,
    pub field_name: Option<String>,
    pub document_kind: DocumentKind,
    pub conflict_loser: bool,
    pub decided_by: ConflictTier,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditFilter {
    pub class: Option<String>,
    pub source: Option<String>,
    pub action: Option<String>,
    pub document_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditLogRow {
    pub source: String,
    pub class: String,
    pub action: String,
    pub field_name: Option<String>,
    pub document_kind: String,
    pub conflict_loser: bool,
    pub decided_by: String,
}

#[allow(dead_code)]
pub const AUDIT_RESTRICTED_COLUMNS: &[&str] = &[
    "source",
    "class",
    "action",
    "field_name",
    "document_kind",
    "conflict_loser",
    "decided_by",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictTier {
    None,
    ClassPriority,
    RulePriority,
    Score,
    SpanLength,
    Validator,
    RecognizerId,
    Merged,
}

/// `RedactionEntry` must remain metadata-only.
///
/// ```compile_fail
/// use gaze::{Action, DocumentKind, PiiClass, RedactionEntry};
///
/// let _entry = RedactionEntry {
///     source: "regex".to_string(),
///     class: PiiClass::Email,
///     action: Action::Tokenize,
///     field_name: None,
///     document_kind: DocumentKind::Text,
///     conflict_loser: false,
///     decided_by: gaze::ConflictTier::None,
///     raw: Some("alice@example.com".to_string()),
/// };
/// ```
const _: () = ();

pub struct SqliteLogger {
    conn: Mutex<Connection>,
}

impl SqliteLogger {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|err| crate::Error::Sqlite(err.to_string()))?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS redaction_log (
                source TEXT NOT NULL,
                class TEXT NOT NULL,
                action TEXT NOT NULL,
                field_name TEXT NULL,
                document_kind TEXT NOT NULL,
                conflict_loser INTEGER NOT NULL,
                decided_by TEXT NOT NULL DEFAULT 'none'
            );
            "#,
        )
        .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
        let has_decided_by = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(redaction_log)")
                .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
            let columns = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
            let mut found = false;
            for column in columns {
                if column.map_err(|err| crate::Error::Sqlite(err.to_string()))? == "decided_by" {
                    found = true;
                    break;
                }
            }
            found
        };
        if !has_decided_by {
            conn.execute(
                "ALTER TABLE redaction_log ADD COLUMN decided_by TEXT NOT NULL DEFAULT 'none'",
                [],
            )
            .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn entries(&self) -> Result<Vec<RedactionEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| crate::Error::Sqlite("sqlite mutex poisoned".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT source, class, action, field_name, document_kind, conflict_loser, decided_by FROM redaction_log",
            )
            .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
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
                })
            })
            .map_err(|err| crate::Error::Sqlite(err.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|err| crate::Error::Sqlite(err.to_string()))?);
        }
        Ok(entries)
    }

    pub fn query(path: &Path, filter: &AuditFilter) -> Result<Vec<AuditLogRow>> {
        let conn = open_audit_query_connection(path)?;
        let has_decided_by = table_has_column(&conn, "decided_by")?;
        let (sql, values) = build_audit_query_sql(filter, has_decided_by);
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
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
                })
            })
            .map_err(|err| crate::Error::Sqlite(err.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|err| crate::Error::Sqlite(err.to_string()))?);
        }
        Ok(entries)
    }
}

impl RedactionLogger for SqliteLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| crate::Error::Sqlite("sqlite mutex poisoned".to_string()))?;
        conn.execute(
            "INSERT INTO redaction_log (source, class, action, field_name, document_kind, conflict_loser, decided_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.source,
                pii_class_to_db(&entry.class),
                action_to_db(entry.action),
                entry.field_name,
                document_kind_to_db(&entry.document_kind),
                if entry.conflict_loser { 1 } else { 0 },
                conflict_tier_to_db(entry.decided_by),
            ],
        )
        .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
        Ok(())
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

/// Audit reads are defense-in-depth restricted to metadata columns that are
/// safe to display. Do not switch this path to `SELECT *`; future schema
/// additions may include restore material or other sensitive payloads.
pub fn build_audit_query_sql(filter: &AuditFilter, has_decided_by: bool) -> (String, Vec<String>) {
    let decided_by_column = if has_decided_by {
        "decided_by"
    } else {
        "'none' AS decided_by"
    };
    let mut sql = format!(
        "SELECT source, class, action, field_name, document_kind, conflict_loser, {decided_by_column} FROM redaction_log"
    );
    let mut predicates = Vec::new();
    let mut values = Vec::new();
    if let Some(class) = &filter.class {
        predicates.push("class = ?");
        values.push(class.clone());
    }
    if let Some(source) = &filter.source {
        predicates.push("source = ?");
        values.push(source.clone());
    }
    if let Some(action) = &filter.action {
        predicates.push("action = ?");
        values.push(action.clone());
    }
    if let Some(document_kind) = &filter.document_kind {
        predicates.push("document_kind = ?");
        values.push(document_kind.clone());
    }
    if !predicates.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&predicates.join(" AND "));
    }
    sql.push_str(" ORDER BY rowid");
    (sql, values)
}

fn open_audit_query_connection(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|err| crate::Error::Sqlite(err.to_string()))
}

fn table_has_column(conn: &Connection, name: &str) -> Result<bool> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(redaction_log)")
        .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| crate::Error::Sqlite(err.to_string()))?;
    for column in columns {
        if column.map_err(|err| crate::Error::Sqlite(err.to_string()))? == name {
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
