use serde_json::Value;

const SNAPSHOT: &str =
    include_str!("../../gaze-recognizers/benches/gaze_pipeline_bench_snapshot.json");

const CONFIGS: [&str; 5] = [
    "rule-floor-core",
    "rule-floor-extended",
    "pass3-kiji",
    "pass3-opf",
    "pass3-locale-aware",
];
const LOCALES: [&str; 3] = ["Global", "EnUs", "DeDe"];
const LABELS: [&str; 7] = [
    "Email",
    "IBAN",
    "credit_card",
    "phone",
    "postal_code",
    "Name-first",
    "Name-surname",
];

fn main() {
    let snapshot: Value =
        serde_json::from_str(SNAPSHOT).expect("valid gaze pipeline benchmark snapshot");
    assert_eq!(snapshot["schema_version"], Value::from(1));
    assert_eq!(
        snapshot["benchmark"],
        Value::String("gaze-pipeline-end-to-end".to_string())
    );
    assert_eq!(snapshot["corpus"]["fixture_count"], Value::from(150));
    assert_eq!(
        snapshot["corpus"]["sha256"],
        Value::String(
            "c6e78cca59df550fad18e59e9877da03da82c73b80c2368e5233d76353ccfa2f".to_string()
        )
    );
    assert_cells(&snapshot);
    println!("{SNAPSHOT}");
}

fn assert_cells(snapshot: &Value) {
    let cells = snapshot["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), CONFIGS.len() * LOCALES.len());
    for config in CONFIGS {
        for locale in LOCALES {
            let cell = cells
                .iter()
                .find(|cell| cell["pipeline_config"] == config && cell["locale"] == locale)
                .unwrap_or_else(|| panic!("missing cell for {config}.{locale}"));
            assert!(cell["status"].is_string(), "cell status must be a string");
            if cell["status"] == "measured" {
                assert_detection(cell);
                assert_performance(cell);
            } else {
                assert!(
                    cell["reason"].is_string(),
                    "unavailable cells must include a reason"
                );
            }
        }
    }
}

fn assert_detection(cell: &Value) {
    let detection = &cell["detection"];
    for key in ["strict_recall", "strict_leak_rate", "precision", "f1"] {
        assert_nullable_unit_float(&detection[key], key);
    }
    let per_class = detection["per_class_recall"]
        .as_object()
        .expect("per_class_recall object");
    for label in LABELS {
        assert!(
            per_class.contains_key(label),
            "per_class_recall missing {label}"
        );
        assert_nullable_unit_float(&per_class[label], label);
    }
}

fn assert_performance(cell: &Value) {
    let performance = &cell["performance"];
    for key in [
        "cold_start_ms",
        "mean_ms",
        "p50_ms",
        "p95_ms",
        "p99_ms",
        "throughput_bytes_per_sec",
    ] {
        assert_nonnegative_number(&performance[key], key);
    }
    let pass_timing = &performance["pass_timing"];
    assert_nonnegative_number(&pass_timing["pass1_ms"], "pass1_ms");
    assert!(pass_timing["pass2_ms"].is_null(), "pass2_ms must be null");
    assert_nonnegative_number(&pass_timing["pass3_ms"], "pass3_ms");
}

fn assert_nullable_unit_float(value: &Value, label: &str) {
    if value.is_null() {
        return;
    }
    let Some(number) = value.as_f64() else {
        panic!("{label} must be null or number");
    };
    assert!(
        (0.0..=1.0).contains(&number),
        "{label} must be between 0 and 1"
    );
}

fn assert_nonnegative_number(value: &Value, label: &str) {
    let Some(number) = value.as_f64() else {
        panic!("{label} must be a number");
    };
    assert!(number >= 0.0, "{label} must be nonnegative");
}
