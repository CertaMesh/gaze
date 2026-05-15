use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gaze::{
    Action, ClassRule, DefaultRule, Detection, Detector, DictionaryBundle, LeakSuspect, Pipeline,
    PipelineOptimizationConfig, RawDocument, SafetyNet, SafetyNetContext, SafetyNetError,
    SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};

#[derive(Clone)]
struct EmailDetector;

impl Detector for EmailDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .find("alice@example.invalid")
            .map(|start| {
                Detection::new(
                    start..start + "alice@example.invalid".len(),
                    gaze::PiiClass::Email,
                    "bench.email",
                )
            })
            .into_iter()
            .collect()
    }
}

struct SlowSafetyNet {
    calls: Arc<AtomicUsize>,
}

struct SlowNameDetector {
    bytes: Arc<AtomicUsize>,
}

impl Detector for SlowNameDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        self.bytes.fetch_add(input.len(), Ordering::SeqCst);
        thread::sleep(Duration::from_micros(
            (input.len() as u64).saturating_mul(10),
        ));
        input
            .find("Dr. Schmidt")
            .map(|start| {
                Detection::new(
                    start..start + "Dr. Schmidt".len(),
                    gaze::PiiClass::Name,
                    "bench.name",
                )
            })
            .into_iter()
            .collect()
    }
}

impl SafetyNet for SlowSafetyNet {
    fn id(&self) -> &str {
        "bench-slow"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }

    fn check(
        &self,
        _clean_text: &str,
        _context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        thread::sleep(Duration::from_micros(100));
        Ok(Vec::new())
    }
}

fn main() {
    let corpus = corpus();
    let baseline = run_config("baseline", PipelineOptimizationConfig::default(), &corpus);
    let skip_class = run_config(
        "skip_class_gating",
        PipelineOptimizationConfig::new().with_skip_class_gating(true),
        &corpus,
    );
    let capitals = run_config(
        "capitals_heuristic_gate",
        PipelineOptimizationConfig::new().with_capitals_heuristic_gate(true),
        &corpus,
    );
    let combined = run_config(
        "combined_skip_and_capitals",
        PipelineOptimizationConfig::new()
            .with_skip_class_gating(true)
            .with_capitals_heuristic_gate(true),
        &corpus,
    );
    println!(
        "{{\"baseline\":{},\"configs\":[{},{},{}],\"prefix_cache\":{}}}",
        baseline.to_json(None),
        skip_class.to_json(Some(&baseline)),
        capitals.to_json(Some(&baseline)),
        combined.to_json(Some(&baseline)),
        run_prefix_cache_bench(),
    );
}

fn corpus() -> Vec<String> {
    let mut inputs = Vec::new();
    for index in 0..100 {
        inputs.push(format!("Reach alice@example.invalid about case {index}"));
        inputs.push(format!("lowercase status update number {index}"));
        inputs.push(format!("20260515-0000-{index:04}-999999"));
    }
    inputs
}

fn run_config(
    name: &'static str,
    optimization_config: PipelineOptimizationConfig,
    corpus: &[String],
) -> BenchResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::builder()
        .detector(EmailDetector)
        .rule(ClassRule::new(gaze::PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(SlowSafetyNet {
            calls: Arc::clone(&calls),
        })
        .pipeline_optimizations(optimization_config)
        .build()
        .expect("pipeline");
    let dictionaries = DictionaryBundle::default();
    let start = Instant::now();
    for input in corpus {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let (_, _, report) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(input.clone()),
                &[gaze::LocaleTag::Global],
                &dictionaries,
                SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
            )
            .expect("clean");
        assert_eq!(
            report.stats.suspect_count, 0,
            "recall-preserving empty report"
        );
    }
    BenchResult {
        name,
        calls: calls.load(Ordering::SeqCst),
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    }
}

fn run_prefix_cache_bench() -> String {
    let baseline = run_prefix_cache_config("baseline", false);
    let cached = run_prefix_cache_config("prefix_cache", true);
    let byte_reduction = 1.0 - (cached.bytes_processed as f64 / baseline.bytes_processed as f64);
    let latency_reduction = 1.0 - (cached.elapsed_ms / baseline.elapsed_ms);
    format!(
        "{{\"baseline\":{},\"cached\":{},\"byte_reduction\":{:.4},\"latency_reduction\":{:.4}}}",
        baseline.to_json(),
        cached.to_json(),
        byte_reduction,
        latency_reduction,
    )
}

fn run_prefix_cache_config(name: &'static str, enabled: bool) -> PrefixBenchResult {
    let bytes = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::builder()
        .detector(SlowNameDetector {
            bytes: Arc::clone(&bytes),
        })
        .rule(ClassRule::new(gaze::PiiClass::Name, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .pipeline_optimizations(PipelineOptimizationConfig::new().with_prefix_cache(enabled))
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let start = Instant::now();
    for index in 0..100 {
        pipeline
            .redact(
                &session,
                RawDocument::Text(format!("Dr. Schmidt reports {index}")),
            )
            .expect("redact");
    }
    PrefixBenchResult {
        name,
        bytes_processed: bytes.load(Ordering::SeqCst),
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    }
}

struct BenchResult {
    name: &'static str,
    calls: usize,
    elapsed_ms: f64,
}

impl BenchResult {
    fn to_json(&self, baseline: Option<&BenchResult>) -> String {
        let reduction = baseline
            .map(|baseline| {
                if baseline.calls == 0 {
                    0.0
                } else {
                    1.0 - (self.calls as f64 / baseline.calls as f64)
                }
            })
            .unwrap_or(0.0);
        format!(
            "{{\"name\":\"{}\",\"safety_net_calls\":{},\"elapsed_ms\":{:.3},\"call_reduction\":{:.4}}}",
            self.name, self.calls, self.elapsed_ms, reduction
        )
    }
}

struct PrefixBenchResult {
    name: &'static str,
    bytes_processed: usize,
    elapsed_ms: f64,
}

impl PrefixBenchResult {
    fn to_json(&self) -> String {
        format!(
            "{{\"name\":\"{}\",\"detector_bytes_processed\":{},\"elapsed_ms\":{:.3}}}",
            self.name, self.bytes_processed, self.elapsed_ms
        )
    }
}
