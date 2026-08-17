use std::borrow::Cow;
use std::collections::BTreeSet;

use rusqlite::types::Value;

pub const DEFAULT_SNAPSHOT_SCHEME: &str = "gaze.snapshot.v1.sha256-salted";
pub const DEFAULT_SNAPSHOT_ALG: &str = "SHA-256";

/// Query filter for [`crate::SqliteLogger::query`] and
/// [`crate::SqliteLogger::query_safety_net`].
///
/// Construct with `AuditFilter::default()` for all rows, or set fields to
/// narrow by class, source, action, document kind, raw safety-net label, field
/// path, epoch-millisecond time range, or session ID.
#[derive(Debug, Clone, Default, PartialEq)]
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
    pub has_ambiguity: Option<bool>,
    pub ambiguity_reason: Option<String>,
    pub collision_family: Option<String>,
    pub collision_variant: Option<String>,
    pub recognizer_id: Option<String>,
    pub recognizer_version_id: Option<String>,
    pub provenance_stage: Option<String>,
    pub provenance_model_id: Option<String>,
    pub provenance_model_version: Option<String>,
    pub provenance_artifact_sha256: Option<String>,
    pub provenance_tokenizer_sha256: Option<String>,
    pub provenance_locale_resolved: Option<String>,
    pub provenance_locale_match_kind: Option<String>,
    pub provenance_canonical_class: Option<String>,
    pub provenance_native_class: Option<String>,
    pub provenance_confidence: Option<f64>,
    pub provenance_merged_from: Option<String>,
    pub restore_events_only: bool,
}

/// Metadata-only redaction audit row returned by [`crate::SqliteLogger::query`].
///
/// This row mirrors the approved audit export surface. It is safe for audit
/// display and reporting, but it is not restore material and contains no
/// original PII or token values.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditLogRow {
    pub source: String,
    pub recognizer_id: Option<String>,
    pub recognizer_version_id: Option<String>,
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
    pub validator_fail_reason: Option<String>,
    pub ambiguity_record: Option<String>,
    pub collision_family: Option<String>,
    pub collision_variant: Option<String>,
    pub fallback_triggered: Option<String>,
    pub provenance_stage: Option<String>,
    pub provenance_model_id: Option<String>,
    pub provenance_model_version: Option<String>,
    pub provenance_artifact_sha256: Option<String>,
    pub provenance_tokenizer_sha256: Option<String>,
    pub provenance_locale_resolved: Option<String>,
    pub provenance_locale_match_kind: Option<String>,
    pub provenance_canonical_class: Option<String>,
    pub provenance_native_class: Option<String>,
    pub provenance_confidence: Option<f64>,
    pub provenance_merged_from: Option<String>,
    pub restore_policy: Option<String>,
    pub restore_decision: Option<String>,
    pub restore_unknown_token_count: Option<i64>,
    pub restore_manifest_bypass_count: Option<i64>,
    pub restore_fresh_pii_count: Option<i64>,
    pub restore_phase_mask: Option<i64>,
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
    "recognizer_id",
    "recognizer_version_id",
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
    "validator_fail_reason",
    "ambiguity_record",
    "collision_family",
    "collision_variant",
    "fallback_triggered",
    "provenance_stage",
    "provenance_model_id",
    "provenance_model_version",
    "provenance_artifact_sha256",
    "provenance_tokenizer_sha256",
    "provenance_locale_resolved",
    "provenance_locale_match_kind",
    "provenance_canonical_class",
    "provenance_native_class",
    "provenance_confidence",
    "provenance_merged_from",
    "restore_policy",
    "restore_decision",
    "restore_unknown_token_count",
    "restore_manifest_bypass_count",
    "restore_fresh_pii_count",
    "restore_phase_mask",
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentColumns(BTreeSet<String>);

impl PresentColumns {
    pub fn new(columns: BTreeSet<String>) -> Self {
        Self(columns)
    }

    pub fn contains(&self, column: &str) -> bool {
        self.0.contains(column)
    }
}

#[derive(Debug, Default, PartialEq)]
struct Predicates<'a>(Vec<Cow<'a, str>>);

impl<'a> Predicates<'a> {
    fn push(&mut self, predicate: impl Into<Cow<'a, str>>) {
        self.0.push(predicate.into());
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn join(&self, separator: &str) -> String {
        self.0
            .iter()
            .map(Cow::as_ref)
            .collect::<Vec<_>>()
            .join(separator)
    }
}

fn audit_select_expression<'a>(column: &'a str, present_columns: &PresentColumns) -> Cow<'a, str> {
    if present_columns.contains(column) {
        return Cow::Borrowed(column);
    }

    match column {
        "source" | "class" | "action" | "field_name" | "document_kind" | "conflict_loser" => {
            Cow::Borrowed(column)
        }
        "decided_by" => Cow::Borrowed("'none' AS decided_by"),
        "snapshot_scheme" => Cow::Owned(format!("'{DEFAULT_SNAPSHOT_SCHEME}' AS snapshot_scheme")),
        "snapshot_alg" => Cow::Owned(format!("'{DEFAULT_SNAPSHOT_ALG}' AS snapshot_alg")),
        _ => Cow::Owned(format!("NULL AS {column}")),
    }
}

/// Audit reads are defense-in-depth restricted to metadata columns that are
/// safe to display. Do not switch this path to `SELECT *`; future schema
/// additions may include restore material or other sensitive payloads.
pub fn build_audit_query_sql(
    filter: &AuditFilter,
    present_columns: &PresentColumns,
) -> (String, Vec<Value>) {
    let select_list = AUDIT_RESTRICTED_COLUMNS
        .iter()
        .map(|column| audit_select_expression(column, present_columns))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!("SELECT {select_list} FROM redaction_log");
    let mut predicates = Predicates::default();
    let mut values = Vec::new();
    if let Some(class) = &filter.class {
        predicates.push("class = ?");
        values.push(Value::Text(class.clone()));
    }
    if let Some(source) = &filter.source {
        predicates.push("source = ?");
        values.push(Value::Text(source.clone()));
    }
    if let Some(recognizer_id) = &filter.recognizer_id {
        if present_columns.contains("recognizer_id") {
            predicates.push("recognizer_id = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Text(recognizer_id.clone()));
    }
    if let Some(recognizer_version_id) = &filter.recognizer_version_id {
        if present_columns.contains("recognizer_version_id") {
            predicates.push("recognizer_version_id = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Text(recognizer_version_id.clone()));
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
        if present_columns.contains("created_at") {
            predicates.push("created_at >= ?");
        } else {
            predicates.push("NULL >= ?");
        }
        values.push(Value::Integer(from_epoch_ms));
    }
    if let Some(to_epoch_ms) = filter.to_epoch_ms {
        if present_columns.contains("created_at") {
            predicates.push("created_at <= ?");
        } else {
            predicates.push("NULL <= ?");
        }
        values.push(Value::Integer(to_epoch_ms));
    }
    if let Some(session_id) = &filter.session_id {
        if present_columns.contains("session_id") {
            predicates.push("session_id = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Text(session_id.clone()));
    }
    if let Some(snapshot_scheme) = &filter.snapshot_scheme {
        if present_columns.contains("snapshot_scheme") {
            predicates.push("snapshot_scheme = ?");
        } else {
            predicates.push("'gaze.snapshot.v1.sha256-salted' = ?");
        }
        values.push(Value::Text(snapshot_scheme.clone()));
    }
    if let Some(snapshot_alg) = &filter.snapshot_alg {
        if present_columns.contains("snapshot_alg") {
            predicates.push("snapshot_alg = ?");
        } else {
            predicates.push("'SHA-256' = ?");
        }
        values.push(Value::Text(snapshot_alg.clone()));
    }
    if let Some(snapshot_key_version) = filter.snapshot_key_version {
        if present_columns.contains("snapshot_key_version") {
            predicates.push("snapshot_key_version = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Integer(snapshot_key_version));
    }
    if let Some(has_ambiguity) = filter.has_ambiguity {
        if present_columns.contains("ambiguity_record") {
            predicates.push(if has_ambiguity {
                "ambiguity_record IS NOT NULL"
            } else {
                "ambiguity_record IS NULL"
            });
        } else {
            predicates.push(if has_ambiguity {
                "NULL IS NOT NULL"
            } else {
                "NULL IS NULL"
            });
        }
    }
    if let Some(reason) = &filter.ambiguity_reason {
        if present_columns.contains("ambiguity_record") {
            predicates.push("json_extract(ambiguity_record, '$.reason') = ?");
        } else {
            predicates.push("json_extract(NULL, '$.reason') = ?");
        }
        values.push(Value::Text(reason.clone()));
    }
    if let Some(family) = &filter.collision_family {
        if present_columns.contains("collision_family") {
            predicates.push("collision_family = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Text(family.clone()));
    }
    if let Some(variant) = &filter.collision_variant {
        if present_columns.contains("collision_variant") {
            predicates.push("collision_variant = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Text(variant.clone()));
    }
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_stage"),
        "provenance_stage",
        &filter.provenance_stage,
    );
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_model_id"),
        "provenance_model_id",
        &filter.provenance_model_id,
    );
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_model_version"),
        "provenance_model_version",
        &filter.provenance_model_version,
    );
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_artifact_sha256"),
        "provenance_artifact_sha256",
        &filter.provenance_artifact_sha256,
    );
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_tokenizer_sha256"),
        "provenance_tokenizer_sha256",
        &filter.provenance_tokenizer_sha256,
    );
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_locale_resolved"),
        "provenance_locale_resolved",
        &filter.provenance_locale_resolved,
    );
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_locale_match_kind"),
        "provenance_locale_match_kind",
        &filter.provenance_locale_match_kind,
    );
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_canonical_class"),
        "provenance_canonical_class",
        &filter.provenance_canonical_class,
    );
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_native_class"),
        "provenance_native_class",
        &filter.provenance_native_class,
    );
    if let Some(confidence) = filter.provenance_confidence {
        if present_columns.contains("provenance_confidence") {
            predicates.push("provenance_confidence = ?");
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Real(confidence));
    }
    add_optional_text_filter(
        &mut predicates,
        &mut values,
        present_columns.contains("provenance_merged_from"),
        "provenance_merged_from",
        &filter.provenance_merged_from,
    );
    if filter.restore_events_only {
        if present_columns.contains("restore_policy") {
            predicates.push("restore_policy IS NOT NULL");
        } else {
            predicates.push("NULL IS NOT NULL");
        }
    }
    if !predicates.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&predicates.join(" AND "));
    }
    sql.push_str(" ORDER BY rowid");
    (sql, values)
}

fn add_optional_text_filter<'a>(
    predicates: &mut Predicates<'a>,
    values: &mut Vec<Value>,
    has_column: bool,
    column: &'a str,
    value: &Option<String>,
) {
    if let Some(value) = value {
        if has_column {
            predicates.push(Cow::Owned(format!("{column} = ?")));
        } else {
            predicates.push("NULL = ?");
        }
        values.push(Value::Text(value.clone()));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const OPTIONAL_COLUMNS: [&str; 30] = [
        "decided_by",
        "created_at",
        "session_id",
        "snapshot_scheme",
        "snapshot_alg",
        "snapshot_key_version",
        "validator_fail_reason",
        "ambiguity_record",
        "collision_family",
        "collision_variant",
        "fallback_triggered",
        "recognizer_id",
        "recognizer_version_id",
        "provenance_stage",
        "provenance_model_id",
        "provenance_model_version",
        "provenance_artifact_sha256",
        "provenance_tokenizer_sha256",
        "provenance_locale_resolved",
        "provenance_locale_match_kind",
        "provenance_canonical_class",
        "provenance_native_class",
        "provenance_confidence",
        "provenance_merged_from",
        "restore_policy",
        "restore_decision",
        "restore_unknown_token_count",
        "restore_manifest_bypass_count",
        "restore_fresh_pii_count",
        "restore_phase_mask",
    ];

    fn build_with_columns(columns: &[&str]) -> String {
        let present_columns =
            PresentColumns::new(columns.iter().map(|column| (*column).to_string()).collect());
        build_audit_query_sql(&AuditFilter::default(), &present_columns).0
    }

    #[test]
    fn generated_sql_is_byte_identical_for_column_presence_matrix() {
        assert_eq!(
            build_with_columns(&OPTIONAL_COLUMNS),
            "SELECT source, recognizer_id, recognizer_version_id, class, action, field_name, document_kind, conflict_loser, decided_by, created_at, session_id, snapshot_scheme, snapshot_alg, snapshot_key_version, validator_fail_reason, ambiguity_record, collision_family, collision_variant, fallback_triggered, provenance_stage, provenance_model_id, provenance_model_version, provenance_artifact_sha256, provenance_tokenizer_sha256, provenance_locale_resolved, provenance_locale_match_kind, provenance_canonical_class, provenance_native_class, provenance_confidence, provenance_merged_from, restore_policy, restore_decision, restore_unknown_token_count, restore_manifest_bypass_count, restore_fresh_pii_count, restore_phase_mask FROM redaction_log ORDER BY rowid"
        );
        assert_eq!(
            build_with_columns(&[]),
            "SELECT source, NULL AS recognizer_id, NULL AS recognizer_version_id, class, action, field_name, document_kind, conflict_loser, 'none' AS decided_by, NULL AS created_at, NULL AS session_id, 'gaze.snapshot.v1.sha256-salted' AS snapshot_scheme, 'SHA-256' AS snapshot_alg, NULL AS snapshot_key_version, NULL AS validator_fail_reason, NULL AS ambiguity_record, NULL AS collision_family, NULL AS collision_variant, NULL AS fallback_triggered, NULL AS provenance_stage, NULL AS provenance_model_id, NULL AS provenance_model_version, NULL AS provenance_artifact_sha256, NULL AS provenance_tokenizer_sha256, NULL AS provenance_locale_resolved, NULL AS provenance_locale_match_kind, NULL AS provenance_canonical_class, NULL AS provenance_native_class, NULL AS provenance_confidence, NULL AS provenance_merged_from, NULL AS restore_policy, NULL AS restore_decision, NULL AS restore_unknown_token_count, NULL AS restore_manifest_bypass_count, NULL AS restore_fresh_pii_count, NULL AS restore_phase_mask FROM redaction_log ORDER BY rowid"
        );
        let mixed = [
            "created_at",
            "snapshot_alg",
            "ambiguity_record",
            "collision_variant",
            "recognizer_id",
            "provenance_stage",
            "provenance_model_version",
            "provenance_tokenizer_sha256",
            "provenance_locale_match_kind",
            "provenance_native_class",
            "provenance_merged_from",
            "restore_decision",
            "restore_manifest_bypass_count",
            "restore_phase_mask",
        ];
        assert_eq!(
            build_with_columns(&mixed),
            "SELECT source, recognizer_id, NULL AS recognizer_version_id, class, action, field_name, document_kind, conflict_loser, 'none' AS decided_by, created_at, NULL AS session_id, 'gaze.snapshot.v1.sha256-salted' AS snapshot_scheme, snapshot_alg, NULL AS snapshot_key_version, NULL AS validator_fail_reason, ambiguity_record, NULL AS collision_family, collision_variant, NULL AS fallback_triggered, provenance_stage, NULL AS provenance_model_id, provenance_model_version, NULL AS provenance_artifact_sha256, provenance_tokenizer_sha256, NULL AS provenance_locale_resolved, provenance_locale_match_kind, NULL AS provenance_canonical_class, provenance_native_class, NULL AS provenance_confidence, provenance_merged_from, NULL AS restore_policy, restore_decision, NULL AS restore_unknown_token_count, restore_manifest_bypass_count, NULL AS restore_fresh_pii_count, restore_phase_mask FROM redaction_log ORDER BY rowid"
        );
    }

    #[test]
    fn select_list_contains_each_allowlisted_column_once_in_order() {
        let sql = build_with_columns(&[]);
        let select_list = sql
            .strip_prefix("SELECT ")
            .unwrap()
            .split_once(" FROM redaction_log")
            .unwrap()
            .0;
        let aliases = select_list
            .split(", ")
            .map(|expression| {
                expression
                    .rsplit_once(" AS ")
                    .map_or(expression, |(_, alias)| alias)
            })
            .collect::<Vec<_>>();

        assert_eq!(aliases, AUDIT_RESTRICTED_COLUMNS);
        for column in AUDIT_RESTRICTED_COLUMNS {
            assert_eq!(aliases.iter().filter(|alias| *alias == column).count(), 1);
        }
    }

    #[test]
    fn optional_text_filter_is_total_for_future_allowlisted_columns() {
        let mut predicates = Predicates::default();
        let mut values = Vec::new();

        add_optional_text_filter(
            &mut predicates,
            &mut values,
            true,
            "future_provenance",
            &Some("synthetic".to_string()),
        );

        assert_eq!(predicates.0, ["future_provenance = ?"]);
        assert_eq!(values, [Value::Text("synthetic".to_string())]);
    }
}
