use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

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
        if let Err(err) = conn.execute(
            "ALTER TABLE redaction_log ADD COLUMN decided_by TEXT NOT NULL DEFAULT 'none'",
            [],
        ) {
            let message = err.to_string();
            if !message.contains("duplicate column name") {
                return Err(crate::Error::Sqlite(message));
            }
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
