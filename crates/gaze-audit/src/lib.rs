#![cfg_attr(docsrs, feature(doc_cfg))]

//! Passive audit sinks for Gaze metadata-only redaction logs.
//!
//! This crate owns concrete audit storage and query helpers. It depends on
//! `gaze-types` contracts, and deliberately does not depend on `gaze`.

mod query;
mod sqlite;

pub use query::{
    build_audit_query_sql, build_safety_net_query_sql, AuditFilter, AuditLogRow, LeakSuspectRow,
    AUDIT_RESTRICTED_COLUMNS, DEFAULT_SNAPSHOT_ALG, DEFAULT_SNAPSHOT_SCHEME,
    SAFETY_NET_RESTRICTED_COLUMNS,
};
pub use sqlite::{AuditError, LeakSuspectLogEntry, LeakSuspectLogger, Result, SqliteLogger};
