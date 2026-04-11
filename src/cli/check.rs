//! `gaze check` — parse `policy.toml` and print a human-readable summary
//! or a structured error. Exits nonzero on any validation failure.

use std::path::Path;

use crate::policy::parser::{load_from_file, PolicyError};

pub fn run(policy_path: &Path) -> Result<String, PolicyError> {
    let policy = load_from_file(policy_path)?;
    let (conn_name, conn) = policy
        .connection
        .iter()
        .next()
        .expect("validated to exactly one connection");
    let db = &policy.policy.database;
    Ok(format!(
        "OK — policy at {path}
  connection: {conn_name} → {kind}://{user}@{remote_host}:{remote_port}/{database}
                (tunnel: {ssh_host}, local :{local_port})
  allowed_tables: {tables}
  blocked_columns: {blocked}
  max_rows: {max_rows}
  max_distinct: {max_distinct}
  column_rules: {rules}",
        path = policy_path.display(),
        kind = conn.kind,
        user = conn.user,
        remote_host = conn.remote_host,
        remote_port = conn.remote_port,
        database = conn.database,
        ssh_host = conn.ssh_host,
        local_port = conn.local_port,
        tables = db.allowed_tables.join(", "),
        blocked = db.blocked_columns.join(", "),
        max_rows = db.max_rows,
        max_distinct = db.max_distinct,
        rules = db.column_rules.len(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn check_valid_policy_returns_summary() {
        let out = run(&fixture("policy_valid.toml")).unwrap();
        assert!(out.contains("OK"));
        assert!(out.contains("production"));
        assert!(out.contains("column_rules: 3"));
    }

    #[test]
    fn check_rejects_two_conns() {
        let err = run(&fixture("policy_two_conns.toml")).unwrap_err();
        assert!(matches!(err, PolicyError::ConnectionCount { .. }));
    }
}
