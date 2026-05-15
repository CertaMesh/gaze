#[cfg(feature = "safety-net-kiji")]
use gaze_recognizers::safety_net::kiji_distilbert::{
    KijiDistilbertBackend, KijiDistilbertPrecision, OrtKijiBackend, OrtKijiConfig,
    KIJI_DISTILBERT_BUNDLE_SHA256, KIJI_DISTILBERT_HF_COMMIT, KIJI_DISTILBERT_HF_REPO,
    KIJI_DISTILBERT_INT8_BUNDLE_SHA256, KIJI_DISTILBERT_INT8_SHA256SUMS,
    KIJI_DISTILBERT_SHA256SUMS,
};
#[cfg(feature = "safety-net-openai")]
use gaze_recognizers::safety_net::openai_filter::backend::subprocess::{
    OPF_CHECKPOINT_BUNDLE_SHA256, OPF_SOURCE_COMMIT, OPF_SOURCE_REPO, REQUIRED_OPF_ARTIFACTS,
};
use serde_json::Value;

const SNAPSHOT: &str = include_str!("safety_net_matrix_snapshot.json");
const PERF_SNAPSHOT: &str = include_str!("safety_net_perf_snapshot.json");

const BACKENDS: [&str; 3] = [
    "kiji_distilbert",
    "kiji_distilbert_int8",
    "openai_privacy_filter",
];
const LOCALES: [&str; 3] = ["Global", "EnUs", "DeDe"];
const MODES: [&str; 2] = ["direct_detector", "observer_residual"];

fn main() {
    #[cfg(feature = "safety-net-kiji")]
    if std::env::var("GAZE_SAFETY_NET_MATRIX_KIJI_BACKEND").as_deref() == Ok("ort") {
        run_live_ort_bench();
        return;
    }

    let snapshot: Value =
        serde_json::from_str(SNAPSHOT).expect("valid safety-net matrix benchmark snapshot");

    assert_eq!(snapshot["schema_version"], Value::from(2));
    assert_eq!(
        snapshot["benchmark"],
        Value::String("safety-net-multi-backend-matrix".to_string())
    );
    assert_kiji_pins(&snapshot);
    assert_opf_pins(&snapshot);
    assert_strict_span_leak_rate_shape(&snapshot);
    assert_cells(&snapshot);
    assert_kiji_int8_recall_gate(&snapshot);
    assert_perf_snapshot();

    println!("{SNAPSHOT}");
}

#[cfg(feature = "safety-net-kiji")]
fn run_live_ort_bench() {
    use std::time::Instant;

    let model_dir = std::env::var_os("GAZE_KIJI_DISTILBERT_MODEL_DIR")
        .expect("GAZE_KIJI_DISTILBERT_MODEL_DIR is required for live ORT bench");
    let cold_start = Instant::now();
    let precision = match std::env::var("GAZE_KIJI_DISTILBERT_PRECISION").as_deref() {
        Ok("int8") => KijiDistilbertPrecision::Int8,
        _ => KijiDistilbertPrecision::Fp32,
    };
    let backend = OrtKijiBackend::new(OrtKijiConfig::new(model_dir).with_precision(precision))
        .expect("load Kiji ORT backend");
    let cold_start_ms = cold_start.elapsed().as_secs_f64() * 1000.0;
    let fixture = "Alice Smith visited Berlin before meeting Dr. Schmidt at Example Corp.";
    backend.infer(fixture).expect("warmup inference");

    let mut samples = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        backend.infer(fixture).expect("inference");
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    let p50_ms = samples[samples.len() / 2];
    println!(
        "{{\"backend\":\"kiji_distilbert_ort\",\"precision\":\"{}\",\"cold_start_ms\":{cold_start_ms:.3},\"warm_p50_ms\":{p50_ms:.3},\"iterations\":{}}}",
        precision.as_str(),
        samples.len()
    );
}

#[cfg(feature = "safety-net-kiji")]
fn assert_kiji_pins(snapshot: &Value) {
    let pins = &snapshot["backends"]["kiji_distilbert"]["pins"];
    assert_eq!(
        pins["source_repo"],
        Value::String(KIJI_DISTILBERT_HF_REPO.to_string())
    );
    assert_eq!(
        pins["source_commit"],
        Value::String(KIJI_DISTILBERT_HF_COMMIT.to_string())
    );
    assert_eq!(
        pins["bundle_sha256"],
        Value::String(KIJI_DISTILBERT_BUNDLE_SHA256.to_string())
    );
    assert_eq!(
        pins["model_sha256"],
        Value::String(checksum_for("model.onnx").to_string())
    );
    assert_eq!(
        pins["tokenizer_sha256"],
        Value::String(checksum_for("tokenizer.json").to_string())
    );
    assert_eq!(
        pins["label_map_sha256"],
        Value::String(checksum_for("labels.json").to_string())
    );
    let int8_pins = &snapshot["backends"]["kiji_distilbert_int8"]["pins"];
    assert_eq!(
        int8_pins["source_repo"],
        Value::String(KIJI_DISTILBERT_HF_REPO.to_string())
    );
    assert_eq!(
        int8_pins["source_commit"],
        Value::String(KIJI_DISTILBERT_HF_COMMIT.to_string())
    );
    assert_eq!(
        int8_pins["bundle_sha256"],
        Value::String(KIJI_DISTILBERT_INT8_BUNDLE_SHA256.to_string())
    );
    assert_eq!(
        int8_pins["model_sha256"],
        Value::String(int8_checksum_for("model.int8.onnx").to_string())
    );
    assert_eq!(
        int8_pins["tokenizer_sha256"],
        Value::String(int8_checksum_for("tokenizer.json").to_string())
    );
    assert_eq!(
        int8_pins["label_map_sha256"],
        Value::String(int8_checksum_for("labels.json").to_string())
    );
}

#[cfg(feature = "safety-net-kiji")]
fn checksum_for(filename: &str) -> &str {
    KIJI_DISTILBERT_SHA256SUMS
        .lines()
        .find_map(|line| {
            let (sha256, path) = line.split_once("  ")?;
            (path == filename).then_some(sha256)
        })
        .expect("checksum present in Kiji SHA256SUMS")
}

#[cfg(feature = "safety-net-kiji")]
fn int8_checksum_for(filename: &str) -> &str {
    KIJI_DISTILBERT_INT8_SHA256SUMS
        .lines()
        .find_map(|line| {
            let (sha256, path) = line.split_once("  ")?;
            (path == filename).then_some(sha256)
        })
        .expect("checksum present in Kiji int8 SHA256SUMS")
}

#[cfg(not(feature = "safety-net-kiji"))]
fn assert_kiji_pins(snapshot: &Value) {
    let pins = snapshot["backends"]["kiji_distilbert"]["pins"]
        .as_object()
        .expect("Kiji pins object");
    for key in [
        "source_repo",
        "source_commit",
        "bundle_sha256",
        "model_sha256",
        "tokenizer_sha256",
        "label_map_sha256",
    ] {
        assert!(pins.contains_key(key), "missing Kiji pin key: {key}");
    }
    let int8_pins = snapshot["backends"]["kiji_distilbert_int8"]["pins"]
        .as_object()
        .expect("Kiji int8 pins object");
    for key in [
        "source_repo",
        "source_commit",
        "bundle_sha256",
        "model_sha256",
        "tokenizer_sha256",
        "label_map_sha256",
    ] {
        assert!(
            int8_pins.contains_key(key),
            "missing Kiji int8 pin key: {key}"
        );
    }
}

#[cfg(feature = "safety-net-openai")]
fn assert_opf_pins(snapshot: &Value) {
    let pins = snapshot["backends"]["openai_privacy_filter"]["pins"]
        .as_object()
        .expect("OPF pins object");
    assert_eq!(
        pins["source_repo"],
        Value::String(OPF_SOURCE_REPO.to_string())
    );
    assert_eq!(
        pins["source_commit"],
        Value::String(OPF_SOURCE_COMMIT.to_string())
    );
    assert_eq!(
        pins["checkpoint_bundle_sha256"],
        OPF_CHECKPOINT_BUNDLE_SHA256
            .map(|sha256| Value::String(sha256.to_string()))
            .unwrap_or(Value::Null)
    );
    assert_eq!(
        pins["required_opf_artifacts"],
        Value::Array(
            REQUIRED_OPF_ARTIFACTS
                .iter()
                .map(|artifact| Value::String((*artifact).to_string()))
                .collect()
        )
    );
    assert!(
        !pins.contains_key("binary_sha256"),
        "OPF pins must not include binary_sha256"
    );
}

#[cfg(not(feature = "safety-net-openai"))]
fn assert_opf_pins(snapshot: &Value) {
    let pins = snapshot["backends"]["openai_privacy_filter"]["pins"]
        .as_object()
        .expect("OPF pins object");
    for key in [
        "source_repo",
        "source_commit",
        "checkpoint_bundle_sha256",
        "required_opf_artifacts",
    ] {
        assert!(pins.contains_key(key), "missing OPF pin key: {key}");
    }
    assert!(
        !pins.contains_key("binary_sha256"),
        "OPF pins must not include binary_sha256"
    );
}

fn assert_strict_span_leak_rate_shape(snapshot: &Value) {
    let leak_rates = snapshot["strict_span_leak_rate"]
        .as_object()
        .expect("strict_span_leak_rate object");
    assert_eq!(leak_rates.len(), BACKENDS.len() * LOCALES.len());
    for backend in BACKENDS {
        for locale in LOCALES {
            let key = format!("{backend}.{locale}");
            let value = leak_rates
                .get(&key)
                .unwrap_or_else(|| panic!("missing strict_span_leak_rate for {key}"));
            assert_nullable_unit_float(value, &format!("strict_span_leak_rate {key}"));
        }
    }
}

fn assert_cells(snapshot: &Value) {
    let cells = snapshot["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), BACKENDS.len() * LOCALES.len() * MODES.len());

    for backend in BACKENDS {
        for locale in LOCALES {
            for mode in MODES {
                let cell = cells
                    .iter()
                    .find(|cell| {
                        cell["backend"] == backend
                            && cell["locale"] == locale
                            && cell["mode"] == mode
                    })
                    .unwrap_or_else(|| panic!("missing cell for {backend}.{locale}.{mode}"));
                assert_cell_schema(cell, mode);
            }
        }
    }
}

fn assert_kiji_int8_recall_gate(snapshot: &Value) {
    const MAX_RECALL_DELTA: f64 = 0.02;

    for locale in LOCALES {
        for mode in MODES {
            let fp32 = recall_for(snapshot, "kiji_distilbert", locale, mode);
            let int8 = recall_for(snapshot, "kiji_distilbert_int8", locale, mode);
            assert!(
                fp32 - int8 <= MAX_RECALL_DELTA,
                "kiji int8 recall regression exceeds {MAX_RECALL_DELTA:.2} for {locale}.{mode}: fp32={fp32:.6}, int8={int8:.6}"
            );
        }
    }
}

fn recall_for(snapshot: &Value, backend: &str, locale: &str, mode: &str) -> f64 {
    snapshot["cells"]
        .as_array()
        .expect("cells array")
        .iter()
        .find(|cell| cell["backend"] == backend && cell["locale"] == locale && cell["mode"] == mode)
        .and_then(|cell| cell["metrics"]["recall"].as_f64())
        .unwrap_or_else(|| panic!("missing numeric recall for {backend}.{locale}.{mode}"))
}

fn assert_cell_schema(cell: &Value, mode: &str) {
    assert!(cell.get("backend").is_some(), "cell missing backend");
    assert!(cell.get("locale").is_some(), "cell missing locale");
    assert!(cell.get("mode").is_some(), "cell missing mode");
    let metrics = cell["metrics"].as_object().expect("cell metrics object");
    for key in ["precision", "recall", "f1", "per_class"] {
        assert!(metrics.contains_key(key), "cell metrics missing {key}");
    }
    for key in ["precision", "recall", "f1"] {
        assert_nullable_unit_float(&metrics[key], key);
    }
    assert!(
        metrics["per_class"].as_object().is_some(),
        "per_class must be an object"
    );

    if mode == "observer_residual" {
        for key in [
            "observer_residual_recall",
            "agreement_with_rule_floor",
            "expansion_fraction",
            "contradiction_fraction",
            "novel_tp_over_rule_floor",
        ] {
            let value = metrics
                .get(key)
                .unwrap_or_else(|| panic!("observer_residual metrics missing {key}"));
            assert_nullable_unit_float(value, key);
        }
    }
}

fn assert_perf_snapshot() {
    let snapshot: Value =
        serde_json::from_str(PERF_SNAPSHOT).expect("valid safety-net perf benchmark snapshot");
    assert_eq!(snapshot["schema_version"], Value::from(1));
    assert_eq!(
        snapshot["benchmark"],
        Value::String("safety-net-perf".to_string())
    );
    let backends = snapshot["backends"]
        .as_object()
        .expect("perf backends object");
    for backend in BACKENDS {
        let metrics = backends
            .get(backend)
            .unwrap_or_else(|| panic!("missing perf backend {backend}"))
            .as_object()
            .expect("perf backend metrics object");
        for key in [
            "cold_start_ms",
            "per_fixture_median_ms",
            "per_fixture_p95_ms",
            "per_fixture_p99_ms",
            "per_fixture_mean_ms",
            "throughput_bytes_per_sec",
        ] {
            assert_positive_number(&metrics[key], key);
        }
        assert_eq!(metrics["fixture_count"], Value::from(150));
        assert_eq!(metrics["device"], Value::String("cpu".to_string()));
        assert!(
            metrics["measurement_scope"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "measurement_scope must be a non-empty string"
        );
    }
    let host = snapshot["host"].as_object().expect("perf host object");
    for key in ["os", "arch", "cpu", "python"] {
        assert!(
            host.get(key).is_some_and(Value::is_string),
            "perf host missing string {key}"
        );
    }
}

fn assert_positive_number(value: &Value, name: &str) {
    let number = value
        .as_f64()
        .unwrap_or_else(|| panic!("{name} must be numeric"));
    assert!(number > 0.0, "{name} must be positive");
}

fn assert_nullable_unit_float(value: &Value, name: &str) {
    if value.is_null() {
        return;
    }
    let number = value
        .as_f64()
        .unwrap_or_else(|| panic!("{name} must be null or numeric"));
    assert!(
        (0.0..=1.0).contains(&number),
        "{name} must be between 0.0 and 1.0"
    );
}
