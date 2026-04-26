#[macro_export]
macro_rules! emit_forbidden_audit_use {
    () => {
        use gaze_audit::SqliteLogger;
    };
}
