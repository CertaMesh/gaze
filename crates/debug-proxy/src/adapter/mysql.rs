use std::collections::BTreeMap;

use async_trait::async_trait;
use gaze::Value;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use sqlx::{Column, Row, TypeInfo};

use crate::adapter::{AdapterError, DatabaseAdapter};

pub struct MysqlAdapter {
    pool: MySqlPool,
}

impl MysqlAdapter {
    pub async fn connect(url: &str) -> Result<Self, AdapterError> {
        let pool = MySqlPoolOptions::new()
            .max_connections(1)
            .connect(url)
            .await
            .map_err(|err| AdapterError::Connection(err.to_string()))?;
        Ok(Self { pool })
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
    match ty.to_ascii_uppercase().as_str() {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" | "INT UNSIGNED"
        | "BIGINT UNSIGNED" => row
            .try_get::<Option<i64>, _>(index)
            .ok()
            .flatten()
            .map(Value::I64)
            .unwrap_or_else(|| Value::String(String::new())),
        _ => row
            .try_get::<Option<String>, _>(index)
            .ok()
            .flatten()
            .map(Value::String)
            .unwrap_or_else(|| Value::String(String::new())),
    }
}

fn escape_ident(ident: &str) -> String {
    ident.replace('`', "``")
}
