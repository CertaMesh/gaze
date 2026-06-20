pub struct SqliteLogger;
pub struct AuditFilter;
pub struct AuditLogRow;

pub static AUDIT_RESTRICTED_COLUMNS: &[&str] = &["session_blob"];

pub trait AuditSink {}

impl AuditSink for SqliteLogger {}

impl SqliteLogger {
    pub fn new() -> Self {
        Self
    }

    pub fn flush(&self) {}
}

pub fn build_audit_query_sql(_: AuditFilter) -> String {
    String::new()
}
