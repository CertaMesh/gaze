fn restore<T>()
where
    T: gaze_audit::AuditSink,
{
}

fn main() {
    restore::<gaze_audit::SqliteLogger>();
}
