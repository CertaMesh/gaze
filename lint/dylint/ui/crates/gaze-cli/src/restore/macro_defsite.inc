#[macro_export]
macro_rules! emit_forbidden_audit_use_from_protected_defsite {
    () => {
        use gaze_audit::SqliteLogger;
    };
}
