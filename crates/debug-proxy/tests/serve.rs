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
