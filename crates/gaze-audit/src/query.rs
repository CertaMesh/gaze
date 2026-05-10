use rusqlite::types::Value;

pub const DEFAULT_SNAPSHOT_SCHEME: &str = "gaze.snapshot.v1.sha256-salted";
pub const DEFAULT_SNAPSHOT_ALG: &str = "SHA-256";

/// Query filter for [`crate::SqliteLogger::query`] and
/// [`crate::SqliteLogger::query_safety_net`].
///
/// Construct with `AuditFilter::default()` for all rows, or set fields to
/// narrow by class, source, action, document kind, raw safety-net label, field
/// path, epoch-millisecond time range, or session ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditFilter {
    pub class: Option<String>,
    pub source: Option<String>,
    pub action: Option<String>,
    pub document_kind: Option<String>,
    pub raw_label: Option<String>,
    pub field_path: Option<String>,
    pub from_epoch_ms: Option<i64>,
    pub to_epoch_ms: Option<i64>,
    pub session_id: Option<String>,
    pub snapshot_scheme: Option<String>,
    pub snapshot_alg: Option<String>,
    pub snapshot_key_version: Option<i64>,
}

/// Metadata-only redaction audit row returned by [`crate::SqliteLogger::query`].
///
/// This row mirrors the approved audit export surface. It is safe for audit
/// display and reporting, but it is not restore material and contains no
/// original PII or token values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditLogRow {
    pub source: String,
    pub class: String,
    pub action: String,
    pub field_name: Option<String>,
    pub document_kind: String,
    pub conflict_loser: bool,
    pub decided_by: String,
    pub created_at: Option<i64>,
    pub session_id: Option<String>,
    pub snapshot_scheme: String,
    pub snapshot_alg: String,
    pub snapshot_key_version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeakSuspectRow {
    pub id: i64,
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

/// The approved metadata-only column set exported by `audit export` and queried
/// by [`crate::SqliteLogger::query`].
///
/// Columns that would expose raw PII, token values, or document content are
/// excluded. The `audit export` CLI command selects only from this set, and the
/// `gaze_module_isolation` Dylint lint prevents the clean path from routing raw
/// values into the audit path.
pub const AUDIT_RESTRICTED_COLUMNS: &[&str] = &[
    "source",
    "class",
    "action",
    "field_name",
    "document_kind",
    "conflict_loser",
    "decided_by",
    "created_at",
    "session_id",
    "snapshot_scheme",
    "snapshot_alg",
    "snapshot_key_version",
];

pub const SAFETY_NET_RESTRICTED_COLUMNS: &[&str] = &[
    "id",
    "safety_net_id",
    "raw_label",
    "mapped_class",
    "leak_kind",
    "span_len",
    "document_kind",
    "field_path",
    "score",
    "created_at",
    "session_id",
    "pipeline_class",
    "safety_net_replay_hash",
    "backend_id",
    "backend_version",
    "decoding_params_hash",
    "telemetry_kind",
];

/// Audit reads are defense-in-depth restricted to metadata columns that are
/// safe to display. Do not switch this path to `SELECT *`; future schema
/// additions may include restore material or other sensitive payloads.
pub fn build_audit_query_sql(
    filter: &AuditFilter,
    has_decided_by: bool,
    has_created_at: bool,
    has_session_id: bool,
    has_snapshot_scheme: bool,
    has_snapshot_alg: bool,
    has_snapshot_key_version: bool,
) -> (String, Vec<Value>) {
    let decided_by_column = if has_decided_by {
        "decided_by"
    } else {
        "'none' AS decided_by"
    };
    let created_at_column = if has_created_at {
        "created_at"
    } else {
        "NULL AS created_at"
    };
    let session_id_column = if has_session_id {
        "session_id"
    } else {
        "NULL AS session_id"
    };
    let snapshot_scheme_column = if has_snapshot_scheme {
        "snapshot_scheme".to_string()
    } else {
        format!("'{DEFAULT_SNAPSHOT_SCHEME}' AS snapshot_scheme")
    };
    let snapshot_alg_column = if has_snapshot_alg {
        "snapshot_alg".to_string()
    } else {
        format!("'{DEFAULT_SNAPSHOT_ALG}' AS snapshot_alg")
    };
    let snapshot_key_version_column = if has_snapshot_key_version {
        "snapshot_key_version"
    } else {
        "NULL AS snapshot_key_version"
    };
    let mut sql = format!(
        "SELECT source, class, action, field_name, document_kind, conflict_loser, {decided_by_column}, {created_at_column}, {session_id_column}, {snapshot_scheme_column}, {snapshot_alg_column}, {snapshot_key_version_column} FROM redaction_log"
    );
    let mut predicates = Vec::new();
    let mut values = Vec::new();
    if let Some(class) = &filter.class {
        predicates.push("class = ?");
        values.push(Value::Text(class.clone()));
    }
    if let Some(source) = &filter.source {
        predicates.push("source = ?");
        values.push(Value::Text(source.clone()));
    }
    if let Some(action) = &filter.action {
        predicates.push("action = ?");
        values.push(Value::Text(action.clone()));
    }
    if let Some(document_kind) = &filter.document_kind {
        predicates.push("document_kind = ?");
        values.push(Value::Text(document_kind.clone()));
    }
    if let Some(from_epoch_ms) = filter.from_epoch_ms {
        if has_created_at {
            predicates.push("created_at >= ?");
        } else {
            predicates.push("NULL >= ?");
        }
        values.push(Value::Integer(from_epoch_ms));
    }
    if let Some(to_epoch_ms) = filter.to_epoch_ms {
        if has_created_at {
            predicates.push("created_at <= ?");
        } else {
            predicates.push("NULL <= ?");
        }
        values.push(Value::Integer(to_epoch_ms));
    }
    if let Some(session_id) = &filter.session_id {
        if has_session_id {
            predicates.push("session_id = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Text(session_id.clone()));
    }
    if let Some(snapshot_scheme) = &filter.snapshot_scheme {
        if has_snapshot_scheme {
            predicates.push("snapshot_scheme = ?");
        } else {
            predicates.push("'gaze.snapshot.v1.sha256-salted' = ?");
        }
        values.push(Value::Text(snapshot_scheme.clone()));
    }
    if let Some(snapshot_alg) = &filter.snapshot_alg {
        if has_snapshot_alg {
            predicates.push("snapshot_alg = ?");
        } else {
            predicates.push("'SHA-256' = ?");
        }
        values.push(Value::Text(snapshot_alg.clone()));
    }
    if let Some(snapshot_key_version) = filter.snapshot_key_version {
        if has_snapshot_key_version {
            predicates.push("snapshot_key_version = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Integer(snapshot_key_version));
    }
    if !predicates.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&predicates.join(" AND "));
    }
    sql.push_str(" ORDER BY rowid");
    (sql, values)
}

/// Safety-net reads are restricted to bytes-free metadata columns. The table
/// intentionally stores labels, classes, lengths, and replay hashes, never the
/// suspect text or emitted placeholder bytes.
pub fn build_safety_net_query_sql(filter: &AuditFilter) -> (String, Vec<Value>) {
    let mut sql = format!(
        "SELECT {} FROM safety_net_log",
        SAFETY_NET_RESTRICTED_COLUMNS.join(", ")
    );
    let mut predicates = Vec::new();
    let mut values = Vec::new();
    if let Some(class) = &filter.class {
        predicates.push("mapped_class = ?");
        values.push(Value::Text(class.clone()));
    }
    if let Some(source) = &filter.source {
        predicates.push("safety_net_id = ?");
        values.push(Value::Text(source.clone()));
    }
    if let Some(action) = &filter.action {
        predicates.push("leak_kind = ?");
        values.push(Value::Text(action.clone()));
    }
    if let Some(raw_label) = &filter.raw_label {
        predicates.push("raw_label = ?");
        values.push(Value::Text(raw_label.clone()));
    }
    if let Some(document_kind) = &filter.document_kind {
        predicates.push("document_kind = ?");
        values.push(Value::Text(document_kind.clone()));
    }
    if let Some(field_path) = &filter.field_path {
        predicates.push("field_path = ?");
        values.push(Value::Text(field_path.clone()));
    }
    if let Some(from_epoch_ms) = filter.from_epoch_ms {
        predicates.push("created_at >= ?");
        values.push(Value::Integer(from_epoch_ms));
    }
    if let Some(to_epoch_ms) = filter.to_epoch_ms {
        predicates.push("created_at <= ?");
        values.push(Value::Integer(to_epoch_ms));
    }
    if let Some(session_id) = &filter.session_id {
        predicates.push("session_id = ?");
        values.push(Value::Text(session_id.clone()));
    }
    if !predicates.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&predicates.join(" AND "));
    }
    sql.push_str(" ORDER BY id");
    (sql, values)
}
