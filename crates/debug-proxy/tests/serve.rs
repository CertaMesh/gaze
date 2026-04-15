use std::fs;

use debug_proxy::cli::serve;
use debug_proxy::policy::PolicyFile;

#[test]
fn policy_requires_exactly_one_production_connection() {
    let err = PolicyFile::from_toml(
        r#"
        [policy.database]

        [[policy.database.columns]]
        column = "email"
        class = "email"
        "#,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("exactly one [connection.production]"));
}

#[test]
fn serve_fails_when_password_env_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let policy_path = temp.path().join("policy.toml");
    fs::write(
        &policy_path,
        r#"
        [connection.production]
        kind = "mysql"
        ssh_host = "deploy@example.com"
        local_port = 13306
        remote_host = "127.0.0.1"
        remote_port = 3306
        database = "app"
        user = "gaze_ro"
        password_env = "GAZE_MISSING_PASSWORD"

        [policy.database]

        [[policy.database.columns]]
        column = "email"
        class = "email"
        action = "tokenize"
        "#,
    )
    .expect("write policy");

    std::env::remove_var("GAZE_MISSING_PASSWORD");
    match serve::prepare(&policy_path) {
        Ok(_) => panic!("expected missing env var error"),
        Err(err) => assert!(err.to_string().contains("missing env var")),
    }
}

#[test]
fn serve_preparation_keeps_log_path_and_local_tunnel_url() {
    let temp = tempfile::tempdir().expect("tempdir");
    let policy_path = temp.path().join("policy.toml");
    fs::write(
        &policy_path,
        r#"
        [connection.production]
        kind = "mysql"
        ssh_host = "deploy@example.com"
        local_port = 13306
        remote_host = "10.0.0.12"
        remote_port = 3306
        database = "app"
        user = "gaze_ro"
        password_env = "GAZE_DB_PASSWORD"

        [policy.database]

        [[policy.database.columns]]
        column = "email"
        class = "email"
        action = "tokenize"

        [policy.logs]
        path = "/var/log/app/laravel.log"
        strip_patterns = ["(?i)password=.*"]
        "#,
    )
    .expect("write policy");

    std::env::set_var("GAZE_DB_PASSWORD", "secret");
    let prepared = serve::prepare(&policy_path).expect("prepare");

    assert_eq!(
        prepared
            .policy
            .policy
            .logs
            .as_ref()
            .and_then(|logs| logs.path.as_deref()),
        Some(std::path::Path::new("/var/log/app/laravel.log"))
    );
    assert_eq!(
        serve::mysql_url(&prepared.connection, &prepared.password),
        "mysql://gaze_ro:secret@127.0.0.1:13306/app"
    );
}
