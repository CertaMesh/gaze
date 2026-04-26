use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn audit_metadata_only_fails_when_restore_imports_redaction_entry() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "use gaze::RedactionEntry;\n\npub fn restore() {}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("audit-metadata-only")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "gate unexpectedly passed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("RedactionEntry"),
        "stderr should name forbidden symbol, got: {stderr}"
    );
}

#[test]
fn audit_metadata_only_fails_when_restore_imports_multiline_redaction_entry() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "use gaze::{\n    RedactionEntry,\n};\n\npub fn restore() {}\n",
    )
    .unwrap();

    assert_gate_rejects(dir.path(), "RedactionEntry");
}

#[test]
fn audit_metadata_only_fails_for_each_forbidden_audit_symbol() {
    for symbol in [
        "redaction_log",
        "ConflictTier",
        "DocumentKind",
        "RedactionEntry",
        "RedactionLogger",
        "SqliteLogger",
        "AuditFilter",
        "AuditLogRow",
        "AUDIT_RESTRICTED_COLUMNS",
        "build_audit_query_sql",
        "current_epoch_ms",
    ] {
        let dir = tempdir().unwrap();
        let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
        fs::create_dir_all(&restore_dir).unwrap();
        fs::write(
            restore_dir.join("mod.rs"),
            format!("use gaze::{symbol};\n\npub fn restore() {{}}\n"),
        )
        .unwrap();

        assert_gate_rejects(dir.path(), symbol);
    }
}

fn assert_gate_rejects(root: &std::path::Path, symbol: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("audit-metadata-only")
        .current_dir(root)
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "gate unexpectedly passed for {symbol}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(symbol),
        "stderr should name forbidden symbol {symbol}, got: {stderr}"
    );
}

#[test]
fn audit_metadata_only_passes_when_restore_imports_no_audit_metadata() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "use crate::error::CliError;\n\npub fn restore(_: CliError) {}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("audit-metadata-only")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "gate failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
