use std::path::PathBuf;
use std::process::Command;

#[test]
fn ui() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let aux_dir = manifest_dir.join("target/ui-aux");
    std::fs::create_dir_all(&aux_dir).expect("failed to create UI auxiliary output dir");
    let aux_rlib = aux_dir.join("libgaze_audit.rlib");
    let status = Command::new("rustc")
        .args([
            "--crate-name",
            "gaze_audit",
            "--crate-type",
            "rlib",
            "--edition",
            "2021",
        ])
        .arg(manifest_dir.join("ui/auxiliary/gaze_audit.rs"))
        .arg("-o")
        .arg(&aux_rlib)
        .status()
        .expect("failed to build UI auxiliary gaze_audit crate");
    assert!(status.success(), "failed to build UI auxiliary gaze_audit crate");

    let dylint_toml = r#"
[gaze_dylint]
protected_paths = ["crates/gaze-cli/src/restore", "crates/gaze/src"]
forbidden_crates = ["gaze_audit"]
forbidden_items = [
    "gaze_audit::SqliteLogger",
    "gaze_audit::AuditFilter",
    "gaze_audit::AuditLogRow",
    "gaze_audit::build_audit_query_sql",
    "gaze_audit::AUDIT_RESTRICTED_COLUMNS",
]
"#;
    dylint_testing::ui::Test::src_base(env!("CARGO_PKG_NAME"), "ui")
        .dylint_toml(dylint_toml)
        .rustc_flags([
            "--edition=2021".to_owned(),
            "--extern".to_owned(),
            format!("gaze_audit={}", aux_rlib.display()),
        ])
        .run();
}
