use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use debug_proxy::adapter::{
    AdapterError, ColumnSchema, ColumnType, DatabaseAdapter, LogAdapter, TableSchema, ToolContext,
};
use debug_proxy::mcp::errors::CANARY;
use debug_proxy::policy::{build_pipeline, PolicyFile};
use gaze::{Scope, Session, Value};

struct FakeDb;

#[async_trait]
impl DatabaseAdapter for FakeDb {
    async fn tables(&self) -> Result<Vec<String>, AdapterError> {
        Ok(vec!["users".to_string(), "orders".to_string()])
    }

    async fn schema(&self, table: &str) -> Result<TableSchema, AdapterError> {
        if table != "users" {
            return Err(AdapterError::UnknownTable(table.to_string()));
        }

        Ok(TableSchema {
            table: "users".to_string(),
            columns: vec![
                ColumnSchema {
                    name: "id".to_string(),
                    ty: ColumnType::Int,
                    nullable: false,
                },
                ColumnSchema {
                    name: "email".to_string(),
                    ty: ColumnType::Text,
                    nullable: true,
                },
            ],
            primary_key: vec!["id".to_string()],
        })
    }

    async fn sample(
        &self,
        table: &str,
        _limit: usize,
    ) -> Result<Vec<BTreeMap<String, Value>>, AdapterError> {
        if table != "users" {
            return Err(AdapterError::UnknownTable(table.to_string()));
        }

        Ok(vec![BTreeMap::from([
            ("id".to_string(), Value::I64(1)),
            ("email".to_string(), Value::String(CANARY.to_string())),
        ])])
    }

    async fn count(&self, table: &str) -> Result<u64, AdapterError> {
        if table != "users" {
            return Err(AdapterError::UnknownTable(table.to_string()));
        }
        Ok(2)
    }

    async fn distinct(
        &self,
        table: &str,
        column: &str,
        _limit: usize,
    ) -> Result<Vec<Value>, AdapterError> {
        if table != "users" {
            return Err(AdapterError::UnknownTable(table.to_string()));
        }
        if column != "email" {
            return Err(AdapterError::Query(format!("unknown column: {column}")));
        }

        Ok(vec![
            Value::String(CANARY.to_string()),
            Value::String("other@example.com".to_string()),
        ])
    }
}

struct FakeLogs;

#[async_trait]
impl LogAdapter for FakeLogs {
    async fn tail(&self, _limit: usize) -> Result<Vec<String>, AdapterError> {
        Ok(vec![format!("tail user={CANARY}")])
    }

    async fn search(
        &self,
        pattern: &str,
        _level: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<String>, AdapterError> {
        Ok(vec![format!("match {pattern} user={CANARY}")])
    }

    async fn context(&self, request_id: &str) -> Result<Vec<String>, AdapterError> {
        Ok(vec![format!("request_id={request_id} user={CANARY}")])
    }
}

fn make_ctx() -> ToolContext<FakeDb, FakeLogs> {
    let policy = PolicyFile::from_toml(
        r#"
        [connection.production]
        kind = "mysql"
        ssh_host = "deploy@example.com"
        local_port = 13306
        remote_host = "127.0.0.1"
        remote_port = 3306
        database = "app"
        user = "gaze_ro"
        password_env = "GAZE_DB_PASSWORD"

        [policy.database]

        [[policy.database.columns]]
        column = "email"
        class = "email"
        action = "tokenize"
        "#,
    )
    .expect("policy");
    let policy = Arc::new(policy);
    let pipeline = build_pipeline(&policy).expect("pipeline");
    let session =
        Arc::new(Session::new(Scope::Conversation("req-1".to_string())).expect("session"));
    ToolContext::with_policy(
        pipeline,
        session,
        Some(policy),
        Arc::new(FakeDb),
        Some(Arc::new(FakeLogs)),
    )
}

fn email_token(value: &serde_json::Value, index: usize) -> String {
    let token = value.as_str().expect("token string").to_string();
    assert!(token.starts_with('<'));
    assert!(token.ends_with(&format!(":Email_{index}>")));
    token
}

#[tokio::test]
async fn db_tables_returns_all_names() {
    let ctx = make_ctx();
    let tables = ctx.db_tables().await.expect("tables");
    assert_eq!(tables, vec!["users".to_string(), "orders".to_string()]);
}

#[tokio::test]
async fn db_schema_exposes_pii_class_from_policy() {
    let ctx = make_ctx();
    let schema = ctx.db_schema("users").await.expect("schema");
    assert_eq!(schema.table, "users");
    assert_eq!(schema.primary_key, vec!["id".to_string()]);
    assert_eq!(schema.columns[0].name, "id");
    assert_eq!(schema.columns[0].pii_class, "none");
    assert_eq!(schema.columns[1].name, "email");
    assert_eq!(schema.columns[1].pii_class, "email");
}

#[tokio::test]
async fn db_count_returns_adapter_count() {
    let ctx = make_ctx();
    let count = ctx.db_count("users").await.expect("count");
    assert_eq!(count, 2);
}

#[tokio::test]
async fn db_distinct_redacts_values_with_shared_session() {
    let ctx = make_ctx();
    let values = ctx
        .db_distinct("users", "email", 10)
        .await
        .expect("distinct");
    assert_eq!(values.len(), 2);
    let first = email_token(&values[0], 1);
    let second = email_token(&values[1], 2);
    assert_ne!(first, second);
}

#[tokio::test]
async fn logs_search_and_context_redact_text() {
    let ctx = make_ctx();
    let search = ctx
        .logs_search("integrity", None, 10)
        .await
        .expect("search");
    let context = ctx.logs_context("req-1").await.expect("context");

    assert_eq!(search.len(), 1);
    assert_eq!(context.len(), 1);
    assert!(!search[0].contains(CANARY));
    assert!(!context[0].contains(CANARY));
    let marker = format!(":Email_{}>", 1);
    assert!(search[0].contains('<') && search[0].contains(&marker));
    assert!(context[0].contains('<') && context[0].contains(&marker));
}
