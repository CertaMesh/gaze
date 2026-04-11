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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use crate::adapter::{ColumnSchema, DatabaseAdapter, Filter, FilterOp, LogAdapter};
use crate::anon::Anonymizer;
use crate::audit::{AuditEntry, AuditLog};
use crate::mcp::errors::ErrorSanitizer;
use crate::policy::parser::Policy;
use crate::scanner::Scanner;
use crate::types::{CleanRow, ColumnType, RawRow, Value};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SampleArgs {
    pub env: String,
    pub table: String,
    #[serde(default)]
    pub filters: Vec<FilterArg>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SchemaArgs {
    pub env: String,
    pub table: String,
}

#[derive(Debug, Serialize)]
pub struct SchemaResult {
    pub table: String,
    pub columns: Vec<ColumnOut>,
    pub primary_key: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ColumnOut {
    pub name: String,
    pub ty: String,
    pub nullable: bool,
    /// PII class as declared in policy.toml, or "none".
    pub pii_class: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CountArgs {
    pub env: String,
    pub table: String,
    #[serde(default)]
    pub filters: Vec<FilterArg>,
}

#[derive(Debug, Serialize)]
pub struct CountResult {
    pub count: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DistinctArgs {
    pub env: String,
    pub table: String,
    pub column: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct DistinctResult {
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExplainArgs {
    pub env: String,
    pub table: String,
    #[serde(default)]
    pub filters: Vec<FilterArg>,
}

#[derive(Debug, Serialize)]
pub struct ExplainResult {
    pub plan: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsSearchArgs {
    pub env: String,
    pub pattern: String,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsTailArgs {
    pub env: String,
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsContextArgs {
    pub env: String,
    pub request_id: String,
}

#[derive(Debug, Serialize)]
pub struct LogLineOut {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct LogsResult {
    pub lines: Vec<LogLineOut>,
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
    #[error("ColumnNotAllowed: {0}")]
    ColumnNotAllowed(String),
    #[error("LogsDisabled: no [policy.logs] block in policy")]
    LogsDisabled,
    #[error("AdapterError: {0}")]
    Adapter(String),
}

pub struct ToolContext {
    pub policy: Policy,
    pub adapter: Arc<dyn DatabaseAdapter>,
    pub anonymizer: Arc<Anonymizer>,
    pub audit: Arc<AuditLog>,
    pub sanitizer: ErrorSanitizer,
    pub log_adapter: Option<Arc<dyn LogAdapter>>,
    pub scanner: Scanner,
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

    /// Resolve filter values: PII columns require session tokens that
    /// reverse into raw values. Non-PII columns pass through. Same rules
    /// as `db_sample`.
    fn resolve_filters(&self, filters: &[FilterArg]) -> Result<Vec<Filter>, ToolError> {
        let mut resolved: Vec<Filter> = Vec::with_capacity(filters.len());
        for f in filters {
            let op = parse_op(&f.op).ok_or(ToolError::InvalidFilterValue)?;
            if matches!(op, FilterOp::Like) && self.is_pii_column(&f.column) {
                return Err(ToolError::InvalidFilterValue);
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
        Ok(resolved)
    }

    pub async fn db_schema(&self, args: SchemaArgs) -> Result<SchemaResult, ToolError> {
        let start = Instant::now();
        let decision = self.check_and_run_schema(&args).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let (audit_decision, reason, cols_count) = match &decision {
            Ok(r) => ("allow", None::<String>, Some(r.columns.len() as u64)),
            Err(e) => ("deny", Some(format!("{e}")), None),
        };
        let req_json = serde_json::json!({ "table": args.table }).to_string();
        self.audit
            .append(AuditEntry {
                tool: "db.schema",
                request_json: &req_json,
                decision: audit_decision,
                reason: reason.as_deref(),
                duration_ms,
                result_rows: None,
                result_columns: cols_count,
            })
            .ok();

        match decision {
            Ok(r) => Ok(r),
            Err(ToolError::Adapter(msg)) => Err(ToolError::Adapter(self.sanitizer.sanitize(&msg))),
            Err(other) => Err(other),
        }
    }

    async fn check_and_run_schema(&self, args: &SchemaArgs) -> Result<SchemaResult, ToolError> {
        if args.env != "production" {
            return Err(ToolError::UnknownEnvironment(args.env.clone()));
        }
        let db = &self.policy.policy.database;
        if !db.allowed_tables.iter().any(|t| t == &args.table) {
            return Err(ToolError::TableNotAllowed(args.table.clone()));
        }
        let schema = self
            .adapter
            .schema(&args.table)
            .await
            .map_err(|e| ToolError::Adapter(e.to_string()))?;

        let columns: Vec<ColumnOut> = schema
            .columns
            .iter()
            .map(|c: &ColumnSchema| ColumnOut {
                name: c.name.clone(),
                ty: column_type_to_string(c.ty).to_string(),
                nullable: c.nullable,
                pii_class: pii_class_for(&self.policy, &c.name),
            })
            .collect();

        Ok(SchemaResult {
            table: schema.table,
            columns,
            primary_key: schema.primary_key,
        })
    }

    pub async fn db_count(&self, args: CountArgs) -> Result<CountResult, ToolError> {
        let start = Instant::now();
        let decision = self.check_and_run_count(&args).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let (audit_decision, reason) = match &decision {
            Ok(_) => ("allow", None::<String>),
            Err(e) => ("deny", Some(format!("{e}"))),
        };
        let req_json = serde_json::json!({
            "table": args.table,
            "filters": args.filters.iter().map(|f| serde_json::json!({
                "column": f.column,
                "op": f.op,
                "values": f.values,
            })).collect::<Vec<_>>()
        })
        .to_string();
        self.audit
            .append(AuditEntry {
                tool: "db.count",
                request_json: &req_json,
                decision: audit_decision,
                reason: reason.as_deref(),
                duration_ms,
                result_rows: None,
                result_columns: None,
            })
            .ok();

        match decision {
            Ok(r) => Ok(r),
            Err(ToolError::Adapter(msg)) => Err(ToolError::Adapter(self.sanitizer.sanitize(&msg))),
            Err(other) => Err(other),
        }
    }

    async fn check_and_run_count(&self, args: &CountArgs) -> Result<CountResult, ToolError> {
        if args.env != "production" {
            return Err(ToolError::UnknownEnvironment(args.env.clone()));
        }
        let db = &self.policy.policy.database;
        if !db.allowed_tables.iter().any(|t| t == &args.table) {
            return Err(ToolError::TableNotAllowed(args.table.clone()));
        }
        // Every filter column must be in count_allowed_columns, and no LIKE
        // anywhere (count on wildcard matches leaks shape information).
        for f in &args.filters {
            if !db.count_allowed_columns.iter().any(|c| c == &f.column) {
                return Err(ToolError::InvalidFilterValue);
            }
            if f.op == "like" {
                return Err(ToolError::InvalidFilterValue);
            }
        }
        let resolved = self.resolve_filters(&args.filters)?;
        let count = self
            .adapter
            .count(&args.table, &resolved)
            .await
            .map_err(|e| ToolError::Adapter(e.to_string()))?;
        Ok(CountResult { count })
    }

    pub async fn db_distinct(&self, args: DistinctArgs) -> Result<DistinctResult, ToolError> {
        let start = Instant::now();
        let decision = self.check_and_run_distinct(&args).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let (audit_decision, reason, rows) = match &decision {
            Ok(r) => ("allow", None::<String>, Some(r.values.len() as u64)),
            Err(e) => ("deny", Some(format!("{e}")), None),
        };
        let req_json = serde_json::json!({
            "table": args.table,
            "column": args.column,
            "limit": args.limit,
        })
        .to_string();
        self.audit
            .append(AuditEntry {
                tool: "db.distinct",
                request_json: &req_json,
                decision: audit_decision,
                reason: reason.as_deref(),
                duration_ms,
                result_rows: rows,
                result_columns: None,
            })
            .ok();

        match decision {
            Ok(r) => Ok(r),
            Err(ToolError::Adapter(msg)) => Err(ToolError::Adapter(self.sanitizer.sanitize(&msg))),
            Err(other) => Err(other),
        }
    }

    async fn check_and_run_distinct(
        &self,
        args: &DistinctArgs,
    ) -> Result<DistinctResult, ToolError> {
        if args.env != "production" {
            return Err(ToolError::UnknownEnvironment(args.env.clone()));
        }
        let db = &self.policy.policy.database;
        if !db.allowed_tables.iter().any(|t| t == &args.table) {
            return Err(ToolError::TableNotAllowed(args.table.clone()));
        }
        if !db
            .distinct_allowed_columns
            .iter()
            .any(|c| c == &args.column)
        {
            return Err(ToolError::ColumnNotAllowed(args.column.clone()));
        }
        let limit = args.limit.unwrap_or(db.max_distinct);
        if limit > db.max_distinct {
            return Err(ToolError::LimitExceeded {
                requested: limit,
                max: db.max_distinct,
            });
        }

        let raw_rows = self
            .adapter
            .distinct(&args.table, &args.column, limit)
            .await
            .map_err(|e| ToolError::Adapter(e.to_string()))?;

        let mut values: Vec<serde_json::Value> = raw_rows
            .into_iter()
            .map(|r| self.anonymizer.clean(r))
            .filter_map(|clean| clean.columns().get(&args.column).cloned())
            .collect();

        // SECURITY-CRITICAL: shuffle with OsRng per request so the
        // caller cannot use column order as a side channel back into
        // adapter row order.
        use rand::seq::SliceRandom;
        values.shuffle(&mut rand::rngs::OsRng);

        Ok(DistinctResult { values })
    }

    pub async fn db_explain(&self, args: ExplainArgs) -> Result<ExplainResult, ToolError> {
        let start = Instant::now();
        let decision = self.check_and_run_explain(&args).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let (audit_decision, reason) = match &decision {
            Ok(_) => ("allow", None::<String>),
            Err(e) => ("deny", Some(format!("{e}"))),
        };
        let req_json = serde_json::json!({
            "table": args.table,
            "filters": args.filters.iter().map(|f| serde_json::json!({
                "column": f.column,
                "op": f.op,
                "values": f.values,
            })).collect::<Vec<_>>()
        })
        .to_string();
        self.audit
            .append(AuditEntry {
                tool: "db.explain",
                request_json: &req_json,
                decision: audit_decision,
                reason: reason.as_deref(),
                duration_ms,
                result_rows: None,
                result_columns: None,
            })
            .ok();

        match decision {
            Ok(r) => Ok(r),
            Err(ToolError::Adapter(msg)) => Err(ToolError::Adapter(self.sanitizer.sanitize(&msg))),
            Err(other) => Err(other),
        }
    }

    async fn check_and_run_explain(&self, args: &ExplainArgs) -> Result<ExplainResult, ToolError> {
        if args.env != "production" {
            return Err(ToolError::UnknownEnvironment(args.env.clone()));
        }
        let db = &self.policy.policy.database;
        if !db.allowed_tables.iter().any(|t| t == &args.table) {
            return Err(ToolError::TableNotAllowed(args.table.clone()));
        }
        let resolved = self.resolve_filters(&args.filters)?;
        let plan = self
            .adapter
            .explain(&args.table, &resolved)
            .await
            .map_err(|e| ToolError::Adapter(e.to_string()))?;
        // Plans come straight from the database engine — they may
        // contain literal values from filters, strings from pushdowns,
        // or column samples. Always pass through the sanitizer.
        let sanitized = self.sanitizer.sanitize(&plan);
        Ok(ExplainResult { plan: sanitized })
    }

    pub async fn logs_search(&self, args: LogsSearchArgs) -> Result<LogsResult, ToolError> {
        let start = Instant::now();
        let decision = self.check_and_run_logs_search(&args).await;
        let duration_ms = start.elapsed().as_millis() as u64;
        let (audit_decision, reason, rows) = match &decision {
            Ok(r) => ("allow", None::<String>, Some(r.lines.len() as u64)),
            Err(e) => ("deny", Some(format!("{e}")), None),
        };
        let req_json = serde_json::json!({
            "pattern": args.pattern,
            "level": args.level,
            "limit": args.limit,
        })
        .to_string();
        self.audit
            .append(AuditEntry {
                tool: "logs.search",
                request_json: &req_json,
                decision: audit_decision,
                reason: reason.as_deref(),
                duration_ms,
                result_rows: rows,
                result_columns: None,
            })
            .ok();

        match decision {
            Ok(r) => Ok(r),
            Err(ToolError::Adapter(msg)) => Err(ToolError::Adapter(self.sanitizer.sanitize(&msg))),
            Err(other) => Err(other),
        }
    }

    async fn check_and_run_logs_search(
        &self,
        args: &LogsSearchArgs,
    ) -> Result<LogsResult, ToolError> {
        if args.env != "production" {
            return Err(ToolError::UnknownEnvironment(args.env.clone()));
        }
        let adapter = self
            .log_adapter
            .as_ref()
            .ok_or(ToolError::LogsDisabled)?
            .clone();
        let lines = adapter
            .search(
                &args.pattern,
                args.level.as_deref(),
                args.limit.unwrap_or(100),
            )
            .await
            .map_err(|e| ToolError::Adapter(e.to_string()))?;
        Ok(LogsResult {
            lines: self.redact_log_lines(lines),
        })
    }

    pub async fn logs_tail(&self, args: LogsTailArgs) -> Result<LogsResult, ToolError> {
        let start = Instant::now();
        let decision = self.check_and_run_logs_tail(&args).await;
        let duration_ms = start.elapsed().as_millis() as u64;
        let (audit_decision, reason, rows) = match &decision {
            Ok(r) => ("allow", None::<String>, Some(r.lines.len() as u64)),
            Err(e) => ("deny", Some(format!("{e}")), None),
        };
        let req_json = serde_json::json!({ "n": args.n }).to_string();
        self.audit
            .append(AuditEntry {
                tool: "logs.tail",
                request_json: &req_json,
                decision: audit_decision,
                reason: reason.as_deref(),
                duration_ms,
                result_rows: rows,
                result_columns: None,
            })
            .ok();

        match decision {
            Ok(r) => Ok(r),
            Err(ToolError::Adapter(msg)) => Err(ToolError::Adapter(self.sanitizer.sanitize(&msg))),
            Err(other) => Err(other),
        }
    }

    async fn check_and_run_logs_tail(&self, args: &LogsTailArgs) -> Result<LogsResult, ToolError> {
        if args.env != "production" {
            return Err(ToolError::UnknownEnvironment(args.env.clone()));
        }
        let adapter = self
            .log_adapter
            .as_ref()
            .ok_or(ToolError::LogsDisabled)?
            .clone();
        let lines = adapter
            .tail(args.n.unwrap_or(100))
            .await
            .map_err(|e| ToolError::Adapter(e.to_string()))?;
        Ok(LogsResult {
            lines: self.redact_log_lines(lines),
        })
    }

    pub async fn logs_context(&self, args: LogsContextArgs) -> Result<LogsResult, ToolError> {
        let start = Instant::now();
        let decision = self.check_and_run_logs_context(&args).await;
        let duration_ms = start.elapsed().as_millis() as u64;
        let (audit_decision, reason, rows) = match &decision {
            Ok(r) => ("allow", None::<String>, Some(r.lines.len() as u64)),
            Err(e) => ("deny", Some(format!("{e}")), None),
        };
        let req_json = serde_json::json!({ "request_id": args.request_id }).to_string();
        self.audit
            .append(AuditEntry {
                tool: "logs.context",
                request_json: &req_json,
                decision: audit_decision,
                reason: reason.as_deref(),
                duration_ms,
                result_rows: rows,
                result_columns: None,
            })
            .ok();

        match decision {
            Ok(r) => Ok(r),
            Err(ToolError::Adapter(msg)) => Err(ToolError::Adapter(self.sanitizer.sanitize(&msg))),
            Err(other) => Err(other),
        }
    }

    async fn check_and_run_logs_context(
        &self,
        args: &LogsContextArgs,
    ) -> Result<LogsResult, ToolError> {
        if args.env != "production" {
            return Err(ToolError::UnknownEnvironment(args.env.clone()));
        }
        let adapter = self
            .log_adapter
            .as_ref()
            .ok_or(ToolError::LogsDisabled)?
            .clone();
        let lines = adapter
            .context(&args.request_id)
            .await
            .map_err(|e| ToolError::Adapter(e.to_string()))?;
        Ok(LogsResult {
            lines: self.redact_log_lines(lines),
        })
    }

    /// Layer 3 scanner + layer 2 anonymizer pipeline. Scanner strips
    /// user-configured patterns (password=*, bearer tokens), then the
    /// anonymizer runs Worka detection over the surviving text by
    /// wrapping the message in a single-column `RawRow`.
    fn redact_log_lines(&self, lines: Vec<crate::adapter::LogLine>) -> Vec<LogLineOut> {
        lines
            .into_iter()
            .map(|line| {
                let after_scan = self.scanner.redact(&line.message);
                let mut cols: BTreeMap<String, Value> = BTreeMap::new();
                cols.insert("message".into(), Value::Text(after_scan));
                let cleaned = self.anonymizer.clean(RawRow { columns: cols });
                let message = cleaned
                    .columns()
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                LogLineOut {
                    timestamp: line.timestamp,
                    level: line.level,
                    message,
                }
            })
            .collect()
    }
}

fn column_type_to_string(ty: ColumnType) -> &'static str {
    match ty {
        ColumnType::Int => "int",
        ColumnType::Float => "float",
        ColumnType::Text => "text",
        ColumnType::Bool => "bool",
        ColumnType::Bytes => "bytes",
        ColumnType::Date => "date",
        ColumnType::DateTime => "datetime",
    }
}

fn pii_class_for(policy: &Policy, column: &str) -> String {
    policy
        .policy
        .database
        .column_rules
        .iter()
        .find(|r| r.column == column)
        .map(|r| r.class.clone())
        .unwrap_or_else(|| "none".into())
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
    use crate::adapter::{AdapterError, LogLine, TableSchema};
    use crate::anon::Anonymizer;
    use crate::policy::classifier::{Classifier, PiiClass};
    use async_trait::async_trait;
    use tempfile::tempdir;

    struct FakeAdapter;

    #[async_trait]
    impl DatabaseAdapter for FakeAdapter {
        async fn schema(&self, table: &str) -> Result<TableSchema, AdapterError> {
            Ok(TableSchema {
                table: table.to_string(),
                columns: vec![
                    ColumnSchema {
                        name: "id".into(),
                        ty: ColumnType::Int,
                        nullable: false,
                    },
                    ColumnSchema {
                        name: "email".into(),
                        ty: ColumnType::Text,
                        nullable: true,
                    },
                ],
                primary_key: vec!["id".into()],
            })
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
            Ok(42)
        }
        async fn distinct(
            &self,
            _: &str,
            column: &str,
            _: usize,
        ) -> Result<Vec<RawRow>, AdapterError> {
            // Ten predictable rows — lets distinct_shuffles_results detect
            // reordering without relying on fuzzy set equality.
            let mut out = Vec::with_capacity(10);
            for i in 0..10i64 {
                let mut cols = BTreeMap::new();
                cols.insert(column.to_string(), Value::Int(i));
                out.push(RawRow { columns: cols });
            }
            Ok(out)
        }
        async fn explain(&self, _: &str, _: &[Filter]) -> Result<String, AdapterError> {
            Ok("SELECT * FROM users WHERE email = 'krishan@example.com'".to_string())
        }
    }

    struct FakeLogAdapter;

    #[async_trait]
    impl LogAdapter for FakeLogAdapter {
        async fn search(
            &self,
            _pattern: &str,
            _level: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LogLine>, AdapterError> {
            Ok(vec![LogLine {
                timestamp: "2025-01-01T00:00:00Z".into(),
                level: "info".into(),
                message: "user=alice password=hunter2".into(),
                raw: "user=alice password=hunter2".into(),
            }])
        }
        async fn tail(&self, _n: usize) -> Result<Vec<LogLine>, AdapterError> {
            Ok(vec![])
        }
        async fn context(&self, _request_id: &str) -> Result<Vec<LogLine>, AdapterError> {
            Ok(vec![])
        }
    }

    const BASE_POLICY_TOML: &str = r#"
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
max_distinct = 10
count_allowed_columns = ["id"]
distinct_allowed_columns = ["id"]

[[policy.database.columns]]
table = "users"
column = "id"
class = "id"

[[policy.database.columns]]
table = "users"
column = "email"
class = "email"
"#;

    fn make_ctx() -> ToolContext {
        make_ctx_inner(None)
    }

    fn make_ctx_with_logs(adapter: Arc<dyn LogAdapter>) -> ToolContext {
        make_ctx_inner(Some(adapter))
    }

    fn make_ctx_inner(log_adapter: Option<Arc<dyn LogAdapter>>) -> ToolContext {
        let policy: Policy = toml::from_str(BASE_POLICY_TOML).unwrap();
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
        let scanner = Scanner::compile(&[
            "password=[^ ]+".to_string(),
            "Bearer [A-Za-z0-9._-]+".to_string(),
        ])
        .unwrap();
        ToolContext {
            policy,
            adapter: Arc::new(FakeAdapter),
            anonymizer,
            audit,
            sanitizer: ErrorSanitizer::default(),
            log_adapter,
            scanner,
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

    #[tokio::test]
    async fn schema_returns_pii_class_labels() {
        let ctx = make_ctx();
        let res = ctx
            .db_schema(SchemaArgs {
                env: "production".into(),
                table: "users".into(),
            })
            .await
            .unwrap();
        assert_eq!(res.table, "users");
        assert_eq!(res.primary_key, vec!["id".to_string()]);
        let id_col = res.columns.iter().find(|c| c.name == "id").unwrap();
        assert_eq!(id_col.pii_class, "id");
        assert_eq!(id_col.ty, "int");
        assert!(!id_col.nullable);
        let email_col = res.columns.iter().find(|c| c.name == "email").unwrap();
        assert_eq!(email_col.pii_class, "email");
        assert_eq!(email_col.ty, "text");
        assert!(email_col.nullable);
    }

    #[tokio::test]
    async fn count_rejects_like_filter() {
        let ctx = make_ctx();
        let err = ctx
            .db_count(CountArgs {
                env: "production".into(),
                table: "users".into(),
                filters: vec![FilterArg {
                    column: "id".into(),
                    op: "like".into(),
                    values: vec!["1%".into()],
                }],
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidFilterValue));
    }

    #[tokio::test]
    async fn count_rejects_disallowed_column() {
        let ctx = make_ctx();
        let err = ctx
            .db_count(CountArgs {
                env: "production".into(),
                table: "users".into(),
                filters: vec![FilterArg {
                    column: "email".into(),
                    op: "eq".into(),
                    values: vec!["whatever".into()],
                }],
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidFilterValue));
    }

    #[tokio::test]
    async fn distinct_rejects_disallowed_column() {
        let ctx = make_ctx();
        let err = ctx
            .db_distinct(DistinctArgs {
                env: "production".into(),
                table: "users".into(),
                column: "email".into(),
                limit: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::ColumnNotAllowed(_)));
    }

    #[tokio::test]
    async fn distinct_rejects_limit_over_max() {
        let ctx = make_ctx();
        let err = ctx
            .db_distinct(DistinctArgs {
                env: "production".into(),
                table: "users".into(),
                column: "id".into(),
                limit: Some(20),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::LimitExceeded { .. }));
    }

    #[tokio::test]
    async fn distinct_shuffles_results() {
        let ctx = make_ctx();
        let mut observed = std::collections::HashSet::new();
        for _ in 0..20 {
            let res = ctx
                .db_distinct(DistinctArgs {
                    env: "production".into(),
                    table: "users".into(),
                    column: "id".into(),
                    limit: Some(10),
                })
                .await
                .unwrap();
            let order: Vec<String> = res.values.iter().map(|v| v.to_string()).collect();
            observed.insert(order);
        }
        // 10 items → 10! permutations. Probability of all 20 iterations
        // landing on the same order is ~1/10!^19 ≈ 0. Two distinct
        // orderings is a near-certain lower bound.
        assert!(
            observed.len() >= 2,
            "expected shuffled output to yield multiple orderings, saw {}",
            observed.len()
        );
    }

    #[tokio::test]
    async fn explain_sanitizes_pii_in_plan() {
        let ctx = make_ctx();
        let res = ctx
            .db_explain(ExplainArgs {
                env: "production".into(),
                table: "users".into(),
                filters: vec![],
            })
            .await
            .unwrap();
        assert!(
            !res.plan.contains("krishan@example.com"),
            "explain plan leaked raw email: {}",
            res.plan
        );
    }

    #[tokio::test]
    async fn logs_search_redacts_password() {
        let ctx = make_ctx_with_logs(Arc::new(FakeLogAdapter));
        let res = ctx
            .logs_search(LogsSearchArgs {
                env: "production".into(),
                pattern: ".*".into(),
                level: None,
                limit: None,
            })
            .await
            .unwrap();
        assert_eq!(res.lines.len(), 1);
        let msg = &res.lines[0].message;
        assert!(
            !msg.contains("hunter2"),
            "scanner failed to redact password: {msg}"
        );
    }
}
