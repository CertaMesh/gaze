//! MCP tool handlers. Each handler:
//!   1. Validates args against the policy (table allowlist, limit cap,
//!      column rules on filter values — session tokens for PII columns).
//!   2. Calls the adapter to get `RawRow`s.
//!   3. Runs `Anonymizer::clean()` on each row.
//!   4. Writes an audit-log entry.
//!   5. Returns `CleanRow`s (as `serde_json::Value`).
//!
//! Errors flow through `ErrorSanitizer::sanitize` before being returned
//! to the MCP peer, and collapse to `InvalidFilterValue` on any
//! PII-adjacent failure per spec §error handling.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::adapter::{DatabaseAdapter, Filter, FilterOp};
use crate::anon::Anonymizer;
use crate::audit::{AuditEntry, AuditLog};
use crate::mcp::errors::ErrorSanitizer;
use crate::policy::parser::Policy;
use crate::types::CleanRow;

#[derive(Debug, Deserialize)]
pub struct SampleArgs {
    pub env: String,
    pub table: String,
    #[serde(default)]
    pub filters: Vec<FilterArg>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct FilterArg {
    pub column: String,
    pub op: String,
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SampleResult {
    pub rows: Vec<CleanRow>,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("UnknownEnvironment: {0}")]
    UnknownEnvironment(String),
    #[error("TableNotAllowed: {0}")]
    TableNotAllowed(String),
    #[error("LimitExceeded: requested {requested} exceeds max {max}")]
    LimitExceeded { requested: usize, max: usize },
    #[error("InvalidFilterValue")]
    InvalidFilterValue,
    #[error("AdapterError: {0}")]
    Adapter(String),
}

pub struct ToolContext {
    pub policy: Policy,
    pub adapter: Arc<dyn DatabaseAdapter>,
    pub anonymizer: Arc<Anonymizer>,
    pub audit: Arc<AuditLog>,
    pub sanitizer: ErrorSanitizer,
}

impl ToolContext {
    pub async fn db_sample(&self, args: SampleArgs) -> Result<SampleResult, ToolError> {
        let start = Instant::now();
        let decision = self.check_and_run_sample(&args).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let (audit_decision, reason, rows_count) = match &decision {
            Ok(rows) => ("allow", None::<String>, Some(rows.len() as u64)),
            Err(e) => ("deny", Some(format!("{e}")), None),
        };

        let req_json = serde_json::json!({
            "table": args.table,
            "limit": args.limit,
            "filters": args.filters.iter().map(|f| serde_json::json!({
                "column": f.column,
                "op": f.op,
                // We intentionally log the raw (token) filter values —
                // they are session tokens, not raw PII.
                "values": f.values,
            })).collect::<Vec<_>>()
        })
        .to_string();

        self.audit
            .append(AuditEntry {
                tool: "db.sample",
                request_json: &req_json,
                decision: audit_decision,
                reason: reason.as_deref(),
                duration_ms,
                result_rows: rows_count,
                result_columns: None,
            })
            .ok();

        match decision {
            Ok(rows) => Ok(SampleResult { rows }),
            // Preserve the original variant so callers can match on it;
            // only the Adapter variant can carry PII, so sanitize its
            // message in place.
            Err(ToolError::Adapter(msg)) => Err(ToolError::Adapter(self.sanitizer.sanitize(&msg))),
            Err(other) => Err(other),
        }
    }

    async fn check_and_run_sample(&self, args: &SampleArgs) -> Result<Vec<CleanRow>, ToolError> {
        if args.env != "production" {
            return Err(ToolError::UnknownEnvironment(args.env.clone()));
        }
        let db = &self.policy.policy.database;
        if !db.allowed_tables.iter().any(|t| t == &args.table) {
            return Err(ToolError::TableNotAllowed(args.table.clone()));
        }
        let limit = args.limit.unwrap_or(db.max_rows);
        if limit > db.max_rows {
            return Err(ToolError::LimitExceeded {
                requested: limit,
                max: db.max_rows,
            });
        }

        // Resolve filters: for PII columns every value must be a known
        // session token (reverse-mapped). Non-PII columns pass through.
        let mut resolved: Vec<Filter> = Vec::with_capacity(args.filters.len());
        for f in &args.filters {
            let op = parse_op(&f.op).ok_or(ToolError::InvalidFilterValue)?;
            if matches!(op, FilterOp::Like) {
                // Block LIKE on PII columns; block is conservative here.
                if self.is_pii_column(&f.column) {
                    return Err(ToolError::InvalidFilterValue);
                }
            }
            let mut resolved_values = Vec::with_capacity(f.values.len());
            for v in &f.values {
                if self.is_pii_column(&f.column) {
                    let raw = self
                        .anonymizer
                        .raw_for_fake(&f.column, v)
                        .ok_or(ToolError::InvalidFilterValue)?;
                    resolved_values.push(raw);
                } else {
                    resolved_values.push(v.clone());
                }
            }
            resolved.push(Filter {
                column: f.column.clone(),
                op,
                values: resolved_values,
            });
        }

        let raw_rows = self
            .adapter
            .sample(&args.table, &resolved, limit)
            .await
            .map_err(|e| ToolError::Adapter(e.to_string()))?;

        let clean: Vec<_> = raw_rows
            .into_iter()
            .map(|r| self.anonymizer.clean(r))
            .collect();
        Ok(clean)
    }

    fn is_pii_column(&self, column: &str) -> bool {
        self.policy
            .policy
            .database
            .column_rules
            .iter()
            .any(|r| r.column == column && r.class != "none" && r.class != "non_pii")
    }
}

fn parse_op(s: &str) -> Option<FilterOp> {
    Some(match s {
        "eq" => FilterOp::Eq,
        "neq" => FilterOp::Neq,
        "lt" => FilterOp::Lt,
        "lte" => FilterOp::Lte,
        "gt" => FilterOp::Gt,
        "gte" => FilterOp::Gte,
        "in" => FilterOp::In,
        "like" => FilterOp::Like,
        "is_null" => FilterOp::IsNull,
        "is_not_null" => FilterOp::IsNotNull,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{AdapterError, TableSchema};
    use crate::anon::Anonymizer;
    use crate::policy::classifier::{Classifier, PiiClass};
    use crate::types::{RawRow, Value};
    use async_trait::async_trait;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    struct FakeAdapter;

    #[async_trait]
    impl DatabaseAdapter for FakeAdapter {
        async fn schema(&self, _table: &str) -> Result<TableSchema, AdapterError> {
            Err(AdapterError::Query("unused".into()))
        }
        async fn sample(
            &self,
            _table: &str,
            filters: &[Filter],
            _limit: usize,
        ) -> Result<Vec<RawRow>, AdapterError> {
            // If a filter resolved to an unknown session token, the policy
            // layer would have rejected it before calling us. Assert we
            // only get raw values here.
            for f in filters {
                for v in &f.values {
                    // Session tokens for emails look like `user_*@example.com`.
                    // Only tokens carry the `user_` prefix — raw emails do not.
                    assert!(
                        !v.starts_with("user_"),
                        "filter values hitting adapter must be raw, not tokens"
                    );
                }
            }
            let mut cols = BTreeMap::new();
            cols.insert("id".into(), Value::Int(42));
            cols.insert("email".into(), Value::Text("krishan@example.com".into()));
            Ok(vec![RawRow { columns: cols }])
        }
        async fn count(&self, _: &str, _: &[Filter]) -> Result<u64, AdapterError> {
            Err(AdapterError::Query("unused".into()))
        }
        async fn distinct(&self, _: &str, _: &str, _: usize) -> Result<Vec<RawRow>, AdapterError> {
            Err(AdapterError::Query("unused".into()))
        }
        async fn explain(&self, _: &str, _: &[Filter]) -> Result<String, AdapterError> {
            Err(AdapterError::Query("unused".into()))
        }
    }

    fn make_ctx() -> ToolContext {
        let policy_text = r#"
[connection.production]
kind = "mysql"
ssh_host = "x@y"
local_port = 13306
remote_host = "127.0.0.1"
remote_port = 3306
database = "t"
user = "u"
password_env = "P"

[policy.database]
allowed_tables = ["users"]
max_rows = 10

[[policy.database.columns]]
table = "users"
column = "id"
class = "id"

[[policy.database.columns]]
table = "users"
column = "email"
class = "email"
"#;
        let policy: Policy = toml::from_str(policy_text).unwrap();
        crate::policy::parser::validate(&policy).unwrap();
        let classifier = Classifier::new()
            .with_column("id", PiiClass::Id)
            .with_column("email", PiiClass::Email);
        let anonymizer = Arc::new(Anonymizer::new(classifier));
        // Keep the temp dir alive for the duration of the process by
        // leaking it — otherwise it is deleted when `make_ctx` returns
        // and subsequent sqlite writes from the audit log silently fail.
        let tmp = tempdir().unwrap().keep();
        let audit = Arc::new(AuditLog::open(&tmp.join("audit.db")).unwrap());
        ToolContext {
            policy,
            adapter: Arc::new(FakeAdapter),
            anonymizer,
            audit,
            sanitizer: ErrorSanitizer::default(),
        }
    }

    #[tokio::test]
    async fn sample_anonymizes_rows_and_audits() {
        let ctx = make_ctx();
        let res = ctx
            .db_sample(SampleArgs {
                env: "production".into(),
                table: "users".into(),
                filters: vec![],
                limit: Some(5),
            })
            .await
            .unwrap();
        assert_eq!(res.rows.len(), 1);
        let json = serde_json::to_string(&res.rows[0]).unwrap();
        assert!(!json.contains("krishan@example.com"));
        assert!(json.contains("user_"));
        assert_eq!(ctx.audit.count().unwrap(), 1);
    }

    #[tokio::test]
    async fn sample_rejects_disallowed_table() {
        let ctx = make_ctx();
        let err = ctx
            .db_sample(SampleArgs {
                env: "production".into(),
                table: "secret_keys".into(),
                filters: vec![],
                limit: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::TableNotAllowed(_)));
    }

    #[tokio::test]
    async fn sample_rejects_raw_pii_filter_value() {
        let ctx = make_ctx();
        let err = ctx
            .db_sample(SampleArgs {
                env: "production".into(),
                table: "users".into(),
                filters: vec![FilterArg {
                    column: "email".into(),
                    op: "eq".into(),
                    values: vec!["hacker@example.com".into()],
                }],
                limit: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidFilterValue));
    }

    #[tokio::test]
    async fn sample_accepts_session_token_filter() {
        let ctx = make_ctx();
        // First call to populate the session map.
        let first = ctx
            .db_sample(SampleArgs {
                env: "production".into(),
                table: "users".into(),
                filters: vec![],
                limit: Some(1),
            })
            .await
            .unwrap();
        let token = first.rows[0]
            .columns()
            .get("email")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();
        // Second call uses the token — should resolve and succeed.
        let res = ctx
            .db_sample(SampleArgs {
                env: "production".into(),
                table: "users".into(),
                filters: vec![FilterArg {
                    column: "email".into(),
                    op: "eq".into(),
                    values: vec![token],
                }],
                limit: Some(1),
            })
            .await
            .unwrap();
        assert_eq!(res.rows.len(), 1);
    }
}
