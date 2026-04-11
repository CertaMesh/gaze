//! MySQL adapter (sqlx + runtime-tokio-rustls). Connects with a URL like
//! `mysql://user:pass@host:port/db`. The host is normally `127.0.0.1`
//! with the SSH tunnel forwarding the real port.

use async_trait::async_trait;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use sqlx::Column;
use sqlx::Row;
use sqlx::TypeInfo;
use std::collections::BTreeMap;

use crate::adapter::{AdapterError, ColumnSchema, DatabaseAdapter, Filter, TableSchema};
use crate::types::{ColumnType, RawRow, Value};

pub struct MysqlAdapter {
    pool: MySqlPool,
    database: String,
}

impl MysqlAdapter {
    pub async fn connect(url: &str) -> Result<Self, AdapterError> {
        let pool = MySqlPoolOptions::new()
            .max_connections(1) // single-connection policy per spec
            .connect(url)
            .await
            .map_err(|e| AdapterError::Connection(e.to_string()))?;

        // Discover the database name from `SELECT DATABASE()`.
        let row: (Option<String>,) = sqlx::query_as("SELECT DATABASE()")
            .fetch_one(&pool)
            .await
            .map_err(|e| AdapterError::Query(e.to_string()))?;
        let database = row.0.unwrap_or_default();

        Ok(Self { pool, database })
    }

    /// Escape hatch used by the e2e harness to seed fixtures. Never call
    /// this from MCP handlers — it bypasses every policy check.
    #[cfg(any(test, feature = "test-utils"))]
    pub async fn raw_execute(&self, sql: &str) -> Result<(), AdapterError> {
        for stmt in sql.split(';') {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            sqlx::query(stmt)
                .execute(&self.pool)
                .await
                .map_err(|e| AdapterError::Query(e.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait]
impl DatabaseAdapter for MysqlAdapter {
    async fn schema(&self, table: &str) -> Result<TableSchema, AdapterError> {
        let cols: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT
                CAST(COLUMN_NAME AS CHAR) AS column_name,
                CAST(DATA_TYPE AS CHAR) AS data_type,
                CAST(IS_NULLABLE AS CHAR) AS is_nullable,
                CAST(COLUMN_KEY AS CHAR) AS column_key
            FROM information_schema.COLUMNS
            WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
            ORDER BY ORDINAL_POSITION
            "#,
        )
        .bind(&self.database)
        .bind(table)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AdapterError::Query(e.to_string()))?;

        if cols.is_empty() {
            return Err(AdapterError::UnknownTable(table.to_string()));
        }

        let mut columns = Vec::with_capacity(cols.len());
        let mut primary_key = Vec::new();
        for (name, data_type, is_nullable, column_key) in cols {
            let ty = mysql_type_to_column_type(&data_type);
            if column_key.as_deref() == Some("PRI") {
                primary_key.push(name.clone());
            }
            columns.push(ColumnSchema {
                name,
                ty,
                nullable: is_nullable == "YES",
            });
        }

        Ok(TableSchema {
            table: table.to_string(),
            columns,
            primary_key,
        })
    }

    async fn sample(
        &self,
        _table: &str,
        _filters: &[Filter],
        _limit: usize,
    ) -> Result<Vec<RawRow>, AdapterError> {
        // Lands in Task M2.3.
        Err(AdapterError::Query("unimplemented".into()))
    }

    async fn count(&self, _table: &str, _filters: &[Filter]) -> Result<u64, AdapterError> {
        Err(AdapterError::Query("unimplemented".into()))
    }

    async fn distinct(
        &self,
        _table: &str,
        _column: &str,
        _limit: usize,
    ) -> Result<Vec<RawRow>, AdapterError> {
        Err(AdapterError::Query("unimplemented".into()))
    }

    async fn explain(&self, _table: &str, _filters: &[Filter]) -> Result<String, AdapterError> {
        Err(AdapterError::Query("unimplemented".into()))
    }
}

fn mysql_type_to_column_type(ty: &str) -> ColumnType {
    match ty.to_ascii_lowercase().as_str() {
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint" => ColumnType::Int,
        "float" | "double" | "decimal" => ColumnType::Float,
        "char" | "varchar" | "text" | "tinytext" | "mediumtext" | "longtext" | "json" => {
            ColumnType::Text
        }
        "date" => ColumnType::Date,
        "datetime" | "timestamp" => ColumnType::DateTime,
        "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" => {
            ColumnType::Bytes
        }
        "bit" => ColumnType::Bool,
        _ => ColumnType::Text,
    }
}

/// Convert one sqlx MySQL row into our `RawRow`. Used by sample/distinct.
#[allow(dead_code)]
pub(crate) fn row_from_sqlx(row: &sqlx::mysql::MySqlRow) -> RawRow {
    let mut columns: BTreeMap<String, Value> = BTreeMap::new();
    for (idx, col) in row.columns().iter().enumerate() {
        let name = col.name().to_string();
        let value = decode_one(row, idx, col.type_info().name());
        columns.insert(name, value);
    }
    RawRow { columns }
}

#[allow(dead_code)]
fn decode_one(row: &sqlx::mysql::MySqlRow, idx: usize, type_name: &str) -> Value {
    match type_name.to_ascii_uppercase().as_str() {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" | "INT UNSIGNED"
        | "BIGINT UNSIGNED" => row
            .try_get::<Option<i64>, _>(idx)
            .ok()
            .flatten()
            .map(Value::Int)
            .unwrap_or(Value::Null),
        "FLOAT" | "DOUBLE" | "DECIMAL" => row
            .try_get::<Option<f64>, _>(idx)
            .ok()
            .flatten()
            .map(Value::Float)
            .unwrap_or(Value::Null),
        "BOOLEAN" | "BIT" => row
            .try_get::<Option<bool>, _>(idx)
            .ok()
            .flatten()
            .map(Value::Bool)
            .unwrap_or(Value::Null),
        _ => row
            .try_get::<Option<String>, _>(idx)
            .ok()
            .flatten()
            .map(Value::Text)
            .unwrap_or(Value::Null),
    }
}
