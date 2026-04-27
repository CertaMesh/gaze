const QUERY: fn(gaze_audit::AuditFilter) -> String = gaze_audit::build_audit_query_sql;

fn main() {
    let _ = QUERY;
}
