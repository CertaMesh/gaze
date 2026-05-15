use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::time::Instant;

use gaze::{
    CleanDocument, LeakKind, LocaleTag, Pipeline, RawDocument, SafetyNetFallback, SafetyNetMode,
    SafetyNetPolicy, Scope, Session,
};
use gaze_assembly::CorePipelineConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchConfig {
    RuleFloorCore,
    RuleFloorExtended,
    Pass3Kiji,
    Pass3Opf,
    Pass3LocaleAware,
}

#[derive(Debug, Deserialize)]
struct Request {
    fixture_id: String,
    locale_chain: Vec<String>,
    text: String,
}

#[derive(Debug, Serialize)]
struct ManifestSpan {
    raw_start: usize,
    raw_end: usize,
    clean_start: usize,
    clean_end: usize,
    class: String,
}

#[derive(Debug, Serialize)]
struct LeakSuspectSpan {
    clean_start: usize,
    clean_end: usize,
    class: String,
    safety_net_id: String,
    kind: String,
}

#[derive(Debug, Serialize)]
struct Timing {
    total_ms: f64,
    pass1_ms: f64,
    pass2_ms: Option<f64>,
    pass3_ms: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_config()?;
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut full_pipelines = HashMap::<String, Pipeline>::new();
    let mut floor_pipelines = HashMap::<String, Pipeline>::new();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line)?;
        let locale_chain = request
            .locale_chain
            .iter()
            .map(|locale| LocaleTag::parse(locale))
            .collect::<Result<Vec<_>, _>>()?;
        let cache_key = request.locale_chain.join(",");
        if !full_pipelines.contains_key(&cache_key) {
            full_pipelines.insert(cache_key.clone(), build_pipeline(config, &locale_chain)?);
        }
        if !floor_pipelines.contains_key(&cache_key) {
            floor_pipelines.insert(
                cache_key.clone(),
                build_pipeline(floor_config(config), &locale_chain)?,
            );
        }
        let full = full_pipelines
            .get(&cache_key)
            .expect("full pipeline cached");
        let floor = floor_pipelines
            .get(&cache_key)
            .expect("floor pipeline cached");
        let session = Session::new(Scope::Ephemeral)?;
        let full_start = Instant::now();
        let (clean_doc, manifest, report) = full.clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(request.text.clone()),
            &locale_chain,
            &Default::default(),
            SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
        )?;
        let total_ms = full_start.elapsed().as_secs_f64() * 1000.0;

        let floor_start = Instant::now();
        let _ = floor.clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(request.text),
            &locale_chain,
            &Default::default(),
            SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
        )?;
        let pass1_ms = floor_start.elapsed().as_secs_f64() * 1000.0;

        let CleanDocument::Text(clean_text) = clean_doc else {
            return Err("expected text clean document".into());
        };
        let manifest_spans = manifest
            .into_iter()
            .map(|span| ManifestSpan {
                raw_start: span.raw_span.start,
                raw_end: span.raw_span.end,
                clean_start: span.clean_span.start,
                clean_end: span.clean_span.end,
                class: span.class.to_canonical_str(),
            })
            .collect();
        let leak_suspects = report
            .suspects
            .into_iter()
            .map(|suspect| LeakSuspectSpan {
                clean_start: suspect.span.start,
                clean_end: suspect.span.end,
                class: suspect.class.to_canonical_str(),
                safety_net_id: suspect.safety_net_id,
                kind: leak_kind_name(&suspect.kind).to_string(),
            })
            .collect::<Vec<_>>();
        serde_json::to_writer(
            &mut stdout,
            &Response {
                fixture_id: request.fixture_id,
                clean_text,
                manifest_spans,
                leak_suspects,
                timing: Timing {
                    total_ms,
                    pass1_ms,
                    pass2_ms: None,
                    pass3_ms: (total_ms - pass1_ms).max(0.0),
                },
            },
        )?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct Response {
    fixture_id: String,
    clean_text: String,
    manifest_spans: Vec<ManifestSpan>,
    leak_suspects: Vec<LeakSuspectSpan>,
    timing: Timing,
}

fn parse_config() -> Result<BenchConfig, Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut config = BenchConfig::RuleFloorExtended;
    while let Some(arg) = args.next() {
        if arg == "--config" {
            let value = args.next().ok_or("--config requires a value")?;
            config = match value.as_str() {
                "rule-floor-core" => BenchConfig::RuleFloorCore,
                "rule-floor-extended" => BenchConfig::RuleFloorExtended,
                "pass3-kiji" => BenchConfig::Pass3Kiji,
                "pass3-opf" => BenchConfig::Pass3Opf,
                "pass3-locale-aware" => BenchConfig::Pass3LocaleAware,
                _ => return Err(format!("unknown --config {value}").into()),
            };
        }
    }
    Ok(config)
}

fn floor_config(config: BenchConfig) -> BenchConfig {
    match config {
        BenchConfig::RuleFloorCore => BenchConfig::RuleFloorCore,
        BenchConfig::RuleFloorExtended
        | BenchConfig::Pass3Kiji
        | BenchConfig::Pass3Opf
        | BenchConfig::Pass3LocaleAware => BenchConfig::RuleFloorExtended,
    }
}

fn build_pipeline(
    config: BenchConfig,
    locale_chain: &[LocaleTag],
) -> Result<Pipeline, Box<dyn std::error::Error>> {
    let mut builder = CorePipelineConfig::new().with_locale(locale_chain);
    if config != BenchConfig::RuleFloorCore {
        builder = builder.with_bundled_rulepack("core-extended");
    }
    let mut pipeline = builder.build()?.into_pipeline();
    match config {
        BenchConfig::RuleFloorCore | BenchConfig::RuleFloorExtended => {}
        BenchConfig::Pass3Kiji => {
            pipeline = register_kiji(pipeline)?;
        }
        BenchConfig::Pass3Opf => {
            pipeline = register_opf(pipeline)?;
        }
        BenchConfig::Pass3LocaleAware => {
            return Err(
                "LocaleAwareModelRegistry has no Pipeline SafetyNet adapter in this stack".into(),
            );
        }
    }
    Ok(pipeline)
}

#[cfg(feature = "safety-net-kiji")]
fn register_kiji(pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    use gaze_recognizers::safety_net::kiji_distilbert::KijiDistilbertSafetyNet;

    Ok(pipeline.with_safety_net(KijiDistilbertSafetyNet::from_env()?))
}

#[cfg(not(feature = "safety-net-kiji"))]
fn register_kiji(_pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    Err("compile with gaze-recognizers feature safety-net-kiji".into())
}

#[cfg(feature = "safety-net-openai")]
fn register_opf(pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    use gaze_recognizers::safety_net::openai_filter::OpenAiFilterSafetyNet;

    Ok(pipeline.with_safety_net(OpenAiFilterSafetyNet::from_env()?))
}

#[cfg(not(feature = "safety-net-openai"))]
fn register_opf(_pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    Err("compile with gaze-recognizers feature safety-net-openai".into())
}

fn leak_kind_name(kind: &LeakKind) -> &'static str {
    match kind {
        LeakKind::Uncovered => "uncovered",
        LeakKind::PartialBleed { .. } => "partial_bleed",
        LeakKind::ClassMismatch { .. } => "class_mismatch",
        _ => "unknown",
    }
}
