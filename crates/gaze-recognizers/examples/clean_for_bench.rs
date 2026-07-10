use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use gaze::{
    Action, CleanDocument, Context, EmittedTokenSpan, GazeLocalProtectionTraceItem, LeakKind,
    LeakReportStats, LocaleChain, LocaleTag, NerPolicy, PiiClass, Pipeline, RawDocument, RuleSpec,
    Rulepack, RulepackSource, SafetyNetError, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy,
    Scope, Session,
};
use gaze_recognizers::embedded;
use serde::{Deserialize, Serialize};

const BENCHMARK_ACTIVE_LOCALES: &[LocaleTag] = &[
    LocaleTag::EnUs,
    LocaleTag::DeDe,
    LocaleTag::DeAt,
    LocaleTag::DeCh,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchConfig {
    RuleFloorCore,
    RuleFloorExtended,
    Pass2Ner,
    FullStackKijiResolve,
    FullStackOpfResolve,
    Pass3Kiji,
    Pass3Opf,
    Pass3LocaleAware,
}

impl BenchConfig {
    fn name(self) -> &'static str {
        match self {
            Self::RuleFloorCore => "rule-floor-core",
            Self::RuleFloorExtended => "rule-floor-extended",
            Self::Pass2Ner => "pass2-ner",
            Self::FullStackKijiResolve => "full-stack-kiji-resolve",
            Self::FullStackOpfResolve => "full-stack-opf-resolve",
            Self::Pass3Kiji => "pass3-kiji",
            Self::Pass3Opf => "pass3-opf",
            Self::Pass3LocaleAware => "pass3-locale-aware",
        }
    }

    fn uses_ner(self) -> bool {
        matches!(
            self,
            Self::Pass2Ner | Self::FullStackKijiResolve | Self::FullStackOpfResolve
        )
    }

    fn uses_extended_rule_floor(self) -> bool {
        self != Self::RuleFloorCore
    }
}

#[derive(Debug, Clone)]
struct NerSettings {
    model_dir: PathBuf,
    locale: Option<String>,
    threshold: f32,
}

#[derive(Debug, thiserror::Error)]
enum BenchmarkBuildError {
    #[error("bundled recognizer rulepack '{id}' is unavailable")]
    MissingBundledRulepack { id: String },
    #[error("GAZE_NER_MODEL_DIR is required for this benchmark cell")]
    MissingNerModelDir,
    #[error("invalid GAZE_NER_THRESHOLD '{value}': {source}")]
    InvalidNerThreshold {
        value: String,
        #[source]
        source: std::num::ParseFloatError,
    },
    #[error("GAZE_NER_THRESHOLD must contain valid Unicode")]
    NonUnicodeNerThreshold,
    #[error(transparent)]
    Assembly(#[from] gaze_assembly::BuildError),
    #[error("failed to register safety net for benchmark cell '{cell}': {source}")]
    SafetyNetRegistration {
        cell: &'static str,
        #[source]
        source: Box<dyn std::error::Error>,
    },
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
struct FinalProtectionTraceItem {
    raw_start: usize,
    raw_end: usize,
    class: String,
    action: String,
    provenance: FinalProtectionTraceProvenance,
}

#[derive(Debug, Serialize)]
struct FinalProtectionTraceProvenance {
    stage: String,
    decision: String,
    source_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LeakSuspectSpan {
    clean_start: usize,
    clean_end: usize,
    action_start: usize,
    action_end: usize,
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
    restore_ms: f64,
    post_policy_scan_ms: f64,
}

#[derive(Debug, Serialize)]
struct SafetyNetStats {
    suspect_count: usize,
    uncovered_count: usize,
    partial_bleed_count: usize,
    class_mismatch_count: usize,
    locale_skipped_count: usize,
}

#[derive(Debug, Serialize)]
struct RestoreResult {
    exact: bool,
    decision: String,
    unknown_token_count: u64,
    manifest_bypass_count: u64,
    fresh_pii_detected_count: u64,
    phase_execution_mask: u32,
}

#[derive(Debug, Serialize)]
struct ManifestIntegrity {
    spans: usize,
    invalid_clean_bounds: usize,
    invalid_raw_bounds: usize,
    overlapping_clean_spans: usize,
    non_monotonic_raw_spans: usize,
    token_restore_failures: usize,
    raw_value_mismatches: usize,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum Outcome {
    Success(Response),
    PipelineError {
        fixture_id: String,
        stage: &'static str,
        code: &'static str,
        total_ms: f64,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_config()?;
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let full = build_pipeline(config)?;
    let floor = build_pipeline(floor_config(config))?;
    let pre_safety = if matches!(
        config,
        BenchConfig::FullStackKijiResolve | BenchConfig::FullStackOpfResolve
    ) {
        Some(build_pipeline(BenchConfig::Pass2Ner)?)
    } else {
        None
    };

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line)?;
        match handle_request(config, &full, &floor, pre_safety.as_ref(), request)? {
            Outcome::Success(response) => {
                serde_json::to_writer(&mut stdout, &response)?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
            }
            Outcome::PipelineError {
                fixture_id,
                stage,
                code,
                total_ms,
            } => {
                write_pipeline_error(&mut stdout, &fixture_id, stage, code, total_ms)?;
            }
        }
    }

    Ok(())
}

fn handle_request(
    config: BenchConfig,
    full: &Pipeline,
    floor: &Pipeline,
    pre_safety: Option<&Pipeline>,
    request: Request,
) -> Result<Outcome, Box<dyn std::error::Error>> {
    let locale_chain = request
        .locale_chain
        .iter()
        .map(|locale| LocaleTag::parse(locale))
        .collect::<Result<Vec<_>, _>>()?;
    let raw_text = request.text;
    let session = Session::new(Scope::Ephemeral)?;
    let full_start = Instant::now();
    let clean_result = full.clean_text_with_safety_net_policy_detect_context_and_protection_trace(
        &session,
        &raw_text,
        &locale_chain,
        &Default::default(),
        safety_net_policy(config),
    );
    let total_ms = full_start.elapsed().as_secs_f64() * 1000.0;
    let (clean_doc, manifest, report, final_protection_trace) = match clean_result {
        Ok(result) => result,
        Err(error) => {
            return Ok(Outcome::PipelineError {
                fixture_id: request.fixture_id,
                stage: "clean",
                code: pipeline_error_code(&error),
                total_ms,
            });
        }
    };

    let CleanDocument::Text(clean_text) = clean_doc else {
        return Err("expected text clean document".into());
    };

    let integrity = manifest_integrity(&session, &raw_text, &clean_text, &manifest);
    let restore_start = Instant::now();
    let (restored, restore_telemetry) = full.restore_with_telemetry(&session, &clean_text)?;
    let restore_ms = restore_start.elapsed().as_secs_f64() * 1000.0;
    let restore = RestoreResult {
        exact: restored.text == raw_text,
        decision: restore_telemetry.restore_decision_str().to_string(),
        unknown_token_count: restore_telemetry.unknown_token_count,
        manifest_bypass_count: restore_telemetry.manifest_bypass_count,
        fresh_pii_detected_count: restore_telemetry.fresh_pii_detected_count,
        phase_execution_mask: restore_telemetry.phase_execution_mask,
    };

    let post_policy_scan_start = Instant::now();
    let post_policy_safety_net_stats = if matches!(
        config,
        BenchConfig::FullStackKijiResolve | BenchConfig::FullStackOpfResolve
    ) {
        let post_policy = match full.scan_safety_nets(&session, &clean_text, &locale_chain) {
            Ok(result) => result,
            Err(error) => {
                return Ok(Outcome::PipelineError {
                    fixture_id: request.fixture_id,
                    stage: "post_policy_scan",
                    code: pipeline_error_code(&error),
                    total_ms,
                });
            }
        };
        Some(SafetyNetStats::from(&post_policy.report.stats))
    } else {
        None
    };
    let post_policy_scan_ms = post_policy_scan_start.elapsed().as_secs_f64() * 1000.0;

    let floor_session = Session::new(Scope::Ephemeral)?;
    let floor_start = Instant::now();
    let _ = floor.clean_with_safety_net_policy_detect_context(
        &floor_session,
        RawDocument::Text(raw_text.clone()),
        &locale_chain,
        &Default::default(),
        SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
    )?;
    let pass1_ms = floor_start.elapsed().as_secs_f64() * 1000.0;

    let (pre_safety_text_len, pre_safety_manifest_spans, pre_safety_ms) =
        if let Some(pre_safety_pipeline) = pre_safety {
            let pre_safety_session = Session::new(Scope::Ephemeral)?;
            let pre_safety_start = Instant::now();
            let (pre_safety_doc, pre_safety_manifest, _) = pre_safety_pipeline
                .clean_with_safety_net_policy_detect_context(
                    &pre_safety_session,
                    RawDocument::Text(raw_text.clone()),
                    &locale_chain,
                    &Default::default(),
                    SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
                )?;
            let elapsed = pre_safety_start.elapsed().as_secs_f64() * 1000.0;
            let CleanDocument::Text(pre_safety_text) = pre_safety_doc else {
                return Err("expected text pre-safety document".into());
            };
            (
                Some(pre_safety_text.len()),
                Some(serialize_manifest(pre_safety_manifest)),
                Some(elapsed),
            )
        } else {
            (None, None, None)
        };

    let manifest_spans = serialize_manifest(manifest);
    let final_protection_trace = serialize_final_protection_trace(final_protection_trace);
    let initial_safety_net_stats = SafetyNetStats::from(&report.stats);
    let strict_would_reject = report.stats.uncovered_count + report.stats.partial_bleed_count > 0;
    let leak_suspects = report
        .suspects
        .into_iter()
        .map(|suspect| {
            let action_span = match &suspect.kind {
                LeakKind::PartialBleed { uncovered } => uncovered.clone(),
                _ => suspect.span.clone(),
            };
            LeakSuspectSpan {
                clean_start: suspect.span.start,
                clean_end: suspect.span.end,
                action_start: action_span.start,
                action_end: action_span.end,
                class: suspect.class.to_canonical_str(),
                safety_net_id: suspect.safety_net_id,
                kind: leak_kind_name(&suspect.kind).to_string(),
            }
        })
        .collect::<Vec<_>>();
    Ok(Outcome::Success(Response {
        fixture_id: request.fixture_id,
        clean_text,
        manifest_spans,
        pre_safety_text_len,
        pre_safety_manifest_spans,
        leak_suspects,
        safety_net_mode: safety_net_mode_name(safety_net_policy(config).mode).to_string(),
        strict_would_reject,
        initial_safety_net_stats,
        post_policy_safety_net_stats,
        restore,
        manifest_integrity: integrity,
        timing: Timing {
            total_ms,
            pass1_ms,
            pass2_ms: if matches!(
                config,
                BenchConfig::FullStackKijiResolve | BenchConfig::FullStackOpfResolve
            ) {
                pre_safety_ms.map(|elapsed| (elapsed - pass1_ms).max(0.0))
            } else {
                (config == BenchConfig::Pass2Ner).then_some((total_ms - pass1_ms).max(0.0))
            },
            pass3_ms: match config {
                BenchConfig::RuleFloorCore
                | BenchConfig::RuleFloorExtended
                | BenchConfig::Pass2Ner => 0.0,
                BenchConfig::FullStackKijiResolve | BenchConfig::FullStackOpfResolve => {
                    pre_safety_ms
                        .map(|elapsed| (total_ms - elapsed).max(0.0))
                        .unwrap_or(0.0)
                }
                BenchConfig::Pass3Kiji | BenchConfig::Pass3Opf | BenchConfig::Pass3LocaleAware => {
                    (total_ms - pass1_ms).max(0.0)
                }
            },
            restore_ms,
            post_policy_scan_ms,
        },
        final_protection_trace,
    }))
}

#[derive(Debug, Serialize)]
struct Response {
    fixture_id: String,
    clean_text: String,
    manifest_spans: Vec<ManifestSpan>,
    pre_safety_text_len: Option<usize>,
    pre_safety_manifest_spans: Option<Vec<ManifestSpan>>,
    leak_suspects: Vec<LeakSuspectSpan>,
    safety_net_mode: String,
    strict_would_reject: bool,
    initial_safety_net_stats: SafetyNetStats,
    post_policy_safety_net_stats: Option<SafetyNetStats>,
    restore: RestoreResult,
    manifest_integrity: ManifestIntegrity,
    timing: Timing,
    final_protection_trace: Vec<FinalProtectionTraceItem>,
}

#[derive(Debug, Serialize)]
struct PipelineErrorResponse<'a> {
    fixture_id: &'a str,
    pipeline_error_stage: &'a str,
    pipeline_error_code: &'a str,
    timing: PipelineErrorTiming,
}

#[derive(Debug, Serialize)]
struct PipelineErrorTiming {
    total_ms: f64,
}

fn write_pipeline_error(
    stdout: &mut impl Write,
    fixture_id: &str,
    stage: &str,
    code: &str,
    total_ms: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(
        &mut *stdout,
        &PipelineErrorResponse {
            fixture_id,
            pipeline_error_stage: stage,
            pipeline_error_code: code,
            timing: PipelineErrorTiming { total_ms },
        },
    )?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

fn pipeline_error_code(error: &gaze::Error) -> &'static str {
    match error {
        gaze::Error::SafetyNet(SafetyNetError::InvalidOutput { .. }) => "safety_net_invalid_output",
        gaze::Error::SafetyNet(SafetyNetError::Runtime { .. }) => "safety_net_runtime",
        gaze::Error::SafetyNet(_) => "safety_net_error",
        _ => "pipeline_error",
    }
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
                "pass2-ner" => BenchConfig::Pass2Ner,
                "full-stack-kiji-resolve" => BenchConfig::FullStackKijiResolve,
                "full-stack-opf-resolve" => BenchConfig::FullStackOpfResolve,
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
        | BenchConfig::Pass2Ner
        | BenchConfig::FullStackKijiResolve
        | BenchConfig::FullStackOpfResolve
        | BenchConfig::Pass3Kiji
        | BenchConfig::Pass3Opf
        | BenchConfig::Pass3LocaleAware => BenchConfig::RuleFloorExtended,
    }
}

fn build_pipeline(config: BenchConfig) -> Result<Pipeline, BenchmarkBuildError> {
    let ner = if config.uses_ner() {
        Some(ner_settings_from_env()?)
    } else {
        None
    };
    let mut pipeline = assemble_rule_floor(config, ner)?;
    match config {
        BenchConfig::RuleFloorCore | BenchConfig::RuleFloorExtended | BenchConfig::Pass2Ner => {}
        BenchConfig::FullStackKijiResolve => {
            pipeline = register_kiji_ort(pipeline).map_err(|source| {
                BenchmarkBuildError::SafetyNetRegistration {
                    cell: config.name(),
                    source,
                }
            })?;
        }
        BenchConfig::FullStackOpfResolve => {
            pipeline = register_opf(pipeline).map_err(|source| {
                BenchmarkBuildError::SafetyNetRegistration {
                    cell: config.name(),
                    source,
                }
            })?;
        }
        BenchConfig::Pass3Kiji => {
            pipeline = register_kiji(pipeline).map_err(|source| {
                BenchmarkBuildError::SafetyNetRegistration {
                    cell: config.name(),
                    source,
                }
            })?;
        }
        BenchConfig::Pass3Opf => {
            pipeline = register_opf(pipeline).map_err(|source| {
                BenchmarkBuildError::SafetyNetRegistration {
                    cell: config.name(),
                    source,
                }
            })?;
        }
        BenchConfig::Pass3LocaleAware => {
            pipeline = register_locale_aware(pipeline).map_err(|source| {
                BenchmarkBuildError::SafetyNetRegistration {
                    cell: config.name(),
                    source,
                }
            })?;
        }
    }
    Ok(pipeline)
}

fn ner_settings_from_env() -> Result<NerSettings, BenchmarkBuildError> {
    let model_dir = std::env::var_os("GAZE_NER_MODEL_DIR")
        .map(PathBuf::from)
        .ok_or(BenchmarkBuildError::MissingNerModelDir)?;
    let threshold = match std::env::var("GAZE_NER_THRESHOLD") {
        Ok(value) => value
            .parse::<f32>()
            .map_err(|source| BenchmarkBuildError::InvalidNerThreshold { value, source })?,
        Err(std::env::VarError::NotPresent) => gaze::DEFAULT_NER_THRESHOLD,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(BenchmarkBuildError::NonUnicodeNerThreshold)
        }
    };
    if !(0.0..=1.0).contains(&threshold) {
        return Err(BenchmarkBuildError::Assembly(
            gaze_assembly::BuildError::Policy(gaze::PolicyError::NerThresholdOutOfRange {
                value: threshold,
            }),
        ));
    }

    Ok(NerSettings {
        model_dir,
        locale: std::env::var("GAZE_NER_LOCALE").ok(),
        threshold,
    })
}

fn assemble_rule_floor(
    config: BenchConfig,
    ner: Option<NerSettings>,
) -> Result<Pipeline, BenchmarkBuildError> {
    let bundle = if config.uses_extended_rule_floor() {
        "core-extended"
    } else {
        "core"
    };
    let rulepack = load_bundled_rulepack(bundle)?;
    let mut policy = benchmark_policy(&rulepack, config.uses_extended_rule_floor());
    let ner_threshold = if config.uses_ner() {
        let ner = ner.ok_or(BenchmarkBuildError::MissingNerModelDir)?;
        let mut ner_policy = NerPolicy::default();
        ner_policy.model_dir = Some(ner.model_dir);
        ner_policy.locale = ner.locale;
        ner_policy.threshold = ner.threshold;
        policy.ner = Some(ner_policy);
        Some(ner.threshold)
    } else {
        None
    };
    let active_locales = benchmark_locale_chain(&policy, &rulepack);

    Ok(gaze_assembly::build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        ner_threshold,
    )?)
}

fn load_bundled_rulepack(id: &str) -> Result<Rulepack, BenchmarkBuildError> {
    let contents = embedded(id)
        .ok_or_else(|| BenchmarkBuildError::MissingBundledRulepack { id: id.to_string() })?;
    Rulepack::load(RulepackSource::Embedded(contents))
        .map_err(gaze_assembly::BuildError::from)
        .map_err(BenchmarkBuildError::from)
}

fn benchmark_policy(rulepack: &Rulepack, auto_activate_locale_gated: bool) -> gaze::Policy {
    let mut seen = BTreeSet::<PiiClass>::new();
    let mut rules = Vec::new();
    for recognizer in rulepack
        .recognizers
        .iter()
        .filter(|recognizer| recognizer.enabled)
    {
        if seen.insert(recognizer.class.clone()) {
            rules.push(RuleSpec::Class {
                class: recognizer.class.clone(),
                action: Action::Tokenize,
            });
        }
        if let Some(family_class) = recognizer.collision.as_ref().and_then(|collision| {
            collision
                .mandatory_anchor
                .as_ref()
                .map(|_| PiiClass::Custom(format!("family:{}", collision.family)))
        }) {
            if seen.insert(family_class.clone()) {
                rules.push(RuleSpec::Class {
                    class: family_class,
                    action: Action::Tokenize,
                });
            }
        }
    }
    rules.push(RuleSpec::Default {
        action: Action::Tokenize,
    });

    let mut policy = gaze::Policy::default();
    policy.rules = rules;
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = auto_activate_locale_gated;
    policy
}

fn benchmark_locale_chain(policy: &gaze::Policy, rulepack: &Rulepack) -> LocaleChain {
    let mut defaults = rulepack.default_locales.clone();
    if policy.rulepacks.auto_activate_locale_gated {
        for locale in BENCHMARK_ACTIVE_LOCALES {
            if !defaults.iter().any(|existing| existing == locale) {
                defaults.push(locale.clone());
            }
        }
    }
    LocaleChain::merge_cli_policy_rulepack_default(None, policy.locale.as_deref(), Some(&defaults))
}

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

#[cfg(feature = "safety-net-kiji")]
fn register_kiji(pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    use gaze_recognizers::safety_net::kiji_distilbert::KijiDistilbertSafetyNet;

    Ok(pipeline.with_safety_net(KijiDistilbertSafetyNet::from_env()?))
}

#[cfg(feature = "safety-net-kiji")]
fn register_kiji_ort(pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    use gaze_recognizers::safety_net::kiji_distilbert::KijiDistilbertSafetyNet;

    Ok(pipeline.with_safety_net(KijiDistilbertSafetyNet::from_env_ort()?))
}

#[cfg(not(feature = "safety-net-kiji"))]
fn register_kiji_ort(_pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    Err("compile with gaze-recognizers feature safety-net-kiji".into())
}

#[cfg(not(feature = "safety-net-kiji"))]
fn register_kiji(_pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    Err("compile with gaze-recognizers feature safety-net-kiji".into())
}

#[cfg(feature = "safety-net-openai")]
fn register_opf(pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    use gaze_recognizers::safety_net::openai_filter::{
        OpenAiFilterSafetyNet, SubprocessOpenAiFilterConfig,
    };

    let command =
        std::env::var_os("GAZE_OPENAI_FILTER_OPF").ok_or("GAZE_OPENAI_FILTER_OPF is not set")?;
    let checkpoint = std::env::var_os("OPF_CHECKPOINT").ok_or("OPF_CHECKPOINT is not set")?;
    let config = SubprocessOpenAiFilterConfig::new(command)
        .with_checkpoint_path(checkpoint)
        .with_timeout(std::time::Duration::from_secs(30))
        .with_args([
            "--format",
            "json",
            "--output-mode",
            "typed",
            "--no-print-color-coded-text",
            "--device",
            "cpu",
        ])
        .with_checkpoint_bundle_sha256_verification(true);
    Ok(pipeline.with_safety_net(OpenAiFilterSafetyNet::new(config)))
}

#[cfg(not(feature = "safety-net-openai"))]
fn register_opf(_pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    Err("compile with gaze-recognizers feature safety-net-openai".into())
}

#[cfg(all(feature = "safety-net-kiji", feature = "safety-net-openai"))]
fn register_locale_aware(pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    use gaze_recognizers::safety_net::kiji_distilbert::{
        KijiDistilbertSafetyNet, SubprocessKijiConfig,
    };
    use gaze_recognizers::safety_net::openai_filter::{
        OpenAiFilterSafetyNet, SubprocessOpenAiFilterConfig,
    };

    let kiji_command = std::env::var_os("GAZE_KIJI_DISTILBERT_COMMAND")
        .ok_or("GAZE_KIJI_DISTILBERT_COMMAND is not set")?;
    let kiji_model_dir = std::env::var_os("GAZE_KIJI_DISTILBERT_MODEL_DIR")
        .ok_or("GAZE_KIJI_DISTILBERT_MODEL_DIR is not set")?;
    let opf_command =
        std::env::var_os("GAZE_OPENAI_FILTER_OPF").ok_or("GAZE_OPENAI_FILTER_OPF is not set")?;
    let opf_checkpoint = std::env::var_os("OPF_CHECKPOINT").ok_or("OPF_CHECKPOINT is not set")?;

    Ok(pipeline
        .with_safety_net(
            OpenAiFilterSafetyNet::new(
                SubprocessOpenAiFilterConfig::new(opf_command)
                    .with_checkpoint_path(opf_checkpoint)
                    .with_timeout(std::time::Duration::from_secs(30))
                    .with_args([
                        "--format",
                        "json",
                        "--output-mode",
                        "typed",
                        "--no-print-color-coded-text",
                        "--device",
                        "cpu",
                    ]),
            )
            .with_locales(vec![LocaleTag::Global, LocaleTag::EnUs]),
        )
        .with_safety_net(
            KijiDistilbertSafetyNet::new(
                SubprocessKijiConfig::new(kiji_command)
                    .with_model_dir(kiji_model_dir)
                    .with_timeout(std::time::Duration::from_secs(30)),
            )
            .with_locales(vec![LocaleTag::DeDe]),
        ))
}

#[cfg(not(all(feature = "safety-net-kiji", feature = "safety-net-openai")))]
fn register_locale_aware(_pipeline: Pipeline) -> Result<Pipeline, Box<dyn std::error::Error>> {
    Err("compile with gaze-recognizers features safety-net-kiji,safety-net-openai".into())
}

fn leak_kind_name(kind: &LeakKind) -> &'static str {
    match kind {
        LeakKind::Uncovered => "uncovered",
        LeakKind::PartialBleed { .. } => "partial_bleed",
        LeakKind::ClassMismatch { .. } => "class_mismatch",
        _ => "unknown",
    }
}

fn safety_net_policy(config: BenchConfig) -> SafetyNetPolicy {
    if matches!(
        config,
        BenchConfig::FullStackKijiResolve | BenchConfig::FullStackOpfResolve
    ) {
        SafetyNetPolicy::default()
    } else {
        SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact)
    }
}

fn safety_net_mode_name(mode: SafetyNetMode) -> &'static str {
    match mode {
        SafetyNetMode::Strict => "strict",
        SafetyNetMode::Tolerant => "tolerant",
        SafetyNetMode::Redact => "redact",
        SafetyNetMode::Resolve => "resolve",
        _ => "unknown",
    }
}

impl From<&LeakReportStats> for SafetyNetStats {
    fn from(stats: &LeakReportStats) -> Self {
        Self {
            suspect_count: stats.suspect_count,
            uncovered_count: stats.uncovered_count,
            partial_bleed_count: stats.partial_bleed_count,
            class_mismatch_count: stats.class_mismatch_count,
            locale_skipped_count: stats.locale_skipped_count,
        }
    }
}

fn serialize_manifest(manifest: Vec<EmittedTokenSpan>) -> Vec<ManifestSpan> {
    manifest
        .into_iter()
        .map(|span| ManifestSpan {
            raw_start: span.raw_span.start,
            raw_end: span.raw_span.end,
            clean_start: span.clean_span.start,
            clean_end: span.clean_span.end,
            class: span.class.to_canonical_str(),
        })
        .collect()
}

#[cfg(test)]
fn manifest_signature(
    manifest: &[EmittedTokenSpan],
) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>, String)> {
    manifest
        .iter()
        .map(|span| {
            (
                span.raw_span.clone(),
                span.clean_span.clone(),
                span.class.to_canonical_str(),
            )
        })
        .collect()
}

fn serialize_final_protection_trace(
    trace: Vec<GazeLocalProtectionTraceItem>,
) -> Vec<FinalProtectionTraceItem> {
    trace
        .into_iter()
        .map(|item| FinalProtectionTraceItem {
            raw_start: item.raw_start(),
            raw_end: item.raw_end(),
            class: item.class().to_canonical_str(),
            action: item.action().to_string(),
            provenance: FinalProtectionTraceProvenance {
                stage: item.stage().to_string(),
                decision: item.decision().to_string(),
                source_ids: item.source_ids().to_vec(),
            },
        })
        .collect()
}

fn manifest_integrity(
    session: &Session,
    raw_text: &str,
    clean_text: &str,
    manifest: &[EmittedTokenSpan],
) -> ManifestIntegrity {
    let mut result = ManifestIntegrity {
        spans: manifest.len(),
        invalid_clean_bounds: 0,
        invalid_raw_bounds: 0,
        overlapping_clean_spans: 0,
        non_monotonic_raw_spans: 0,
        token_restore_failures: 0,
        raw_value_mismatches: 0,
    };
    let mut previous_clean_end = 0;
    let mut previous_raw_end = 0;
    for span in manifest {
        let clean_valid = span.clean_span.start < span.clean_span.end
            && span.clean_span.end <= clean_text.len()
            && clean_text.is_char_boundary(span.clean_span.start)
            && clean_text.is_char_boundary(span.clean_span.end);
        let raw_valid = span.raw_span.start < span.raw_span.end
            && span.raw_span.end <= raw_text.len()
            && raw_text.is_char_boundary(span.raw_span.start)
            && raw_text.is_char_boundary(span.raw_span.end);
        result.invalid_clean_bounds += usize::from(!clean_valid);
        result.invalid_raw_bounds += usize::from(!raw_valid);
        result.overlapping_clean_spans += usize::from(span.clean_span.start < previous_clean_end);
        result.non_monotonic_raw_spans += usize::from(span.raw_span.start < previous_raw_end);
        previous_clean_end = previous_clean_end.max(span.clean_span.end);
        previous_raw_end = previous_raw_end.max(span.raw_span.end);

        if !clean_valid {
            continue;
        }
        let token = &clean_text[span.clean_span.clone()];
        let Some(restored) = session.restore(token) else {
            result.token_restore_failures += 1;
            continue;
        };
        if raw_valid && restored != raw_text[span.raw_span.clone()] {
            result.raw_value_mismatches += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_floor_response(fixture_id: &str, locale: &str, text: &str) -> Response {
        let config = BenchConfig::RuleFloorExtended;
        let full = build_pipeline(config).expect("rule-floor-extended pipeline should build");
        let floor = build_pipeline(floor_config(config)).expect("floor pipeline should build");
        let request = Request {
            fixture_id: fixture_id.to_string(),
            locale_chain: vec![locale.to_string()],
            text: text.to_string(),
        };

        match handle_request(config, &full, &floor, None, request)
            .expect("synthetic request should be handled")
        {
            Outcome::Success(response) => response,
            outcome => panic!("expected a success response, got {outcome:?}"),
        }
    }

    fn assert_success_contract(
        response: &Response,
        fixture_id: &str,
        raw_text: &str,
        clean_protected_value: &str,
        trace_forbidden_values: &[&str],
    ) {
        assert_eq!(response.fixture_id, fixture_id);
        assert!(
            response
                .manifest_spans
                .iter()
                .any(|span| span.class == "email"),
            "the deterministic rule floor should tokenize the synthetic email"
        );
        assert!(!response.clean_text.contains(clean_protected_value));
        assert!(response.restore.exact);
        assert_eq!(response.safety_net_mode, "strict");

        let integrity = &response.manifest_integrity;
        assert!(integrity.spans >= 1);
        assert_eq!(integrity.token_restore_failures, 0);
        assert_eq!(integrity.raw_value_mismatches, 0);
        assert_eq!(integrity.invalid_clean_bounds, 0);
        assert_eq!(integrity.invalid_raw_bounds, 0);
        assert_eq!(integrity.overlapping_clean_spans, 0);
        assert_eq!(integrity.non_monotonic_raw_spans, 0);

        let serialized = serde_json::to_value(response).expect("response should serialize");
        let timing = serialized
            .get("timing")
            .and_then(serde_json::Value::as_object)
            .expect("response should contain a timing object");
        assert_eq!(timing.len(), 6);
        for field in [
            "total_ms",
            "pass1_ms",
            "pass2_ms",
            "pass3_ms",
            "restore_ms",
            "post_policy_scan_ms",
        ] {
            assert!(timing.contains_key(field), "missing timing field {field}");
        }

        let trace_value = serialized
            .get("final_protection_trace")
            .and_then(serde_json::Value::as_array)
            .expect("success response should contain a protection trace array");
        assert_eq!(trace_value.len(), response.final_protection_trace.len());
        assert!(!response.final_protection_trace.is_empty());

        let mut previous_raw_end = 0usize;
        for (item, serialized_item) in response
            .final_protection_trace
            .iter()
            .zip(trace_value.iter())
        {
            assert!(item.raw_start < item.raw_end);
            assert!(item.raw_end <= raw_text.len());
            assert!(raw_text.is_char_boundary(item.raw_start));
            assert!(raw_text.is_char_boundary(item.raw_end));
            assert!(item.raw_start >= previous_raw_end);
            previous_raw_end = item.raw_end;
            assert!(matches!(
                (
                    item.provenance.stage.as_str(),
                    item.provenance.decision.as_str(),
                    item.action.as_str(),
                ),
                ("primary_pipeline", "policy", "tokenize")
                    | ("safety_net", "resolve", "tokenize")
                    | ("safety_net", "redact", "redact")
                    | ("safety_net", "fallback_redact", "redact")
            ));
            assert_eq!(item.provenance.stage, "primary_pipeline");
            assert_eq!(item.provenance.decision, "policy");
            assert_eq!(item.action, "tokenize");
            assert!(!item.provenance.source_ids.is_empty());
            assert!(item
                .provenance
                .source_ids
                .iter()
                .all(|source_id| !source_id.is_empty()));
            assert!(item
                .provenance
                .source_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1]));
            assert_eq!(
                response
                    .manifest_spans
                    .iter()
                    .filter(|span| {
                        span.raw_start == item.raw_start
                            && span.raw_end == item.raw_end
                            && span.class == item.class
                    })
                    .count(),
                1
            );

            let object = serialized_item
                .as_object()
                .expect("trace item should be an object");
            assert_eq!(object.len(), 5);
            for field in ["raw_start", "raw_end", "class", "action", "provenance"] {
                assert!(object.contains_key(field), "missing trace field {field}");
            }
            let provenance = object["provenance"]
                .as_object()
                .expect("trace provenance should be an object");
            assert_eq!(provenance.len(), 3);
            for field in ["stage", "decision", "source_ids"] {
                assert!(
                    provenance.contains_key(field),
                    "missing provenance field {field}"
                );
            }
        }
        for span in &response.manifest_spans {
            assert_eq!(
                response
                    .final_protection_trace
                    .iter()
                    .filter(|item| {
                        item.action == "tokenize"
                            && item.raw_start == span.raw_start
                            && item.raw_end == span.raw_end
                            && item.class == span.class
                    })
                    .count(),
                1
            );
        }
        let serialized_trace = serde_json::to_string(&response.final_protection_trace)
            .expect("trace should serialize");
        for protected_value in trace_forbidden_values {
            assert!(!serialized_trace.contains(protected_value));
        }
    }

    #[test]
    fn en_rule_floor_success_response_contract() {
        let text = "Contact alice@example.invalid or call +1-555-0142.";
        let response = rule_floor_response("en-1", "en", text);

        assert_success_contract(
            &response,
            "en-1",
            text,
            "alice@example.invalid",
            &["alice@example.invalid", "+1-555-0142"],
        );
    }

    #[test]
    fn de_rule_floor_success_response_contract() {
        let text = "Bitte an dr.schmidt@example.invalid schreiben, Tel. +49 1555 0112233.";
        let response = rule_floor_response("de-1", "de", text);

        assert_success_contract(
            &response,
            "de-1",
            text,
            "dr.schmidt@example.invalid",
            &["dr.schmidt@example.invalid", "+49 1555 0112233"],
        );
    }

    #[test]
    fn benchmark_rule_floor_matches_production_assembly_for_en_and_de() {
        let fixtures = [
            (
                LocaleTag::EnUs,
                "Contact alice@example.invalid or call +1 555 0100.",
            ),
            (
                LocaleTag::DeDe,
                "Bitte an dr.schmidt@example.invalid schreiben, Tel. +49 1555 0112233.",
            ),
        ];

        for (locale, raw_text) in fixtures {
            let benchmark = assemble_rule_floor(BenchConfig::RuleFloorExtended, None)
                .expect("benchmark assembly should build");
            let production = gaze_assembly::CorePipelineConfig::new()
                .with_bundled_rulepack("core-extended")
                .with_locale(BENCHMARK_ACTIVE_LOCALES)
                .build()
                .expect("production assembly should build")
                .into_pipeline();
            let session = Session::new(Scope::Ephemeral).expect("session should build");

            let (benchmark_doc, benchmark_manifest, _) = benchmark
                .clean_with_safety_net(
                    &session,
                    RawDocument::Text(raw_text.to_string()),
                    std::slice::from_ref(&locale),
                )
                .expect("benchmark clean should succeed");
            let (production_doc, production_manifest, _) = production
                .clean_with_safety_net(
                    &session,
                    RawDocument::Text(raw_text.to_string()),
                    std::slice::from_ref(&locale),
                )
                .expect("production clean should succeed");

            let CleanDocument::Text(benchmark_text) = benchmark_doc else {
                panic!("benchmark should return text");
            };
            let CleanDocument::Text(production_text) = production_doc else {
                panic!("production should return text");
            };
            assert_eq!(benchmark_text, production_text);
            assert_eq!(
                manifest_signature(&benchmark_manifest),
                manifest_signature(&production_manifest)
            );
            assert!(manifest_signature(&benchmark_manifest)
                .iter()
                .any(|(_, _, class)| class == "email"));
            assert!(manifest_signature(&benchmark_manifest)
                .iter()
                .any(|(_, _, class)| class == "custom:phone"));

            let (benchmark_restore, _) = benchmark
                .restore_with_telemetry(&session, &benchmark_text)
                .expect("benchmark restore should succeed");
            let (production_restore, _) = production
                .restore_with_telemetry(&session, &production_text)
                .expect("production restore should succeed");
            assert_eq!(benchmark_restore.text, raw_text);
            assert_eq!(production_restore.text, raw_text);
        }
    }

    #[test]
    fn assembly_absence_errors_are_typed_and_fail_explicitly() {
        let missing_model = match assemble_rule_floor(BenchConfig::Pass2Ner, None) {
            Ok(_) => panic!("missing NER model configuration must fail"),
            Err(error) => error,
        };
        assert!(matches!(
            missing_model,
            BenchmarkBuildError::MissingNerModelDir
        ));

        let empty_model_dir = tempfile::tempdir().expect("empty model dir should build");
        let missing_artifacts = match assemble_rule_floor(
            BenchConfig::Pass2Ner,
            Some(NerSettings {
                model_dir: empty_model_dir.path().to_path_buf(),
                locale: None,
                threshold: gaze::DEFAULT_NER_THRESHOLD,
            }),
        ) {
            Ok(_) => panic!("missing NER artifacts must fail"),
            Err(error) => error,
        };
        assert!(matches!(
            missing_artifacts,
            BenchmarkBuildError::Assembly(gaze_assembly::BuildError::Policy(
                gaze::PolicyError::NerLoad(_)
            ))
        ));

        let mut policy = gaze::Policy::default();
        policy.rules = vec![gaze::RuleSpec::Default {
            action: Action::Tokenize,
        }];
        let context = empty_context();
        let locales = LocaleChain::from_tags(vec![LocaleTag::EnUs]);
        let missing_recognizers =
            match gaze_assembly::build_pipeline(&policy, &context, &[], &locales, None) {
                Ok(_) => panic!("missing recognizers must fail"),
                Err(error) => error,
            };
        assert!(matches!(
            missing_recognizers,
            gaze_assembly::BuildError::NoRecognizers
        ));
    }

    #[test]
    fn success_response_keeps_empty_protection_trace_field() {
        let response = rule_floor_response("empty-1", "en", "nothing sensitive here.");
        assert!(response.final_protection_trace.is_empty());
        let serialized = serde_json::to_value(&response).expect("response should serialize");
        assert_eq!(
            serialized.get("final_protection_trace"),
            Some(&serde_json::Value::Array(Vec::new()))
        );
    }

    #[test]
    fn pipeline_error_codes_cover_each_mapping_arm() {
        let cases = [
            (
                gaze::Error::SafetyNet(SafetyNetError::InvalidOutput {
                    message: "synthetic invalid output".to_string(),
                }),
                "safety_net_invalid_output",
            ),
            (
                gaze::Error::SafetyNet(SafetyNetError::Runtime {
                    message: "synthetic runtime failure".to_string(),
                }),
                "safety_net_runtime",
            ),
            (
                gaze::Error::SafetyNet(SafetyNetError::Unavailable {
                    reason: "synthetic unavailable backend".to_string(),
                }),
                "safety_net_error",
            ),
            (gaze::Error::ExportForbidden, "pipeline_error"),
        ];

        for (error, expected) in cases {
            assert_eq!(pipeline_error_code(&error), expected);
        }
    }

    #[test]
    fn pipeline_error_response_has_exact_jsonl_shape() {
        let response = PipelineErrorResponse {
            fixture_id: "failure-1",
            pipeline_error_stage: "clean",
            pipeline_error_code: "safety_net_runtime",
            timing: PipelineErrorTiming { total_ms: 12.5 },
        };
        let serialized = serde_json::to_string(&response).expect("error response should serialize");
        assert_eq!(
            serialized,
            r#"{"fixture_id":"failure-1","pipeline_error_stage":"clean","pipeline_error_code":"safety_net_runtime","timing":{"total_ms":12.5}}"#
        );

        let value = serde_json::to_value(&response).expect("error response should serialize");
        let object = value
            .as_object()
            .expect("error response should be a JSON object");
        assert_eq!(object.len(), 4);
        for field in [
            "fixture_id",
            "pipeline_error_stage",
            "pipeline_error_code",
            "timing",
        ] {
            assert!(object.contains_key(field), "missing error field {field}");
        }
        assert!(!object.contains_key("final_protection_trace"));
        let timing = object["timing"]
            .as_object()
            .expect("error timing should be a JSON object");
        assert_eq!(timing.len(), 1);
        assert!(timing.contains_key("total_ms"));

        let mut output = Vec::new();
        write_pipeline_error(
            &mut output,
            "failure-1",
            "clean",
            "safety_net_runtime",
            12.5,
        )
        .expect("error response should be written");
        assert_eq!(output, format!("{serialized}\n").as_bytes());
    }

    #[cfg(feature = "safety-net-kiji")]
    #[test]
    #[ignore = "requires NER and Kiji model environment; records known baseline debt"]
    fn kiji_response_contract_records_restore_and_manifest_debt() {
        let config = BenchConfig::FullStackKijiResolve;
        let full = build_pipeline(config).expect("full Kiji pipeline should build from model env");
        let floor = build_pipeline(floor_config(config)).expect("floor pipeline should build");
        let pre_safety =
            build_pipeline(BenchConfig::Pass2Ner).expect("pre-safety NER pipeline should build");
        let request = Request {
            fixture_id: "de-kiji-debt-1".to_string(),
            locale_chain: vec!["de".to_string()],
            text: "Bitte an dr.schmidt@example.invalid schreiben, Tel. +49 1555 0112233."
                .to_string(),
        };
        let response = match handle_request(config, &full, &floor, Some(&pre_safety), request)
            .expect("Kiji request should be handled")
        {
            Outcome::Success(response) => response,
            outcome => panic!("expected a Kiji response, got {outcome:?}"),
        };

        // Recorded baseline debt (~69.46% exact restore, ~36.85% valid manifest): false
        // restore.exact and non-zero integrity violations are debt signals, not expected-good.
        let value = serde_json::to_value(&response).expect("Kiji response should serialize");
        assert!(value["restore"]["exact"].is_boolean());
        for field in [
            "invalid_clean_bounds",
            "invalid_raw_bounds",
            "overlapping_clean_spans",
            "non_monotonic_raw_spans",
            "token_restore_failures",
            "raw_value_mismatches",
        ] {
            assert!(
                value["manifest_integrity"][field].is_u64(),
                "Kiji response should record integrity debt field {field}"
            );
        }

        let integrity = &response.manifest_integrity;
        let violation_count = integrity.invalid_clean_bounds
            + integrity.invalid_raw_bounds
            + integrity.overlapping_clean_spans
            + integrity.non_monotonic_raw_spans
            + integrity.token_restore_failures
            + integrity.raw_value_mismatches;
        eprintln!(
            "Kiji baseline debt signals: restore_exact={}, manifest_violations={violation_count}",
            response.restore.exact
        );
    }
}
