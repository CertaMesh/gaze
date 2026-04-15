use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use debug_proxy::adapter::{AdapterError, DatabaseAdapter, LogAdapter, ToolContext};
use debug_proxy::mcp::errors::CANARY;
use debug_proxy::policy::{build_pipeline, PolicyFile};
use gaze::{CleanDocument, Scope, Session, Value};

struct FakeDb;

#[async_trait]
impl DatabaseAdapter for FakeDb {
    async fn sample(
        &self,
        table: &str,
        _limit: usize,
    ) -> Result<Vec<BTreeMap<String, Value>>, AdapterError> {
        if table != "users" {
            return Err(AdapterError::UnknownTable(table.to_string()));
        }

        Ok(vec![BTreeMap::from([(
            "email".to_string(),
            Value::String(CANARY.to_string()),
        )])])
    }
}

struct FakeLogs;

#[async_trait]
impl LogAdapter for FakeLogs {
    async fn tail(&self, _limit: usize) -> Result<Vec<String>, AdapterError> {
        Ok(vec![format!("request failed for user={CANARY}")])
    }

    async fn search(
        &self,
        _pattern: &str,
        _level: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<String>, AdapterError> {
        Ok(Vec::new())
    }

    async fn context(&self, _request_id: &str) -> Result<Vec<String>, AdapterError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn canary_never_leaks_and_pseudonyms_match_across_channels() {
    let policy = PolicyFile::from_toml(
        r#"
        [policy.database]

        [[policy.database.columns]]
        column = "email"
        class = "email"
        action = "tokenize"
        "#,
    )
    .expect("policy");
    let pipeline = build_pipeline(&policy).expect("pipeline");
    let session = Arc::new(Session::new(Scope::Conversation("req-1".to_string())).expect("session"));
    let ctx = ToolContext::new(pipeline, session, Arc::new(FakeDb), Some(Arc::new(FakeLogs)));

    let db_rows = ctx.db_sample("users", 10).await.expect("db sample");
    let log_rows = ctx.log_tail(10).await.expect("log tail");

    let CleanDocument::Structured(fields) = &db_rows[0] else {
        panic!("expected structured row");
    };
    let db_token = fields["email"].as_str().expect("token string").to_string();

    let CleanDocument::Text(log_text) = &log_rows[0] else {
        panic!("expected text row");
    };

    assert_eq!(db_token, "Email_1");
    assert!(log_text.contains(&db_token));
    assert!(!log_text.contains(CANARY));
}

#[tokio::test]
async fn adapter_errors_are_sanitized_through_pipeline() {
    struct BrokenDb;

    #[async_trait]
    impl DatabaseAdapter for BrokenDb {
        async fn sample(
            &self,
            _table: &str,
            _limit: usize,
        ) -> Result<Vec<BTreeMap<String, Value>>, AdapterError> {
            Err(AdapterError::Query(format!("duplicate key for {CANARY}")))
        }
    }

    let policy = PolicyFile::from_toml(
        r#"
        [policy.database]

        [[policy.database.columns]]
        column = "email"
        class = "email"
        action = "tokenize"
        "#,
    )
    .expect("policy");
    let pipeline = build_pipeline(&policy).expect("pipeline");
    let session = Arc::new(Session::new(Scope::Conversation("req-1".to_string())).expect("session"));
    let ctx = ToolContext::<BrokenDb, FakeLogs>::new(pipeline, session, Arc::new(BrokenDb), None);

    let err = ctx.db_sample("users", 10).await.unwrap_err().to_string();
    assert!(!err.contains(CANARY));
    assert!(err.contains("Email_1"));
}
