use rusqlite::types::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditFilter {
    pub class: Option<String>,
    pub source: Option<String>,
    pub action: Option<String>,
    pub document_kind: Option<String>,
    pub from_epoch_ms: Option<i64>,
    pub to_epoch_ms: Option<i64>,
    pub session_id: Option<String>,
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
    pub created_at: Option<i64>,
    pub session_id: Option<String>,
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
    let mut sql = format!(
        "SELECT source, class, action, field_name, document_kind, conflict_loser, {decided_by_column}, {created_at_column}, {session_id_column} FROM redaction_log"
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
    if let Some(document_kind) = &filter.document_kind {
        predicates.push("document_kind = ?");
        values.push(Value::Text(document_kind.clone()));
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
