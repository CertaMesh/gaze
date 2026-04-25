#![cfg(feature = "test-utils")]

use debug_proxy::adapter::mysql::MysqlAdapter;
use debug_proxy::adapter::DatabaseAdapter;
use gaze::Value;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;

async fn boot() -> Option<(MysqlAdapter, testcontainers::ContainerAsync<Mysql>)> {
    let container = match Mysql::default().start().await {
        Ok(container) => container,
        Err(_) => return None,
    };
    let port = container.get_host_port_ipv4(3306).await.ok()?;
    let url = format!("mysql://root@127.0.0.1:{port}/test");
    let adapter = MysqlAdapter::connect(&url).await.ok()?;

    adapter
        .raw_execute(
            r#"
            CREATE TABLE users (
                id BIGINT PRIMARY KEY,
                email VARCHAR(191) NOT NULL
            );
            INSERT INTO users VALUES
                (1, 'krishan@example.com'),
                (2, 'alice@example.com'),
                (3, 'bob@example.com');
            "#,
        )
        .await
        .ok()?;

    Some((adapter, container))
}

#[tokio::test]
async fn sample_returns_rows_from_mysql() {
    let Some((adapter, _container)) = boot().await else {
        return;
    };
    let rows = adapter.sample("users", 10).await.expect("sample");
    assert_eq!(rows.len(), 3);
    assert!(matches!(rows[0].get("id"), Some(Value::I64(_))));
    assert!(matches!(rows[0].get("email"), Some(Value::String(_))));
}

#[tokio::test]
async fn sample_respects_limit() {
    let Some((adapter, _container)) = boot().await else {
        return;
    };
    let rows = adapter.sample("users", 2).await.expect("sample");
    assert_eq!(rows.len(), 2);
}
