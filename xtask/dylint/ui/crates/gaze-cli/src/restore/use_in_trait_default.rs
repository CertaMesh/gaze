trait RestoreDefault {
    fn run() {
        use gaze_audit::SqliteLogger;
        let _ = SqliteLogger::new();
    }
}

struct Runner;

impl RestoreDefault for Runner {}

fn main() {
    Runner::run();
}
