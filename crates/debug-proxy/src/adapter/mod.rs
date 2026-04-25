use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use gaze::{CleanDocument, Pipeline, RawDocument, Session, Value};
use serde::Serialize;
use thiserror::Error;

use crate::mcp::errors::ErrorSanitizer;
use crate::policy::PolicyFile;

pub mod laravel_log;
pub mod mysql;
pub mod ssh_tunnel;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("connection error: {0}")]
    Connection(String),
    #[error("query error: {0}")]
    Query(String),
    #[error("unknown table: {0}")]
    UnknownTable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Int,
    Text,
}

#[derive(Debug, Clone)]
pub struct ColumnSchema {
    pub name: String,
    pub ty: ColumnType,
    pub nullable: bool,
}

#[derive(Debug, Clone)]
pub struct TableSchema {
    pub table: String,
    pub columns: Vec<ColumnSchema>,
    pub primary_key: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ColumnSchemaOut {
    pub name: String,
    pub ty: String,
    pub nullable: bool,
    pub pii_class: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableSchemaOut {
    pub table: String,
    pub columns: Vec<ColumnSchemaOut>,
    pub primary_key: Vec<String>,
}

#[async_trait]
pub trait DatabaseAdapter: Send + Sync {
    async fn tables(&self) -> Result<Vec<String>, AdapterError>;
    async fn schema(&self, table: &str) -> Result<TableSchema, AdapterError>;
    async fn sample(
        &self,
        table: &str,
        limit: usize,
    ) -> Result<Vec<BTreeMap<String, Value>>, AdapterError>;
    async fn count(&self, table: &str) -> Result<u64, AdapterError>;
    async fn distinct(
        &self,
        table: &str,
        column: &str,
        limit: usize,
    ) -> Result<Vec<Value>, AdapterError>;
}

#[async_trait]
pub trait LogAdapter: Send + Sync {
    async fn tail(&self, limit: usize) -> Result<Vec<String>, AdapterError>;
    async fn search(
        &self,
        pattern: &str,
        level: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>, AdapterError>;
    async fn context(&self, request_id: &str) -> Result<Vec<String>, AdapterError>;
}

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("{0}")]
    SanitizedAdapter(String),
    #[error("redaction failed: {0}")]
    Redaction(#[from] gaze::Error),
}

pub struct ToolContext<D, L> {
    pipeline: Pipeline,
    session: Arc<Session>,
    policy: Option<Arc<PolicyFile>>,
    db: Arc<D>,
    logs: Option<Arc<L>>,
    sanitizer: ErrorSanitizer,
}

impl<D, L> ToolContext<D, L>
where
    D: DatabaseAdapter + 'static,
    L: LogAdapter + 'static,
{
    pub fn new(
        pipeline: Pipeline,
        session: Arc<Session>,
        db: Arc<D>,
        logs: Option<Arc<L>>,
    ) -> Self {
        Self::with_policy(pipeline, session, None, db, logs)
    }

    pub fn with_policy(
        pipeline: Pipeline,
        session: Arc<Session>,
        policy: Option<Arc<PolicyFile>>,
        db: Arc<D>,
        logs: Option<Arc<L>>,
    ) -> Self {
        Self {
            pipeline,
            session,
            policy,
            db,
            logs,
            sanitizer: ErrorSanitizer,
        }
    }

    pub async fn db_tables(&self) -> Result<Vec<String>, ProxyError> {
        self.db.tables().await.map_err(|err| {
            ProxyError::SanitizedAdapter(
                self.sanitize_adapter_error(err)
                    .unwrap_or_else(|redaction_err| format!("redaction failed: {redaction_err}")),
            )
        })
    }

    pub async fn db_schema(&self, table: &str) -> Result<TableSchemaOut, ProxyError> {
        let schema = self.db.schema(table).await.map_err(|err| {
            ProxyError::SanitizedAdapter(
                self.sanitize_adapter_error(err)
                    .unwrap_or_else(|redaction_err| format!("redaction failed: {redaction_err}")),
            )
        })?;

        Ok(TableSchemaOut {
            table: schema.table,
            columns: schema
                .columns
                .into_iter()
                .map(|column| ColumnSchemaOut {
                    name: column.name.clone(),
                    ty: column_type_name(column.ty).to_string(),
                    nullable: column.nullable,
                    pii_class: self.column_pii_class(&column.name),
                })
                .collect(),
            primary_key: schema.primary_key,
        })
    }

    pub async fn db_sample(
        &self,
        table: &str,
        limit: usize,
    ) -> Result<Vec<CleanDocument>, ProxyError> {
        let rows = match self.db.sample(table, limit).await {
            Ok(rows) => rows,
            Err(err) => {
                return Err(ProxyError::SanitizedAdapter(
                    self.sanitize_adapter_error(err)?,
                ))
            }
        };

        rows.into_iter()
            .map(|row| {
                self.pipeline
                    .redact(&self.session, RawDocument::Structured(row))
                    .map_err(ProxyError::from)
            })
            .collect()
    }

    pub async fn db_count(&self, table: &str) -> Result<u64, ProxyError> {
        self.db.count(table).await.map_err(|err| {
            ProxyError::SanitizedAdapter(
                self.sanitize_adapter_error(err)
                    .unwrap_or_else(|redaction_err| format!("redaction failed: {redaction_err}")),
            )
        })
    }

    pub async fn db_distinct(
        &self,
        table: &str,
        column: &str,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>, ProxyError> {
        let values =
            self.db
                .distinct(table, column, limit)
                .await
                .map_err(|err| {
                    ProxyError::SanitizedAdapter(self.sanitize_adapter_error(err).unwrap_or_else(
                        |redaction_err| format!("redaction failed: {redaction_err}"),
                    ))
                })?;

        values
            .into_iter()
            .map(|value| self.redact_value(column, value))
            .collect()
    }

    pub async fn log_tail(&self, limit: usize) -> Result<Vec<CleanDocument>, ProxyError> {
        let logs = self
            .logs
            .as_ref()
            .ok_or_else(|| ProxyError::SanitizedAdapter("log adapter unavailable".to_string()))?;
        let lines = match logs.tail(limit).await {
            Ok(lines) => lines,
            Err(err) => {
                return Err(ProxyError::SanitizedAdapter(
                    self.sanitize_adapter_error(err)?,
                ))
            }
        };

        lines
            .into_iter()
            .map(|line| {
                self.pipeline
                    .redact(&self.session, RawDocument::Text(line))
                    .map_err(ProxyError::from)
            })
            .collect()
    }

    pub async fn logs_search(
        &self,
        pattern: &str,
        level: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>, ProxyError> {
        let logs = self
            .logs
            .as_ref()
            .ok_or_else(|| ProxyError::SanitizedAdapter("log adapter unavailable".to_string()))?;
        let lines = logs.search(pattern, level, limit).await.map_err(|err| {
            ProxyError::SanitizedAdapter(
                self.sanitize_adapter_error(err)
                    .unwrap_or_else(|redaction_err| format!("redaction failed: {redaction_err}")),
            )
        })?;
        lines
            .into_iter()
            .map(|line| self.redact_text(line))
            .collect()
    }

    pub async fn logs_context(&self, request_id: &str) -> Result<Vec<String>, ProxyError> {
        let logs = self
            .logs
            .as_ref()
            .ok_or_else(|| ProxyError::SanitizedAdapter("log adapter unavailable".to_string()))?;
        let lines = logs.context(request_id).await.map_err(|err| {
            ProxyError::SanitizedAdapter(
                self.sanitize_adapter_error(err)
                    .unwrap_or_else(|redaction_err| format!("redaction failed: {redaction_err}")),
            )
        })?;
        lines
            .into_iter()
            .map(|line| self.redact_text(line))
            .collect()
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    fn sanitize_adapter_error(&self, err: AdapterError) -> Result<String, gaze::Error> {
        self.sanitizer
            .sanitize(&self.pipeline, &self.session, &err.to_string())
    }

    fn redact_text(&self, line: String) -> Result<String, ProxyError> {
        match self
            .pipeline
            .redact(&self.session, RawDocument::Text(line))?
        {
            CleanDocument::Text(text) => Ok(text),
            CleanDocument::Structured(_) => Err(ProxyError::SanitizedAdapter(
                "unexpected structured log output".to_string(),
            )),
        }
    }

    fn redact_value(&self, column: &str, value: Value) -> Result<serde_json::Value, ProxyError> {
        let row = BTreeMap::from([(column.to_string(), value)]);
        match self
            .pipeline
            .redact(&self.session, RawDocument::Structured(row))?
        {
            CleanDocument::Structured(fields) => Ok(fields
                .get(column)
                .cloned()
                .unwrap_or(serde_json::Value::Null)),
            CleanDocument::Text(_) => Err(ProxyError::SanitizedAdapter(
                "unexpected text distinct output".to_string(),
            )),
        }
    }

    fn column_pii_class(&self, column: &str) -> String {
        self.policy
            .as_ref()
            .and_then(|policy| {
                policy
                    .policy
                    .database
                    .column_rules
                    .iter()
                    .find(|rule| rule.column == column)
                    .map(|rule| rule.class.clone())
            })
            .unwrap_or_else(|| "none".to_string())
    }
}

fn column_type_name(column_type: ColumnType) -> &'static str {
    match column_type {
        ColumnType::Int => "int",
        ColumnType::Text => "text",
    }
}
