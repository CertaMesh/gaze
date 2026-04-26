//! Passive audit sinks for Gaze metadata-only redaction logs.
//!
//! This crate owns concrete audit storage and query helpers. It depends on
//! `gaze-types` contracts, and deliberately does not depend on `gaze`.

mod query;
mod sqlite;

pub use query::{build_audit_query_sql, AuditFilter, AuditLogRow, AUDIT_RESTRICTED_COLUMNS};
pub use sqlite::{AuditError, Result, SqliteLogger};
