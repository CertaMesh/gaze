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
fn audit_metadata_only_fails_on_nested_module_redaction_entry() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "mod _restore_probe {\n    use gaze::{ RedactionEntry };\n}\n\npub fn restore() {}\n",
    )
    .unwrap();

    assert_gate_rejects(dir.path(), "RedactionEntry");
}

#[test]
fn audit_metadata_only_fails_on_gaze_glob_import() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "use gaze::*;\n\npub fn restore() {}\n",
    )
    .unwrap();

    assert_gate_rejects(dir.path(), "redaction_log");
}

#[test]
fn audit_metadata_only_fails_on_gaze_renamed_crate_alias() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "use gaze as g;\n\npub fn restore() {}\n",
    )
    .unwrap();

    assert_gate_rejects(dir.path(), "__renamed_gaze_root__");
}

#[test]
fn audit_metadata_only_fails_on_function_body_use() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "pub fn restore() {\n    use gaze::RedactionEntry;\n}\n",
    )
    .unwrap();

    assert_gate_rejects(dir.path(), "RedactionEntry");
}

#[test]
fn audit_metadata_only_fails_on_impl_method_body_use() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "struct Foo;\n\nimpl Foo {\n    fn run(&self) {\n        use gaze::RedactionEntry;\n    }\n}\n",
    )
    .unwrap();

    assert_gate_rejects(dir.path(), "RedactionEntry");
}

#[test]
fn audit_metadata_only_fails_on_extern_crate_alias() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(restore_dir.join("mod.rs"), "extern crate gaze as g;\n").unwrap();

    assert_gate_rejects(dir.path(), "__renamed_gaze_root__");
}

#[test]
fn audit_metadata_only_fails_on_plain_extern_crate() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(restore_dir.join("mod.rs"), "extern crate gaze;\n").unwrap();

    assert_gate_rejects(dir.path(), "__renamed_gaze_root__");
}

#[test]
fn audit_metadata_only_fails_on_path_attribute_external_module() {
    let dir = tempdir().unwrap();
    let restore_dir = dir.path().join("crates/gaze-cli/src/restore");
    fs::create_dir_all(&restore_dir).unwrap();
    fs::write(
        restore_dir.join("mod.rs"),
        "#[path = \"../external.rs\"]\nmod _path_escape;\n",
    )
    .unwrap();
    fs::write(
        restore_dir.join("../external.rs"),
        "use gaze::RedactionEntry;\n",
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
