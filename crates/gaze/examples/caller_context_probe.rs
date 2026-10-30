use gaze::{RecordValueRejectionReason, TypedContext};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
};

#[derive(Deserialize)]
struct Input {
    arm: String,
    context: String,
}

fn main() {
    let file = tempfile::NamedTempFile::new().expect("private context file");
    let mut counts = BTreeMap::<String, [usize; 3]>::new();
    for line in io::stdin().lock().lines() {
        let row: Input = serde_json::from_str(&line.unwrap()).unwrap();
        std::fs::write(file.path(), row.context).unwrap();
        let context = TypedContext::load(file.path()).expect("generated context must load");
        let rejected = context
            .record_value_rejections
            .iter()
            .filter(|r| matches!(r.reason, RecordValueRejectionReason::UnsafeShortMatch))
            .count();
        let c = counts.entry(row.arm).or_default();
        c[0] += 1;
        c[1] += rejected;
        c[2] += usize::from(rejected != 0);
    }
    assert_eq!(counts["positive-control"], [1, 1, 1]);
    println!(
        "{}",
        json!({"columns": ["contexts", "unsafe_short_values", "contexts_with_rejections"], "arms": counts})
    );
}
