use std::fs;

use debug_proxy::cli::{check, init};

fn valid_policy() -> &'static str {
    r#"
[ner]
locale = "de"

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

[policy.logs]
strip_patterns = ["(?i)password[=:][^ ]+"]
"#
}

#[test]
fn init_scaffolds_policy_and_gaze_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    init::run(temp.path()).expect("init");

    assert!(temp.path().join("policy.toml").exists());
    assert!(temp.path().join(".gaze").is_dir());

    let gitignore = fs::read_to_string(temp.path().join(".gitignore")).expect("gitignore");
    assert!(gitignore.contains(".gaze/"));
}

#[test]
fn init_refuses_to_overwrite_existing_policy() {
    let temp = tempfile::tempdir().expect("tempdir");
    init::run(temp.path()).expect("init once");

    let err = init::run(temp.path()).unwrap_err().to_string();
    assert!(err.contains("policy.toml already exists"));
}

#[test]
fn check_summarizes_valid_policy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let policy_path = temp.path().join("policy.toml");
    fs::write(&policy_path, valid_policy()).expect("write policy");

    let summary = check::run(&policy_path).expect("check");
    assert!(summary.contains("OK"));
    assert!(summary.contains("locale: de"));
    assert!(summary.contains("column_rules: 1"));
    assert!(summary.contains("log_strip_patterns: 1"));
}

#[test]
fn check_rejects_invalid_action() {
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
 password_env = "GAZE_DB_PASSWORD"

[policy.database]

[[policy.database.columns]]
column = "email"
class = "email"
action = "explode"
"#,
    )
    .expect("write policy");

    let err = check::run(&policy_path).unwrap_err().to_string();
    assert!(err.contains("invalid action"));
}
