use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, LeakKind, LocaleTag, PiiClass, Pipeline,
    RawDocument, RawMatch, RecognizerSpec, Rulepack, RulepackSource, SafetyNetFallback,
    SafetyNetMode, SafetyNetPolicy, Scope, Session,
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
    let full = build_pipeline(config)?;
    let floor = build_pipeline(floor_config(config))?;

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
                    pass2_ms: (config == BenchConfig::Pass2Ner)
                        .then_some((total_ms - pass1_ms).max(0.0)),
                    pass3_ms: match config {
                        BenchConfig::RuleFloorCore
                        | BenchConfig::RuleFloorExtended
                        | BenchConfig::Pass2Ner => 0.0,
                        BenchConfig::Pass3Kiji
                        | BenchConfig::Pass3Opf
                        | BenchConfig::Pass3LocaleAware => (total_ms - pass1_ms).max(0.0),
                    },
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
                "pass2-ner" => BenchConfig::Pass2Ner,
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
        | BenchConfig::Pass3Kiji
        | BenchConfig::Pass3Opf
        | BenchConfig::Pass3LocaleAware => BenchConfig::RuleFloorExtended,
    }
}

fn build_pipeline(config: BenchConfig) -> Result<Pipeline, Box<dyn std::error::Error>> {
    let mut pipeline = rule_floor_pipeline(config)?;
    match config {
        BenchConfig::RuleFloorCore | BenchConfig::RuleFloorExtended | BenchConfig::Pass2Ner => {}
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

    if config == BenchConfig::Pass2Ner {
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
