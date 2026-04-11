//! End-to-end tests against a real MySQL 8 container. Skipped automatically
//! if Docker isn't reachable (testcontainers returns an error).

use gaze::adapter::mysql::MysqlAdapter;
use gaze::adapter::DatabaseAdapter;
use gaze::types::ColumnType;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;

async fn boot() -> (MysqlAdapter, testcontainers::ContainerAsync<Mysql>) {
    let container = Mysql::default().start().await.expect("docker/mysql start");
    let port = container
        .get_host_port_ipv4(3306)
        .await
        .expect("mysql port");
    let url = format!("mysql://root@127.0.0.1:{port}/test");
    let adapter = MysqlAdapter::connect(&url).await.expect("connect to mysql");

    // Seed a minimal schema.
    adapter
        .raw_execute(
            r#"
            CREATE TABLE users (
                id BIGINT PRIMARY KEY,
                email VARCHAR(191) NOT NULL,
                created_at DATETIME NOT NULL
            );
            INSERT INTO users VALUES
                (1, 'krishan@example.com', '2025-01-01 12:00:00'),
                (2, 'alice@example.com',   '2025-01-02 12:00:00'),
                (3, 'bob@example.com',     '2025-01-03 12:00:00');
        "#,
        )
        .await
        .expect("seed");

    (adapter, container)
}

#[tokio::test]
async fn schema_returns_columns_and_pk() {
    let (adapter, _c) = boot().await;
    let schema = adapter.schema("users").await.expect("schema");
    assert_eq!(schema.table, "users");
    let col_names: Vec<_> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    assert!(col_names.contains(&"id"));
    assert!(col_names.contains(&"email"));
    assert_eq!(schema.primary_key, vec!["id".to_string()]);
    let id_col = schema.columns.iter().find(|c| c.name == "id").unwrap();
    assert_eq!(id_col.ty, ColumnType::Int);
}
