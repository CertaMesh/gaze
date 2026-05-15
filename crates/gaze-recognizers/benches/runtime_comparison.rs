use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use gaze_recognizers::safety_net::kiji_distilbert::{
    KijiDistilbertBackend, OrtKijiBackend, OrtKijiConfig, KIJI_DISTILBERT_BUNDLE_SHA256,
    KIJI_DISTILBERT_HF_COMMIT, KIJI_DISTILBERT_HF_REPO, KIJI_DISTILBERT_SHA256SUMS,
};
use serde::Serialize;

const WARM_ITERATIONS: usize = 100;
const FIXTURE_LIMIT: usize = 12;

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u32,
    benchmark: &'static str,
    host: Host,
    pins: Pins,
    warm_iterations: usize,
    fixture_count: usize,
    runtimes: Vec<RuntimeReport>,
}

#[derive(Debug, Serialize)]
struct Host {
    os: String,
    arch: String,
    cpu: String,
    memory: String,
    rustc: String,
}

#[derive(Debug, Serialize)]
struct Pins {
    source_repo: &'static str,
    source_commit: &'static str,
    bundle_sha256: &'static str,
    sha256sums: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RuntimeReport {
    Measured {
        runtime: &'static str,
        cold_start_ms: f64,
        warm_p50_ms: f64,
        warm_p95_ms: f64,
    },
    Unavailable {
        runtime: &'static str,
        reason: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct SpanKey {
    start: usize,
    end: usize,
    label: String,
}

fn main() {
    let model_dir = env::var_os("GAZE_KIJI_DISTILBERT_MODEL_DIR")
        .map(PathBuf::from)
        .expect("GAZE_KIJI_DISTILBERT_MODEL_DIR is required");
    let fixtures = load_fixtures();
    assert!(
        !fixtures.is_empty(),
        "runtime comparison requires at least one fixture"
    );

    let (ort_report, baseline) = measure(
        "ort",
        || {
            OrtKijiBackend::new(OrtKijiConfig::new(&model_dir))
                .map(|backend| Box::new(backend) as Box<dyn KijiDistilbertBackend>)
        },
        &fixtures,
        None,
    );
    let Some(baseline) = baseline else {
        let report = Report {
            schema_version: 1,
            benchmark: "kiji-runtime-comparison",
            host: host(),
            pins: Pins {
                source_repo: KIJI_DISTILBERT_HF_REPO,
                source_commit: KIJI_DISTILBERT_HF_COMMIT,
                bundle_sha256: KIJI_DISTILBERT_BUNDLE_SHA256,
                sha256sums: KIJI_DISTILBERT_SHA256SUMS,
            },
            warm_iterations: WARM_ITERATIONS,
            fixture_count: fixtures.len(),
            runtimes: vec![ort_report],
        };
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        panic!("BLOCKED: ort baseline must load and infer before comparing runtimes");
    };

    let mut runtimes = vec![ort_report];
    runtimes.push(measure_tract(&model_dir, &fixtures, Some(&baseline)).0);
    runtimes.push(measure_candle(&model_dir, &fixtures, Some(&baseline)).0);

    let report = Report {
        schema_version: 1,
        benchmark: "kiji-runtime-comparison",
        host: host(),
        pins: Pins {
            source_repo: KIJI_DISTILBERT_HF_REPO,
            source_commit: KIJI_DISTILBERT_HF_COMMIT,
            bundle_sha256: KIJI_DISTILBERT_BUNDLE_SHA256,
            sha256sums: KIJI_DISTILBERT_SHA256SUMS,
        },
        warm_iterations: WARM_ITERATIONS,
        fixture_count: fixtures.len(),
        runtimes,
    };
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

fn measure<F>(
    runtime: &'static str,
    load: F,
    fixtures: &[String],
    baseline: Option<&[Vec<SpanKey>]>,
) -> (RuntimeReport, Option<Vec<Vec<SpanKey>>>)
where
    F: FnOnce() -> Result<Box<dyn KijiDistilbertBackend>, gaze_types::SafetyNetError>,
{
    let cold_start = Instant::now();
    let backend = match load() {
        Ok(backend) => backend,
        Err(error) => {
            return (
                RuntimeReport::Unavailable {
                    runtime,
                    reason: error.to_string(),
                },
                None,
            );
        }
    };
    let cold_start_ms = cold_start.elapsed().as_secs_f64() * 1000.0;
    let spans = match infer_all(&*backend, fixtures) {
        Ok(spans) => spans,
        Err(error) => {
            return (
                RuntimeReport::Unavailable {
                    runtime,
                    reason: error.to_string(),
                },
                None,
            );
        }
    };
    if let Some(expected) = baseline {
        assert_eq!(
            expected, spans,
            "BLOCKED: {runtime} recall/span set drifted from ort baseline"
        );
    }

    let mut samples = Vec::with_capacity(WARM_ITERATIONS);
    for index in 0..WARM_ITERATIONS {
        let fixture = &fixtures[index % fixtures.len()];
        let start = Instant::now();
        backend.infer(fixture).expect("warm inference must succeed");
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    let p50 = percentile(&samples, 0.50);
    let p95 = percentile(&samples, 0.95);

    (
        RuntimeReport::Measured {
            runtime,
            cold_start_ms,
            warm_p50_ms: p50,
            warm_p95_ms: p95,
        },
        Some(spans),
    )
}

fn infer_all(
    backend: &dyn KijiDistilbertBackend,
    fixtures: &[String],
) -> Result<Vec<Vec<SpanKey>>, gaze_types::SafetyNetError> {
    fixtures
        .iter()
        .map(|fixture| {
            let mut spans = backend
                .infer(fixture)?
                .into_iter()
                .map(|span| SpanKey {
                    start: span.start,
                    end: span.end,
                    label: span.label,
                })
                .collect::<Vec<_>>();
            spans.sort_by(|left, right| {
                (left.start, left.end, left.label.as_str()).cmp(&(
                    right.start,
                    right.end,
                    right.label.as_str(),
                ))
            });
            Ok(spans)
        })
        .collect()
}

#[cfg(feature = "runtime-tract")]
fn measure_tract(
    model_dir: &Path,
    fixtures: &[String],
    baseline: Option<&[Vec<SpanKey>]>,
) -> (RuntimeReport, Option<Vec<Vec<SpanKey>>>) {
    use gaze_recognizers::safety_net::kiji_distilbert::{TractKijiBackend, TractKijiConfig};
    measure(
        "tract",
        || {
            TractKijiBackend::new(TractKijiConfig::new(model_dir))
                .map(|backend| Box::new(backend) as Box<dyn KijiDistilbertBackend>)
        },
        fixtures,
        baseline,
    )
}

#[cfg(not(feature = "runtime-tract"))]
fn measure_tract(
    _model_dir: &Path,
    _fixtures: &[String],
    _baseline: Option<&[Vec<SpanKey>]>,
) -> (RuntimeReport, Option<Vec<Vec<SpanKey>>>) {
    (
        RuntimeReport::Unavailable {
            runtime: "tract",
            reason: "compile with gaze-recognizers feature runtime-tract".to_string(),
        },
        None,
    )
}

#[cfg(feature = "runtime-candle")]
fn measure_candle(
    model_dir: &Path,
    fixtures: &[String],
    baseline: Option<&[Vec<SpanKey>]>,
) -> (RuntimeReport, Option<Vec<Vec<SpanKey>>>) {
    use gaze_recognizers::safety_net::kiji_distilbert::{CandleKijiBackend, CandleKijiConfig};
    measure(
        "candle",
        || {
            CandleKijiBackend::new(CandleKijiConfig::new(model_dir))
                .map(|backend| Box::new(backend) as Box<dyn KijiDistilbertBackend>)
        },
        fixtures,
        baseline,
    )
}

#[cfg(not(feature = "runtime-candle"))]
fn measure_candle(
    _model_dir: &Path,
    _fixtures: &[String],
    _baseline: Option<&[Vec<SpanKey>]>,
) -> (RuntimeReport, Option<Vec<Vec<SpanKey>>>) {
    (
        RuntimeReport::Unavailable {
            runtime: "candle",
            reason: "compile with gaze-recognizers feature runtime-candle".to_string(),
        },
        None,
    )
}

fn load_fixtures() -> Vec<String> {
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/coverage-loop/corpus");
    let mut paths = fs::read_dir(corpus_dir)
        .expect("coverage-loop corpus directory")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .take(FIXTURE_LIMIT)
        .map(|path| fs::read_to_string(path).expect("fixture text"))
        .collect()
}

fn percentile(samples: &[f64], percentile: f64) -> f64 {
    let index = ((samples.len() - 1) as f64 * percentile).ceil() as usize;
    samples[index]
}

fn host() -> Host {
    Host {
        os: command("uname", &["-sr"]).unwrap_or_else(|| env::consts::OS.to_string()),
        arch: command("uname", &["-m"]).unwrap_or_else(|| env::consts::ARCH.to_string()),
        cpu: command("sysctl", &["-n", "machdep.cpu.brand_string"])
            .or_else(|| command("lscpu", &[]))
            .unwrap_or_else(|| "unknown".to_string()),
        memory: command("sysctl", &["-n", "hw.memsize"])
            .map(|bytes| format!("{bytes} bytes"))
            .or_else(|| command("free", &["-h"]))
            .unwrap_or_else(|| "unknown".to_string()),
        rustc: command("rustc", &["--version"]).unwrap_or_else(|| "unknown".to_string()),
    }
}

fn command(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    })
}
