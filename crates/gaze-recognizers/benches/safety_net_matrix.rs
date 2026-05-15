#[cfg(feature = "safety-net-kiji")]
use gaze_recognizers::safety_net::kiji_distilbert::{
    KIJI_DISTILBERT_BUNDLE_SHA256, KIJI_DISTILBERT_HF_COMMIT, KIJI_DISTILBERT_HF_REPO,
    KIJI_DISTILBERT_SHA256SUMS,
};
#[cfg(feature = "safety-net-openai")]
use gaze_recognizers::safety_net::openai_filter::backend::subprocess::{
    OPF_CHECKPOINT_BUNDLE_SHA256, OPF_SOURCE_COMMIT, OPF_SOURCE_REPO, REQUIRED_OPF_ARTIFACTS,
};
use serde_json::Value;

const SNAPSHOT: &str = include_str!("safety_net_matrix_snapshot.json");

const BACKENDS: [&str; 2] = ["kiji_distilbert", "openai_privacy_filter"];
const LOCALES: [&str; 3] = ["Global", "EnUs", "DeDe"];
const MODES: [&str; 2] = ["direct_detector", "observer_residual"];

fn main() {
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

    println!("{SNAPSHOT}");
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

fn assert_cell_schema(cell: &Value, mode: &str) {
    assert!(cell.get("backend").is_some(), "cell missing backend");
    assert!(cell.get("locale").is_some(), "cell missing locale");
    assert!(cell.get("mode").is_some(), "cell missing mode");
    let metrics = cell["metrics"].as_object().expect("cell metrics object");
    for key in ["precision", "recall", "f1", "per_class"] {
        assert!(metrics.contains_key(key), "cell metrics missing {key}");
    }
    let populated_direct = cell["mode"] == "direct_detector";
    if populated_direct {
        for key in ["precision", "recall", "f1"] {
            assert_nullable_unit_float(&metrics[key], key);
        }
        assert!(
            metrics["per_class"].as_object().is_some(),
            "per_class must be an object"
        );
    } else {
        assert!(metrics["precision"].is_null());
        assert!(metrics["recall"].is_null());
        assert!(metrics["f1"].is_null());
        assert!(
            metrics["per_class"]
                .as_object()
                .is_some_and(serde_json::Map::is_empty),
            "per_class must be an empty object"
        );
    }

    if mode == "observer_residual" {
        for key in [
            "observer_residual_recall",
            "agreement_with_rule_floor",
            "expansion_fraction",
            "contradiction_fraction",
            "novel_tp_over_rule_floor",
        ] {
            assert!(
                metrics.get(key).is_some_and(Value::is_null),
                "observer_residual metrics missing null {key}"
            );
        }
    }
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
