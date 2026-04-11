//! End-to-end canary leak guard for db.sample.
//!
//! Seeds the canary email in a real MySQL testcontainer, drives the full
//! db_sample handler path (adapter → anonymizer → serialization), and asserts
//! the literal sentinel never appears in the returned JSON.
//!
//! Run with: cargo test --features test-utils --test e2e_canary

#![cfg(feature = "test-utils")]

use std::collections::HashMap;
use std::sync::Arc;

use gaze::adapter::mysql::MysqlAdapter;
use gaze::anon::Anonymizer;
use gaze::audit::AuditLog;
use gaze::mcp::errors::{ErrorSanitizer, CANARY};
use gaze::mcp::tools::{SampleArgs, ToolContext};
use gaze::policy::classifier::{Classifier, PiiClass};
use gaze::policy::parser::{ConnectionConfig, DatabasePolicy, Policy, PolicySection};
use gaze::scanner::Scanner;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;

async fn boot_ctx() -> (ToolContext, testcontainers::ContainerAsync<Mysql>) {
    let container = Mysql::default().start().await.expect("docker/mysql start");
    let port = container
        .get_host_port_ipv4(3306)
        .await
        .expect("mysql port");
    let url = format!("mysql://root@127.0.0.1:{port}/test");
    let adapter = MysqlAdapter::connect(&url).await.expect("connect to mysql");

    // Seed minimal schema with canary row.
    adapter
        .raw_execute(
            r#"
            CREATE TABLE users (
                id BIGINT PRIMARY KEY,
                email VARCHAR(191) NOT NULL
            );
            "#,
        )
        .await
        .expect("seed schema");
    adapter
        .raw_execute(&format!(
            "INSERT INTO users VALUES (1, '{CANARY}');"
        ))
        .await
        .expect("seed canary row");

    // Build a minimal policy with one connection.production block.
    let mut connection = HashMap::new();
    connection.insert(
        "production".to_string(),
        ConnectionConfig {
            kind: "mysql".to_string(),
            ssh_host: "localhost".to_string(),
            local_port: port,
            remote_host: "127.0.0.1".to_string(),
            remote_port: 3306,
            database: "test".to_string(),
            user: "root".to_string(),
            password_env: "UNUSED".to_string(),
        },
    );

    let policy = Policy {
        connection,
        policy: PolicySection {
            database: DatabasePolicy {
                allowed_tables: vec!["users".to_string()],
                blocked_columns: vec![],
                max_rows: 50,
                max_distinct: 50,
                count_allowed_columns: vec![],
                distinct_allowed_columns: vec![],
                allowed_operations: vec![],
                column_rules: vec![
                    gaze::policy::parser::ColumnRule {
                        table: "users".to_string(),
                        column: "id".to_string(),
                        class: "id".to_string(),
                    },
                    gaze::policy::parser::ColumnRule {
                        table: "users".to_string(),
                        column: "email".to_string(),
                        class: "email".to_string(),
                    },
                ],
            },
            logs: None,
        },
    };

    let classifier = Classifier::new()
        .with_column("id", PiiClass::Id)
        .with_column("email", PiiClass::Email);

    let anonymizer = Arc::new(Anonymizer::new(classifier));

    // In-memory audit log backed by SQLite.
    let audit_path = std::env::temp_dir().join(format!(
        "gaze_canary_test_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let audit = Arc::new(AuditLog::open(&audit_path).expect("audit log"));

    let ctx = ToolContext {
        policy,
        adapter: Arc::new(adapter),
        anonymizer,
        audit,
        sanitizer: ErrorSanitizer::default(),
        log_adapter: None,
        scanner: Scanner::compile(&[]).unwrap(),
    };

    (ctx, container)
}

#[tokio::test]
async fn canary_never_leaks_through_db_sample() {
    let (ctx, _c) = boot_ctx().await;

    let result = ctx
        .db_sample(SampleArgs {
            env: "production".to_string(),
            table: "users".to_string(),
            filters: vec![],
            limit: Some(10),
        })
        .await
        .unwrap();

    let json = serde_json::to_string(&result.rows).unwrap();
    assert!(
        !json.contains(CANARY),
        "canary leaked into db_sample output: {json}"
    );
}
