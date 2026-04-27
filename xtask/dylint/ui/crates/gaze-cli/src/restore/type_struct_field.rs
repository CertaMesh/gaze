struct RestoreState {
    logger: gaze_audit::SqliteLogger,
}

fn main() {
    let _ = RestoreState {
        logger: gaze_audit::SqliteLogger::new(),
    };
}
