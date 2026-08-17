pub const PROTECTED_PATHS: &[&str] = &[
    "crates/gaze-cli/src/restore",
    "crates/gaze/src",
    "crates/gaze-mcp-core/src",
];

pub const FORBIDDEN_CRATES: &[&str] = &["gaze_audit"];

pub const FORBIDDEN_ITEMS: &[&str] = &[
    "gaze_audit::SqliteLogger",
    "gaze_audit::AuditFilter",
    "gaze_audit::AuditLogRow",
    "gaze_audit::build_audit_query_sql",
    "gaze_audit::AUDIT_RESTRICTED_COLUMNS",
];
