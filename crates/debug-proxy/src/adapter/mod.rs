use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use gaze::{CleanDocument, Pipeline, RawDocument, Session, Value};
use thiserror::Error;

use crate::mcp::errors::ErrorSanitizer;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("connection error: {0}")]
    Connection(String),
    #[error("query error: {0}")]
    Query(String),
    #[error("unknown table: {0}")]
    UnknownTable(String),
}

#[async_trait]
pub trait DatabaseAdapter: Send + Sync {
    async fn sample(
        &self,
        table: &str,
        limit: usize,
    ) -> Result<Vec<BTreeMap<String, Value>>, AdapterError>;
}

#[async_trait]
pub trait LogAdapter: Send + Sync {
    async fn tail(&self, limit: usize) -> Result<Vec<String>, AdapterError>;
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
        Self {
            pipeline,
            session,
            db,
            logs,
            sanitizer: ErrorSanitizer,
        }
    }

    pub async fn db_sample(&self, table: &str, limit: usize) -> Result<Vec<CleanDocument>, ProxyError> {
        let rows = match self.db.sample(table, limit).await {
            Ok(rows) => rows,
            Err(err) => {
                return Err(ProxyError::SanitizedAdapter(
                    self.sanitizer
                        .sanitize(&self.pipeline, &self.session, &err.to_string())?,
                ))
            }
        };

        rows.into_iter()
            .map(|row| self.pipeline.redact(&self.session, RawDocument::Structured(row)).map_err(ProxyError::from))
            .collect()
    }

    pub async fn log_tail(&self, limit: usize) -> Result<Vec<CleanDocument>, ProxyError> {
        let logs = self.logs.as_ref().ok_or_else(|| {
            ProxyError::SanitizedAdapter("log adapter unavailable".to_string())
        })?;
        let lines = match logs.tail(limit).await {
            Ok(lines) => lines,
            Err(err) => {
                return Err(ProxyError::SanitizedAdapter(
                    self.sanitizer
                        .sanitize(&self.pipeline, &self.session, &err.to_string())?,
                ))
            }
        };

        lines.into_iter()
            .map(|line| self.pipeline.redact(&self.session, RawDocument::Text(line)).map_err(ProxyError::from))
            .collect()
    }

    pub fn session(&self) -> &Session {
        &self.session
    }
}
