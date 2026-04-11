//! SQLite-backed audit log. One row per MCP request. No raw PII — only
//! tool name, structured request (tokens OK), decision, duration, result
//! shape (row count, column count).

use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct AuditLog {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct AuditEntry<'a> {
    pub tool: &'a str,
    pub request_json: &'a str,
    pub decision: &'a str, // "allow" | "deny"
    pub reason: Option<&'a str>,
    pub duration_ms: u64,
    pub result_rows: Option<u64>,
    pub result_columns: Option<u64>,
}

impl AuditLog {
    pub fn open(path: &Path) -> Result<Self, AuditError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                tool TEXT NOT NULL,
                -- structured request (tokens, not raw PII; filter values on
                -- PII columns are session tokens)
                request_json TEXT NOT NULL,
                decision TEXT NOT NULL,
                reason TEXT,
                duration_ms INTEGER NOT NULL,
                result_rows INTEGER,
                result_columns INTEGER
            );
            CREATE INDEX IF NOT EXISTS audit_log_ts_idx ON audit_log(ts);
            CREATE INDEX IF NOT EXISTS audit_log_tool_idx ON audit_log(tool);
            "#,
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn append(&self, entry: AuditEntry<'_>) -> Result<(), AuditError> {
        let conn = self.conn.lock().expect("audit log poisoned");
        conn.execute(
            "INSERT INTO audit_log (tool, request_json, decision, reason, duration_ms, result_rows, result_columns)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                entry.tool,
                entry.request_json,
                entry.decision,
                entry.reason,
                entry.duration_ms as i64,
                entry.result_rows.map(|n| n as i64),
                entry.result_columns.map(|n| n as i64),
            ],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<u64, AuditError> {
        let conn = self.conn.lock().expect("audit log poisoned");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM audit_log", [], |r| r.get(0))?;
        Ok(n.max(0) as u64)
    }
}

pub fn default_path(global: bool) -> PathBuf {
    if global {
        dirs_home_join([".gaze", "audit.db"])
    } else {
        PathBuf::from(".gaze/audit.db")
    }
}

fn dirs_home_join<const N: usize>(parts: [&str; N]) -> PathBuf {
    let mut p = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    for part in parts {
        p.push(part);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_creates_schema_and_append_works() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".gaze/audit.db");
        let log = AuditLog::open(&path).unwrap();
        log.append(AuditEntry {
            tool: "db.sample",
            request_json: r#"{"table":"users","limit":5}"#,
            decision: "allow",
            reason: None,
            duration_ms: 12,
            result_rows: Some(5),
            result_columns: Some(4),
        })
        .unwrap();
        assert_eq!(log.count().unwrap(), 1);
    }

    #[test]
    fn default_path_honors_global_flag() {
        let local = default_path(false);
        assert!(local.ends_with(".gaze/audit.db"));
        let global = default_path(true);
        assert!(global.to_string_lossy().contains(".gaze"));
    }
}
