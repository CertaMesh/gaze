struct RestoreRunner;

impl RestoreRunner {
    fn run() {
        use gaze_audit::SqliteLogger;
        let _ = SqliteLogger::new();
    }
}

fn main() {
    RestoreRunner::run();
}
