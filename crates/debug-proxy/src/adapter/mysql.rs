use std::collections::BTreeMap;

use async_trait::async_trait;
use gaze::Value;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use sqlx::{Column, Row, TypeInfo};

use crate::adapter::{AdapterError, ColumnSchema, ColumnType, DatabaseAdapter, TableSchema};

pub struct MysqlAdapter {
    pool: MySqlPool,
    database: String,
}

impl MysqlAdapter {
    pub async fn connect(url: &str) -> Result<Self, AdapterError> {
        let pool = MySqlPoolOptions::new()
            .max_connections(1)
            .connect(url)
            .await
            .map_err(|err| AdapterError::Connection(err.to_string()))?;
        let row: (Option<String>,) = sqlx::query_as("SELECT DATABASE()")
            .fetch_one(&pool)
            .await
            .map_err(|err| AdapterError::Query(err.to_string()))?;
        Ok(Self {
            pool,
            database: row.0.unwrap_or_default(),
        })
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub async fn raw_execute(&self, sql: &str) -> Result<(), AdapterError> {
        for statement in sql.split(';') {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            sqlx::query(statement)
                .execute(&self.pool)
                .await
                .map_err(|err| AdapterError::Query(err.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait]
impl DatabaseAdapter for MysqlAdapter {
    async fn tables(&self) -> Result<Vec<String>, AdapterError> {
        let rows = sqlx::query_as::<_, (String,)>(
            r#"
            SELECT CAST(TABLE_NAME AS CHAR) AS table_name
            FROM information_schema.TABLES
            WHERE TABLE_SCHEMA = ?
            ORDER BY TABLE_NAME
            "#,
        )
        .bind(&self.database)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| AdapterError::Query(err.to_string()))?;
        Ok(rows.into_iter().map(|(table,)| table).collect())
    }

    async fn schema(&self, table: &str) -> Result<TableSchema, AdapterError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
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
        .map_err(|err| AdapterError::Query(err.to_string()))?;
        if rows.is_empty() {
            return Err(AdapterError::UnknownTable(table.to_string()));
        }

        let mut columns = Vec::with_capacity(rows.len());
        let mut primary_key = Vec::new();
        for (name, data_type, is_nullable, column_key) in rows {
            if column_key.as_deref() == Some("PRI") {
                primary_key.push(name.clone());
            }
            columns.push(ColumnSchema {
                name,
                ty: mysql_type_to_column_type(&data_type),
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
        table: &str,
        limit: usize,
    ) -> Result<Vec<BTreeMap<String, Value>>, AdapterError> {
        let sql = format!("SELECT * FROM `{}` LIMIT ?", escape_ident(table));
        let rows = sqlx::query(&sql)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|err| AdapterError::Query(err.to_string()))?;

        Ok(rows.iter().map(row_to_values).collect())
    }

    async fn count(&self, table: &str) -> Result<u64, AdapterError> {
        let sql = format!("SELECT COUNT(*) AS n FROM `{}`", escape_ident(table));
        let (count,) = sqlx::query_as::<_, (i64,)>(&sql)
            .fetch_one(&self.pool)
            .await
            .map_err(|err| AdapterError::Query(err.to_string()))?;
        Ok(count.max(0) as u64)
    }

    async fn distinct(
        &self,
        table: &str,
        column: &str,
        limit: usize,
    ) -> Result<Vec<Value>, AdapterError> {
        let sql = format!(
            "SELECT DISTINCT `{column}` FROM `{table}` LIMIT ?",
            column = escape_ident(column),
            table = escape_ident(table)
        );
        let rows = sqlx::query(&sql)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|err| AdapterError::Query(err.to_string()))?;

        Ok(rows
            .iter()
            .map(|row| {
                let column = &row.columns()[0];
                decode_value(row, 0, column.type_info().name())
            })
            .collect())
    }
}

fn row_to_values(row: &sqlx::mysql::MySqlRow) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for (index, column) in row.columns().iter().enumerate() {
        let name = column.name().to_string();
        let value = decode_value(row, index, column.type_info().name());
        out.insert(name, value);
    }
    out
}

fn decode_value(row: &sqlx::mysql::MySqlRow, index: usize, ty: &str) -> Value {
    let upper = ty.to_ascii_uppercase();
    match upper.as_str() {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" | "INT UNSIGNED"
        | "BIGINT UNSIGNED" => match row.try_get::<Option<i64>, _>(index) {
            Ok(Some(value)) => Value::I64(value),
            Ok(None) => Value::String(String::new()),
            Err(err) => {
                tracing::warn!(
                    column_index = index,
                    column_type = %ty,
                    error = %err,
                    "mysql decode: integer column fell back to empty string"
                );
                Value::String(String::new())
            }
        },
        _ => match row.try_get::<Option<String>, _>(index) {
            Ok(Some(value)) => Value::String(value),
            Ok(None) => Value::String(String::new()),
            Err(err) => {
                tracing::warn!(
                    column_index = index,
                    column_type = %ty,
                    error = %err,
                    "mysql decode: unsupported column type fell back to empty string (potential PII silently dropped)"
                );
                Value::String(String::new())
            }
        },
    }
}

fn mysql_type_to_column_type(ty: &str) -> ColumnType {
    match ty.to_ascii_lowercase().as_str() {
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint" => ColumnType::Int,
        _ => ColumnType::Text,
    }
}

fn escape_ident(ident: &str) -> String {
    ident.replace('`', "``")
}
