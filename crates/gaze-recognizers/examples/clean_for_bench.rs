use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, EmittedTokenSpan, LeakKind, LeakReportStats,
    LocaleTag, PiiClass, Pipeline, RawDocument, RawMatch, RecognizerSpec, Rulepack, RulepackSource,
    SafetyNetError, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};
use gaze_recognizers::{
    embedded, NerOptions, NerRecognizer, NormalizerKind, RegexDetector, ValidatorKind,
};
use serde::{Deserialize, Serialize};

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
    let clean_result = full.clean_with_safety_net_policy_detect_context(
        &session,
        RawDocument::Text(raw_text.clone()),
        &locale_chain,
        &Default::default(),
        safety_net_policy(config),
    );
    let total_ms = full_start.elapsed().as_secs_f64() * 1000.0;
    let (clean_doc, manifest, report) = match clean_result {
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

fn build_pipeline(config: BenchConfig) -> Result<Pipeline, Box<dyn std::error::Error>> {
    let mut pipeline = rule_floor_pipeline(config)?;
    match config {
        BenchConfig::RuleFloorCore | BenchConfig::RuleFloorExtended | BenchConfig::Pass2Ner => {}
        BenchConfig::FullStackKijiResolve => {
            pipeline = register_kiji_ort(pipeline)?;
        }
        BenchConfig::FullStackOpfResolve => {
            pipeline = register_opf(pipeline)?;
        }
        BenchConfig::Pass3Kiji => {
            pipeline = register_kiji(pipeline)?;
        }
        BenchConfig::Pass3Opf => {
            pipeline = register_opf(pipeline)?;
        }
        BenchConfig::Pass3LocaleAware => {
            pipeline = register_locale_aware(pipeline)?;
        }
    }
    Ok(pipeline)
}

fn rule_floor_pipeline(config: BenchConfig) -> Result<Pipeline, Box<dyn std::error::Error>> {
    let bundle = if config == BenchConfig::RuleFloorCore {
        "core"
    } else {
        "core-extended"
    };
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded(bundle).ok_or("missing embedded rulepack")?,
    ))?;
    let mut builder = Pipeline::builder()
        .rule(DefaultRule::new(Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("phone".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("ip_address".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("eth_address".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("postal_code".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("iban".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("credit_card".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("ssn".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("nino".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("pan".to_string()),
            Action::Tokenize,
        ));

    for spec in rulepack
        .recognizers
        .iter()
        .filter(|recognizer| recognizer.enabled)
    {
        if let Some(collision) = spec.collision.clone() {
            builder = builder.register_collision(spec.id.clone(), collision);
        }
        if matches!(spec.matcher, RawMatch::Regex { .. }) {
            builder = builder.recognizer(regex_from_spec(&rulepack, spec)?);
        }
    }

    if matches!(
        config,
        BenchConfig::Pass2Ner
            | BenchConfig::FullStackKijiResolve
            | BenchConfig::FullStackOpfResolve
    ) {
        let model_dir = std::env::var_os("GAZE_NER_MODEL_DIR")
            .map(PathBuf::from)
            .ok_or("GAZE_NER_MODEL_DIR is not set")?;
        let threshold = std::env::var("GAZE_NER_THRESHOLD")
            .ok()
            .map(|value| value.parse::<f32>())
            .transpose()?
            .unwrap_or(0.3);
        let recognizer = NerRecognizer::load_with_options(
            &model_dir,
            NerOptions {
                locale: std::env::var("GAZE_NER_LOCALE").ok(),
                threshold,
            },
        )?;
        builder = builder.recognizer(recognizer);
    }

    Ok(builder.build()?)
}

fn regex_from_spec(
    rulepack: &Rulepack,
    spec: &RecognizerSpec,
) -> Result<RegexDetector, Box<dyn std::error::Error>> {
    let RawMatch::Regex {
        pattern,
        pattern_template,
        capture_groups,
    } = &spec.matcher
    else {
        unreachable!("caller filters non-regex recognizers");
    };
    let pattern = match (pattern.as_deref(), pattern_template.as_deref()) {
        (Some(pattern), None) => pattern.to_string(),
        (None, Some(template)) => lower_pattern_template(rulepack, template)?,
        _ => return Err(format!("invalid regex recognizer pattern shape for {}", spec.id).into()),
    };
    let exclusions = spec
        .context
        .as_ref()
        .map(|context| context.exclusions.clone())
        .unwrap_or_default();
    let validator_kind = spec
        .validator
        .as_ref()
        .map(|validator| ValidatorKind::parse(&validator.kind))
        .transpose()?;
    let normalizer_kind = spec
        .normalizer
        .as_ref()
        .map(|normalizer| NormalizerKind::parse(&normalizer.kind))
        .transpose()?;

    Ok(RegexDetector::with_rulepack_fields(
        &pattern,
        spec.class.clone(),
        &spec.id,
        spec.locales.clone(),
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups.clone(),
        exclusions,
        validator_kind,
        normalizer_kind,
    )?)
}

fn lower_pattern_template(
    rulepack: &Rulepack,
    template: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let Some(locale) = &rulepack.locale else {
        return Err("pattern template requires locale buckets".into());
    };
    let mut pattern = template.to_string();
    for (bucket, values) in &locale.buckets {
        let marker = format!("{{locale.{bucket}}}");
        if pattern.contains(&marker) {
            let alternation = values
                .names
                .iter()
                .map(|value| regex::escape(value))
                .collect::<Vec<_>>()
                .join("|");
            pattern = pattern.replace(&marker, &alternation);
        }
    }
    if pattern.contains("{locale.") {
        return Err(format!("unresolved locale pattern template: {pattern}").into());
    }
    Ok(pattern)
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
