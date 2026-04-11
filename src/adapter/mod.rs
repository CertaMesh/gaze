//! Database + log adapter traits. Adapters return `RawRow`s — never
//! `CleanRow`. The anonymizer sits between the adapter and the MCP
//! handler, so every byte an adapter produces is funnelled through
//! `Anonymizer::clean()` before reaching the wire.

pub mod mysql;
pub mod ssh_tunnel;

use async_trait::async_trait;

use crate::types::{ColumnType, RawRow};

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

#[derive(Debug, Clone)]
pub enum FilterOp {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
    Like,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone)]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    /// Already-validated values. The policy engine checks these against
    /// the session map before handing them to the adapter.
    pub values: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
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
    async fn schema(&self, table: &str) -> Result<TableSchema, AdapterError>;
    async fn sample(
        &self,
        table: &str,
        filters: &[Filter],
        limit: usize,
    ) -> Result<Vec<RawRow>, AdapterError>;
    async fn count(&self, table: &str, filters: &[Filter]) -> Result<u64, AdapterError>;
    async fn distinct(
        &self,
        table: &str,
        column: &str,
        limit: usize,
    ) -> Result<Vec<RawRow>, AdapterError>;
    async fn explain(&self, table: &str, filters: &[Filter]) -> Result<String, AdapterError>;
}
