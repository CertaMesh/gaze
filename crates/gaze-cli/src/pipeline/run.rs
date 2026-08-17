use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;

use gaze::{
    dictionary_bundle_from_context, Action, DictionaryBundle, DictionarySource, DocumentKind,
    LeakKind, LeakReport, LeakReportTelemetry, LocaleTag, PiiClass, Policy, RawDocument,
    RedactionEntry, RedactionLogError, RedactionLogger, Result as GazeResult, RuleSpec, Rulepack,
    RulepackSource, Scope, SensitiveSnapshot, Session, SessionPolicy, SessionScope,
    SessionSnapshotEntry, TypedContext,
};
use gaze_audit::{LeakSuspectLogEntry, LeakSuspectLogger, SqliteLogger};

use crate::clean_overrides::CleanOverrides;
use crate::commands::{
    KijiBackend, KijiDistilbertPrecision, OpenAiFilterDevice, OpenAiFilterOperatingPoint,
    SafetyNetBackend, SafetyNetFallback, SafetyNetKind, SafetyNetMode,
    DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES, DEFAULT_SAFETY_NET_TIMEOUT_MS,
};
use crate::error::CliError;
use crate::io::{read_stdin_text, require_json_format};
use crate::pipeline::build::{
    build_context_pipeline, build_pipeline_from_policy, build_stub_pipeline,
    dictionary_terms_from_rulepacks, load_rulepacks, map_pipeline_error, map_policy_error,
    merged_rulepack_default_locales, resolve_ner_threshold, validate_ner_threshold,
    warn_uncovered_collision_families, ArcLogger,
};

const CORE_EXTENDED_DEPRECATION: &str = "`--rulepack-bundled core-extended` is deprecated since v0.8.0; use `--rulepack-bundled core --locale=<lang>` for explicit activation";

pub(crate) struct CleanOptions<'a> {
    pub(crate) policy: Option<&'a Path>,
    pub(crate) format: &'a str,
    pub(crate) session_ttl: Option<u64>,
    pub(crate) session_scope: Option<&'a str>,
    pub(crate) locale: &'a [String],
    pub(crate) ner_threshold: Option<f32>,
    pub(crate) ner_model_dir: Option<PathBuf>,
    pub(crate) ner_locale: Option<&'a str>,
    pub(crate) rulepack_bundled: &'a [String],
    pub(crate) rulepack_paths: Vec<PathBuf>,
    pub(crate) max_bytes: u64,
    pub(crate) context_json: Option<&'a Path>,
    pub(crate) audit_db: Option<&'a Path>,
    pub(crate) safety_net: Option<SafetyNetKind>,
    pub(crate) safety_net_backend: Option<SafetyNetBackend>,
    pub(crate) safety_net_registry: bool,
    pub(crate) safety_net_add: &'a [SafetyNetBackend],
    pub(crate) openai_filter_command: Option<&'a Path>,
    pub(crate) openai_filter_checkpoint: Option<&'a Path>,
    pub(crate) openai_filter_operating_point: Option<OpenAiFilterOperatingPoint>,
    pub(crate) openai_filter_device: OpenAiFilterDevice,
    pub(crate) kiji_backend: KijiBackend,
    pub(crate) kiji_distilbert_precision: KijiDistilbertPrecision,
    pub(crate) opf_locales: &'a [String],
    pub(crate) opf_command: Option<&'a Path>,
    pub(crate) opf_checkpoint: Option<&'a Path>,
    pub(crate) kiji_distilbert_command: Option<&'a Path>,
    pub(crate) kiji_distilbert_model_dir: Option<&'a Path>,
    pub(crate) kiji_distilbert_locales: &'a [String],
    pub(crate) safety_net_timeout_ms: u64,
    pub(crate) safety_net_input_limit_bytes: usize,
    pub(crate) safety_net_mode: SafetyNetMode,
    pub(crate) safety_net_fallback: SafetyNetFallback,
}

/// Resolves the active Pass-3 SafetyNet backend.
///
/// `--safety-net-backend` takes precedence when explicitly different from the
/// default (`openai-filter`). Otherwise we fall back to the value of
/// `--safety-net=<kind>`. Returns `None` when no safety net is active.
pub(crate) fn effective_safety_net_backend(options: &CleanOptions<'_>) -> Option<SafetyNetBackend> {
    let activator = options.safety_net?;
    if let Some(backend) = options.safety_net_backend {
        return Some(backend);
    }
    Some(SafetyNetBackend::from(activator))
}

pub(crate) fn run_clean(options: CleanOptions<'_>) -> std::result::Result<(), CliError> {
    require_json_format(options.format)?;
    let cli_ner_threshold = options
        .ner_threshold
        .map(validate_ner_threshold)
        .transpose()
        .map_err(map_policy_error)?;
    let clean_overrides = clean_overrides_from_options(&options)?;
    let raw = read_stdin_text(options.max_bytes)?;

    let counter = Arc::new(CountingLogger::new(options.audit_db).map_err(|_| CliError::Pipeline)?);
    let loaded_policy = match options.policy {
        Some(path) => {
            let policy = Policy::load_for_cli(path).map_err(map_policy_error)?;
            Some(clean_overrides.apply_to(&policy))
        }
        None => None,
    };
    let cli_rulepack_policy = if loaded_policy.is_none() && has_rulepack_overrides(&options) {
        Some(policy_for_rulepack_overrides(&clean_overrides)?)
    } else {
        None
    };
    let effective_policy = loaded_policy.as_ref().or(cli_rulepack_policy.as_ref());
    let loaded_rulepacks = match (&loaded_policy, &cli_rulepack_policy) {
        (Some(policy), _) | (None, Some(policy)) => {
            load_rulepacks(policy).map_err(map_pipeline_error)?
        }
        (None, None) => Vec::new(),
    };
    let context = options
        .context_json
        .map(TypedContext::load)
        .transpose()
        .map_err(|err| CliError::PolicyConfigDetail(format!("context json: {err}")))?;
    let context_bundle = context
        .as_ref()
        .map(dictionary_bundle_from_context)
        .unwrap_or_default();
    let rulepack_dictionaries =
        dictionary_terms_from_rulepacks(&loaded_rulepacks).map_err(map_pipeline_error)?;
    let policy_bundle = effective_policy
        .map(|policy| {
            let mut dictionaries = policy.dictionaries.clone();
            dictionaries.extend(rulepack_dictionaries);
            DictionaryBundle::from_rulepack_terms(&dictionaries)
        })
        .unwrap_or_default();
    let dictionaries = DictionaryBundle::merge(policy_bundle, context_bundle);
    let dictionary_stats = dictionaries.stats();
    let mut rulepack_default_locales = merged_rulepack_default_locales(&loaded_rulepacks);
    if effective_policy.is_some_and(|policy| policy.rulepacks.auto_activate_locale_gated) {
        for locale in [
            LocaleTag::EnUs,
            LocaleTag::DeDe,
            LocaleTag::DeAt,
            LocaleTag::DeCh,
        ] {
            if !rulepack_default_locales
                .iter()
                .any(|existing| existing == &locale)
            {
                rulepack_default_locales.push(locale);
            }
        }
    }
    let cli_locales = parse_cli_locales(options.locale)?;
    let locale_chain = gaze::LocaleChain::merge_cli_policy_rulepack_default(
        cli_locales.as_deref(),
        effective_policy.and_then(|policy| policy.locale.as_deref()),
        Some(&rulepack_default_locales),
    );
    let resolved_ner_threshold = resolve_ner_threshold(cli_ner_threshold, effective_policy);

    let pipeline = match effective_policy {
        Some(policy) => build_pipeline_from_policy(
            policy,
            &loaded_rulepacks,
            context.as_ref(),
            &locale_chain,
            resolved_ner_threshold,
        )?
        .with_redaction_logger(ArcLogger(Arc::clone(&counter) as Arc<dyn RedactionLogger>)),
        None if context.is_some() => build_context_pipeline(
            context.as_ref().expect("checked context"),
            Arc::clone(&counter) as Arc<dyn RedactionLogger>,
        )
        .map_err(|err| CliError::PolicyConfigDetail(format!("context pipeline build: {err}")))?,
        None => {
            tracing::warn!("gaze clean running with stub pipeline because --policy was omitted");
            build_stub_pipeline(Arc::clone(&counter) as Arc<dyn RedactionLogger>).map_err(
                |err| CliError::PolicyConfigDetail(format!("stub pipeline build: {err}")),
            )?
        }
    };
    let pipeline = maybe_register_safety_net(pipeline, &options)?;
    validate_safety_net_tolerant_gate(options.safety_net_mode, options.safety_net_fallback)?;
    if matches!(
        options.safety_net_mode,
        SafetyNetMode::Strict | SafetyNetMode::Tolerant
    ) && options.safety_net_fallback != SafetyNetFallback::Redact
    {
        eprintln!("warning: --safety-net-fallback is ignored when --safety-net-mode is terminal");
    }

    let session = match effective_policy {
        Some(policy) => Session::from_policy_with_ttl_override(policy, options.session_ttl),
        None => Session::new(scope_for_cli_without_policy(
            clean_overrides.session_scope.as_ref(),
            options.session_ttl,
        )?),
    }
    .map_err(|_| CliError::Pipeline)?;

    let safety_net_active = options.safety_net.is_some() || options.safety_net_registry;
    let (clean_doc, leak_report) = if safety_net_active {
        let (doc, _manifest, _report) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw),
                locale_chain.as_slice(),
                &dictionaries,
                safety_net_policy(options.safety_net_mode, options.safety_net_fallback),
            )
            .map_err(map_safety_net_pipeline_error)?;
        (doc, _report)
    } else {
        let doc = pipeline
            .pseudonymize_with_detect_context(
                &session,
                RawDocument::Text(raw),
                locale_chain.as_slice(),
                &dictionaries,
            )
            .map_err(|_| CliError::Pipeline)?;
        (doc, LeakReport::default())
    };
    counter
        .log_safety_net_report(&leak_report, &session, DocumentKind::Text)
        .map_err(|_| CliError::Pipeline)?;
    enforce_safety_net_mode(
        &leak_report,
        options.safety_net_mode,
        options.safety_net_fallback,
    )?;

    let clean_text = match clean_doc {
        gaze::CleanDocument::Text(text) => text,
        gaze::CleanDocument::Structured(_) => {
            unreachable!(
                "clean submits only RawDocument::Text; library cannot produce Structured output from Text input"
            )
        }
        _ => unreachable!(
            "clean submits only RawDocument::Text; library cannot produce unknown output from Text input"
        ),
    };

    let entries = session
        .snapshot_entries()
        .into_iter()
        .map(EntryJson::from)
        .collect();
    let snapshot: SensitiveSnapshot = session.export().map_err(|_| CliError::Pipeline)?;
    let session_blob = BASE64.encode(snapshot.into_bytes());

    let response = CleanResponse {
        clean_text,
        session_blob,
        entries,
        stats: Stats {
            detections: counter.detections.load(Ordering::Relaxed),
            locale_chain: locale_chain.to_strings(),
            dictionaries_loaded: dictionary_stats
                .into_iter()
                .map(LoadedDictionaryStats::from)
                .collect(),
            context_source: options.context_json.map(|_| "cli".to_string()),
        },
        leak_report: LeakReportResponse::from(&leak_report),
    };
    let json = serde_json::to_string(&response).map_err(|_| CliError::Pipeline)?;
    // Surface fail-open policy coverage gaps only once the clean succeeded — on
    // an error path there is no output to leak, and the warning must not corrupt
    // the single-line JSON error envelope on stderr (issue #360).
    if let Some(policy) = effective_policy {
        warn_uncovered_collision_families(policy, &loaded_rulepacks, &locale_chain);
    }
    println!("{json}");
    Ok(())
}

pub(crate) fn maybe_register_safety_net(
    pipeline: gaze::Pipeline,
    options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    if options.safety_net_registry {
        if options.safety_net_backend.is_some() {
            return Err(CliError::SafetyNetConfigDetail(
                "--safety-net-registry cannot be combined with --safety-net-backend".to_string(),
            ));
        }
        if options.safety_net_add.is_empty() {
            return Err(CliError::SafetyNetConfigDetail(
                "--safety-net-registry requires at least one --safety-net-add".to_string(),
            ));
        }
        return register_safety_net_registry(pipeline, options);
    }
    let Some(backend) = effective_safety_net_backend(options) else {
        validate_no_backend_options(options)?;
        return Ok(pipeline);
    };
    match backend {
        SafetyNetBackend::OpenaiFilter => register_openai_filter(pipeline, options),
        SafetyNetBackend::KijiDistilbert => register_kiji_distilbert(pipeline, options),
    }
}

#[cfg(any(feature = "safety-net-openai", feature = "safety-net-kiji"))]
fn parse_backend_locales(
    raw: &[String],
    flag: &str,
) -> std::result::Result<Vec<LocaleTag>, CliError> {
    raw.iter()
        .map(|locale| {
            LocaleTag::parse(locale).map_err(|err| {
                CliError::SafetyNetConfigDetail(format!("invalid {flag} '{locale}': {err}"))
            })
        })
        .collect()
}

#[cfg(feature = "safety-net-openai")]
fn openai_filter_command_option<'a>(options: &'a CleanOptions<'_>) -> Option<&'a Path> {
    options.opf_command.or(options.openai_filter_command)
}

#[cfg(feature = "safety-net-openai")]
fn openai_filter_checkpoint_option<'a>(options: &'a CleanOptions<'_>) -> Option<&'a Path> {
    options.opf_checkpoint.or(options.openai_filter_checkpoint)
}

#[cfg(any(feature = "safety-net-openai", feature = "safety-net-kiji"))]
fn register_safety_net_registry(
    pipeline: gaze::Pipeline,
    options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    let mut registry = gaze_recognizers::LocaleAwareModelRegistry::new();
    for backend in options.safety_net_add {
        match backend {
            SafetyNetBackend::OpenaiFilter => register_openai_filter_model(&mut registry, options)?,
            SafetyNetBackend::KijiDistilbert => {
                register_kiji_distilbert_model(&mut registry, options)?
            }
        }
    }
    Ok(pipeline.with_safety_net_registry(registry))
}

#[cfg(not(any(feature = "safety-net-openai", feature = "safety-net-kiji")))]
fn register_safety_net_registry(
    _pipeline: gaze::Pipeline,
    _options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    Err(CliError::SafetyNetConfigDetail(
        "safety-net registry requested but gaze-cli was not compiled with a safety-net backend feature"
            .to_string(),
    ))
}

#[cfg(feature = "safety-net-openai")]
fn register_openai_filter_model(
    registry: &mut gaze_recognizers::LocaleAwareModelRegistry,
    options: &CleanOptions<'_>,
) -> std::result::Result<(), CliError> {
    use gaze_recognizers::safety_net::openai_filter::OpenAiFilterSafetyNet;
    let config = openai_filter_config(options, "--safety-net-add openai-filter")?;
    let locales = parse_backend_locales(options.opf_locales, "--opf-locales")?;
    let net = if locales.is_empty() {
        OpenAiFilterSafetyNet::new(config)
    } else {
        OpenAiFilterSafetyNet::new(config).with_locales(locales)
    };
    registry.register(net);
    Ok(())
}

#[cfg(all(
    not(feature = "safety-net-openai"),
    any(feature = "safety-net-openai", feature = "safety-net-kiji")
))]
fn register_openai_filter_model(
    _registry: &mut gaze_recognizers::LocaleAwareModelRegistry,
    _options: &CleanOptions<'_>,
) -> std::result::Result<(), CliError> {
    Err(CliError::SafetyNetConfigDetail(
        "openai-filter backend requested but gaze-cli was not compiled with feature safety-net-openai"
            .to_string(),
    ))
}

#[cfg(feature = "safety-net-kiji")]
fn register_kiji_distilbert_model(
    registry: &mut gaze_recognizers::LocaleAwareModelRegistry,
    options: &CleanOptions<'_>,
) -> std::result::Result<(), CliError> {
    use gaze_recognizers::safety_net::kiji_distilbert::KijiDistilbertSafetyNet;
    let config = kiji_distilbert_config(options, "--safety-net-add kiji-distilbert")?;
    let locales =
        parse_backend_locales(options.kiji_distilbert_locales, "--kiji-distilbert-locales")?;
    let net = if locales.is_empty() {
        KijiDistilbertSafetyNet::new(config)
    } else {
        KijiDistilbertSafetyNet::new(config).with_locales(locales)
    };
    registry.register(net);
    Ok(())
}

#[cfg(all(
    not(feature = "safety-net-kiji"),
    any(feature = "safety-net-openai", feature = "safety-net-kiji")
))]
fn register_kiji_distilbert_model(
    _registry: &mut gaze_recognizers::LocaleAwareModelRegistry,
    _options: &CleanOptions<'_>,
) -> std::result::Result<(), CliError> {
    Err(CliError::SafetyNetConfigDetail(
        "kiji-distilbert backend requested but gaze-cli was not compiled with feature safety-net-kiji"
            .to_string(),
    ))
}

#[cfg(feature = "safety-net-openai")]
fn register_openai_filter(
    pipeline: gaze::Pipeline,
    options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    use gaze_recognizers::safety_net::openai_filter::OpenAiFilterSafetyNet;

    Ok(
        pipeline.with_safety_net(OpenAiFilterSafetyNet::new(openai_filter_config(
            options,
            "--safety-net-backend=openai-filter",
        )?)),
    )
}

#[cfg(feature = "safety-net-openai")]
fn openai_filter_config(
    options: &CleanOptions<'_>,
    activation: &str,
) -> std::result::Result<
    gaze_recognizers::safety_net::openai_filter::SubprocessOpenAiFilterConfig,
    CliError,
> {
    use gaze_recognizers::safety_net::openai_filter::SubprocessOpenAiFilterConfig;

    let command = openai_filter_command_option(options).ok_or_else(|| {
        CliError::SafetyNetConfigDetail(format!(
            "--openai-filter-command or --opf-command is required for {activation}"
        ))
    })?;
    let checkpoint = openai_filter_checkpoint_option(options).ok_or_else(|| {
        CliError::SafetyNetConfigDetail(format!(
            "--openai-filter-checkpoint or --opf-checkpoint is required for {activation}"
        ))
    })?;
    let mut config = SubprocessOpenAiFilterConfig::new(command)
        .with_checkpoint_path(checkpoint)
        .with_timeout(Duration::from_millis(options.safety_net_timeout_ms))
        .with_max_input_bytes(options.safety_net_input_limit_bytes);
    let mut opf_args = vec!["--format", "json", "--output-mode", "typed"];
    if let Some(operating_point) = options.openai_filter_operating_point {
        let value = operating_point.as_opf_value();
        opf_args.extend(["--operating-point", value]);
        config = config.with_decoding_param("operating_point", value);
    }
    if let Some(device) = options.openai_filter_device.as_opf_value() {
        opf_args.extend(["--device", device]);
        config = config.with_decoding_param("device", device);
    }
    Ok(config.with_args(opf_args))
}

#[cfg(not(feature = "safety-net-openai"))]
fn register_openai_filter(
    _pipeline: gaze::Pipeline,
    _options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    Err(CliError::SafetyNetConfigDetail(
        "safety net requested but gaze-cli was not compiled with feature safety-net-openai"
            .to_string(),
    ))
}

#[cfg(feature = "safety-net-kiji")]
fn register_kiji_distilbert(
    pipeline: gaze::Pipeline,
    options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    use gaze_recognizers::safety_net::kiji_distilbert::KijiDistilbertSafetyNet;

    Ok(
        pipeline.with_safety_net(KijiDistilbertSafetyNet::new(kiji_distilbert_config(
            options,
            "--safety-net-backend=kiji-distilbert",
        )?)),
    )
}

#[cfg(feature = "safety-net-kiji")]
fn kiji_distilbert_config(
    options: &CleanOptions<'_>,
    activation: &str,
) -> std::result::Result<
    gaze_recognizers::safety_net::kiji_distilbert::KijiDistilbertConfig,
    CliError,
> {
    use gaze_recognizers::safety_net::kiji_distilbert::{
        KijiDistilbertConfig, KijiDistilbertPrecision as RecognizerKijiPrecision, OrtKijiConfig,
        SubprocessKijiConfig,
    };

    let model_dir = options.kiji_distilbert_model_dir.ok_or_else(|| {
        CliError::SafetyNetConfigDetail(format!(
            "--kiji-distilbert-model-dir is required for {activation}"
        ))
    })?;

    validate_kiji_artifacts(model_dir, options.kiji_distilbert_precision)?;

    match options.kiji_backend {
        KijiBackend::Subprocess => {
            if options.kiji_distilbert_precision != KijiDistilbertPrecision::Fp32 {
                return Err(CliError::SafetyNetConfigDetail(
                    "--kiji-distilbert-precision=int8 requires --kiji-backend=ort".to_string(),
                ));
            }
            let command = options.kiji_distilbert_command.ok_or_else(|| {
                CliError::SafetyNetConfigDetail(format!(
                    "--kiji-distilbert-command is required for {activation} with --kiji-backend=subprocess"
                ))
            })?;
            let config = SubprocessKijiConfig::new(command)
                .with_model_dir(model_dir)
                .with_timeout(Duration::from_millis(options.safety_net_timeout_ms))
                .with_max_input_bytes(options.safety_net_input_limit_bytes);
            Ok(KijiDistilbertConfig::from(config))
        }
        KijiBackend::Ort => {
            if options.kiji_distilbert_command.is_some() {
                return Err(CliError::SafetyNetConfigDetail(
                    "--kiji-distilbert-command is only valid with --kiji-backend=subprocess"
                        .to_string(),
                ));
            }
            let precision = match options.kiji_distilbert_precision {
                KijiDistilbertPrecision::Fp32 => RecognizerKijiPrecision::Fp32,
                KijiDistilbertPrecision::Int8 => RecognizerKijiPrecision::Int8,
            };
            let config = OrtKijiConfig::new(model_dir)
                .with_precision(precision)
                .with_max_input_bytes(options.safety_net_input_limit_bytes);
            Ok(KijiDistilbertConfig::from(config))
        }
        #[cfg(feature = "runtime-tract")]
        KijiBackend::Tract => {
            if options.kiji_distilbert_command.is_some() {
                return Err(CliError::SafetyNetConfigDetail(
                    "--kiji-distilbert-command is only valid with --kiji-backend=subprocess"
                        .to_string(),
                ));
            }
            let config =
                gaze_recognizers::safety_net::kiji_distilbert::TractKijiConfig::new(model_dir)
                    .with_max_input_bytes(options.safety_net_input_limit_bytes);
            Ok(KijiDistilbertConfig::from(config))
        }
        #[cfg(feature = "runtime-candle")]
        KijiBackend::Candle => {
            if options.kiji_distilbert_command.is_some() {
                return Err(CliError::SafetyNetConfigDetail(
                    "--kiji-distilbert-command is only valid with --kiji-backend=subprocess"
                        .to_string(),
                ));
            }
            let config =
                gaze_recognizers::safety_net::kiji_distilbert::CandleKijiConfig::new(model_dir)
                    .with_max_input_bytes(options.safety_net_input_limit_bytes);
            Ok(KijiDistilbertConfig::from(config))
        }
    }
}

#[cfg(feature = "safety-net-kiji")]
fn validate_kiji_artifacts(
    model_dir: &Path,
    precision: KijiDistilbertPrecision,
) -> std::result::Result<(), CliError> {
    use gaze_recognizers::safety_net::kiji_distilbert::{
        REQUIRED_KIJI_ARTIFACTS, REQUIRED_KIJI_INT8_ARTIFACTS,
    };

    // Pinned-artifact contract (Axis 1): the runtime never silently disables
    // the backend. Surface missing artifacts as a config-level error (exit 2)
    // before backend construction; SHA mismatches still fail in the backend.
    let required = match precision {
        KijiDistilbertPrecision::Fp32 => REQUIRED_KIJI_ARTIFACTS,
        KijiDistilbertPrecision::Int8 => REQUIRED_KIJI_INT8_ARTIFACTS,
    };
    for required in required {
        let artifact = model_dir.join(required);
        if !artifact.exists() {
            return Err(CliError::SafetyNetArtifactMissing {
                backend: "kiji-distilbert",
                path: format!(
                    "{} (install via scripts/fetch/fetch-kiji-safetynet-model.sh)",
                    artifact.display()
                ),
            });
        }
    }
    Ok(())
}

#[cfg(not(feature = "safety-net-kiji"))]
fn register_kiji_distilbert(
    _pipeline: gaze::Pipeline,
    _options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    Err(CliError::SafetyNetConfigDetail(
        "kiji-distilbert backend requested but gaze-cli was not compiled with feature safety-net-kiji"
            .to_string(),
    ))
}

fn validate_no_backend_options(options: &CleanOptions<'_>) -> std::result::Result<(), CliError> {
    if options.openai_filter_command.is_some()
        || options.openai_filter_checkpoint.is_some()
        || options.openai_filter_operating_point.is_some()
        || options.openai_filter_device != OpenAiFilterDevice::Auto
        || options.kiji_backend != KijiBackend::Subprocess
        || options.kiji_distilbert_precision != KijiDistilbertPrecision::Fp32
        || options.opf_command.is_some()
        || options.opf_checkpoint.is_some()
        || !options.opf_locales.is_empty()
        || options.kiji_distilbert_command.is_some()
        || options.kiji_distilbert_model_dir.is_some()
        || !options.kiji_distilbert_locales.is_empty()
        || !options.safety_net_add.is_empty()
        || options.safety_net_timeout_ms != DEFAULT_SAFETY_NET_TIMEOUT_MS
        || options.safety_net_input_limit_bytes != DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES
    {
        return Err(CliError::SafetyNetConfigDetail(
            "safety-net backend options require --safety-net=<kind> activation".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn map_safety_net_pipeline_error(err: gaze::Error) -> CliError {
    match err {
        gaze::Error::SafetyNet(err) => map_safety_net_error(err),
        _ => CliError::Pipeline,
    }
}

fn map_safety_net_error(err: gaze::SafetyNetError) -> CliError {
    match err {
        gaze::SafetyNetError::Unavailable { .. } => CliError::SafetyNetFailure {
            variant: "Unavailable",
        },
        gaze::SafetyNetError::WeightsMissing { .. } => CliError::SafetyNetFailure {
            variant: "WeightsMissing",
        },
        gaze::SafetyNetError::ModelUnavailable { .. } => CliError::SafetyNetFailure {
            variant: "ModelUnavailable",
        },
        gaze::SafetyNetError::ModelIntegrityMismatch { .. } => CliError::SafetyNetFailure {
            variant: "ModelIntegrityMismatch",
        },
        gaze::SafetyNetError::InputTooLarge { .. } => CliError::SafetyNetFailure {
            variant: "InputTooLarge",
        },
        gaze::SafetyNetError::Runtime { message } if message.contains("timed out") => {
            CliError::SafetyNetFailure { variant: "Timeout" }
        }
        gaze::SafetyNetError::Runtime { .. } => CliError::SafetyNetFailure { variant: "Runtime" },
        gaze::SafetyNetError::InvalidOutput { .. } => CliError::SafetyNetFailure {
            variant: "InvalidOutput",
        },
        _ => CliError::SafetyNetFailure { variant: "Unknown" },
    }
}

pub(crate) fn clean_overrides_from_options(
    options: &CleanOptions<'_>,
) -> std::result::Result<CleanOverrides, CliError> {
    let session_scope = options
        .session_scope
        .map(SessionScope::parse)
        .transpose()
        .map_err(map_policy_error)?;
    let ner_locale = options
        .ner_locale
        .map(|locale| {
            gaze::validate_ner_locale(locale)
                .map(|_| locale.to_string())
                .map_err(map_policy_error)
        })
        .transpose()?;
    let (rulepack_bundled, auto_activate_locale_gated) =
        normalize_rulepack_bundles(options.rulepack_bundled);
    let rulepack_bundled = if rulepack_bundled.is_empty() {
        None
    } else {
        Some(rulepack_bundled)
    };

    Ok(CleanOverrides {
        session_scope,
        ner_model_dir: options.ner_model_dir.clone(),
        ner_locale,
        rulepack_bundled,
        rulepack_paths: options.rulepack_paths.clone(),
        auto_activate_locale_gated,
    })
}

fn normalize_rulepack_bundles(raw: &[String]) -> (Vec<String>, bool) {
    let mut auto_activate_locale_gated = false;
    let mut bundled = Vec::with_capacity(raw.len());
    for bundle in raw {
        if bundle == "core-extended" {
            auto_activate_locale_gated = true;
            tracing::warn!("{CORE_EXTENDED_DEPRECATION}");
            eprintln!("warning: {CORE_EXTENDED_DEPRECATION}");
            if !bundled.iter().any(|existing| existing == "core") {
                bundled.push("core".to_string());
            }
        } else if !bundled.iter().any(|existing| existing == bundle) {
            bundled.push(bundle.clone());
        }
    }
    (bundled, auto_activate_locale_gated)
}

pub(crate) fn has_rulepack_overrides(options: &CleanOptions<'_>) -> bool {
    !options.rulepack_bundled.is_empty() || !options.rulepack_paths.is_empty()
}

pub(crate) fn policy_for_rulepack_overrides(
    clean_overrides: &CleanOverrides,
) -> std::result::Result<Policy, CliError> {
    let mut rules = class_rules_for_bundled_overrides(clean_overrides)?;
    rules.push(RuleSpec::Default {
        action: Action::Preserve,
    });
    let mut session = SessionPolicy::default();
    session.scope = SessionScope::Persistent;
    session.ttl_secs = Some(86_400);

    let mut base = Policy::default();
    base.session = session;
    base.rules = rules;
    Ok(clean_overrides.apply_to(&base))
}

fn class_rules_for_bundled_overrides(
    clean_overrides: &CleanOverrides,
) -> std::result::Result<Vec<RuleSpec>, CliError> {
    let Some(bundled) = &clean_overrides.rulepack_bundled else {
        return Ok(Vec::new());
    };
    let mut classes = std::collections::BTreeSet::new();
    for bundle in bundled {
        let contents = gaze_recognizers::embedded(bundle).ok_or_else(|| {
            CliError::PolicyConfigDetail(format!("unknown bundled rulepack: {bundle}"))
        })?;
        let rulepack = Rulepack::load(RulepackSource::Embedded(contents)).map_err(|err| {
            CliError::PolicyConfigDetail(format!("embedded rulepack '{bundle}': {err}"))
        })?;
        classes.extend(rulepack.activated_classes());
        classes.extend(
            rulepack
                .recognizers
                .iter()
                .filter(|recognizer| recognizer.enabled)
                .filter_map(|recognizer| {
                    recognizer
                        .collision
                        .as_ref()
                        .and_then(|collision| {
                            collision
                                .mandatory_anchor
                                .as_ref()
                                .map(|_| &collision.family)
                        })
                        .map(|family| PiiClass::family(family))
                }),
        );
    }
    Ok(classes
        .into_iter()
        .map(|class| RuleSpec::Class {
            class,
            action: Action::Tokenize,
        })
        .collect())
}

pub(crate) fn scope_for_cli_without_policy(
    scope: Option<&SessionScope>,
    ttl_secs: Option<u64>,
) -> std::result::Result<Scope, CliError> {
    match scope.unwrap_or(&SessionScope::Persistent) {
        SessionScope::Ephemeral => Ok(Scope::Ephemeral),
        SessionScope::Conversation => Ok(Scope::Conversation("cli".to_string())),
        SessionScope::Persistent => Ok(Scope::Persistent {
            ttl: Duration::from_secs(ttl_secs.unwrap_or(86_400)),
        }),
        _ => Err(CliError::UnsupportedSessionScope {
            variant: format!("{:?}", scope.unwrap_or(&SessionScope::Persistent)),
        }),
    }
}

pub(crate) fn parse_cli_locales(
    raw: &[String],
) -> std::result::Result<Option<Vec<LocaleTag>>, CliError> {
    if raw.is_empty() {
        return Ok(None);
    }
    raw.iter()
        .map(|locale| {
            LocaleTag::parse(locale).map_err(|err| {
                CliError::PolicyConfigDetail(format!("invalid --locale '{locale}': {err}"))
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map(Some)
}

struct CountingLogger {
    detections: AtomicU64,
    audit: Option<SqliteLogger>,
}

impl CountingLogger {
    fn new(audit_db: Option<&Path>) -> GazeResult<Self> {
        Ok(Self {
            detections: AtomicU64::new(0),
            audit: audit_db
                .map(SqliteLogger::new)
                .transpose()
                .map_err(|err| gaze::Error::Sqlite(err.to_string()))?,
        })
    }

    fn log_safety_net_report(
        &self,
        report: &LeakReport,
        session: &Session,
        document_kind: DocumentKind,
    ) -> gaze_audit::Result<()> {
        let Some(audit) = &self.audit else {
            return Ok(());
        };
        let created_at = chrono::Utc::now().timestamp_millis();
        for suspect in &report.suspects {
            let entry = LeakSuspectLogEntry::from_suspect(
                suspect,
                document_kind,
                created_at,
                Some(session.audit_session_id().to_string()),
                report.replay_hash.clone(),
            );
            audit.log_leak_suspect(&entry)?;
        }
        Ok(())
    }
}

impl RedactionLogger for CountingLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        if let Some(audit) = &self.audit {
            audit
                .log(entry)
                .map_err(|err| RedactionLogError::Sqlite(err.to_string()))?;
        }
        if !entry.conflict_loser
            && entry.document_kind == DocumentKind::Text
            && entry.action != gaze::Action::Preserve
        {
            self.detections.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct CleanResponse {
    clean_text: String,
    session_blob: String,
    entries: Vec<EntryJson>,
    stats: Stats,
    leak_report: LeakReportResponse,
}

#[derive(Debug, Clone, Serialize)]
struct EntryJson {
    class: String,
    raw: String,
    token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    family: Option<String>,
}

impl From<SessionSnapshotEntry> for EntryJson {
    fn from(entry: SessionSnapshotEntry) -> Self {
        Self {
            class: entry_class_to_string(&entry.class),
            raw: entry.raw,
            token: entry.token,
            family: Some(entry.family),
        }
    }
}

pub(crate) fn entry_class_to_string(class: &PiiClass) -> String {
    match class {
        PiiClass::Email => "Email".to_string(),
        PiiClass::Name => "Name".to_string(),
        PiiClass::Location => "Location".to_string(),
        PiiClass::Organization => "Organization".to_string(),
        PiiClass::Custom(name) => format!("Custom:{name}"),
    }
}

#[derive(Serialize)]
struct Stats {
    detections: u64,
    locale_chain: Vec<String>,
    dictionaries_loaded: Vec<LoadedDictionaryStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_source: Option<String>,
}

#[derive(Serialize)]
struct LoadedDictionaryStats {
    name: String,
    term_count: usize,
    source: String,
}

impl From<gaze::DictionaryStats> for LoadedDictionaryStats {
    fn from(stats: gaze::DictionaryStats) -> Self {
        let source = match stats.source {
            DictionarySource::Cli => "cli",
            DictionarySource::Rulepack => "rulepack",
            _ => "unknown",
        };
        Self {
            name: stats.name,
            term_count: stats.term_count,
            source: source.to_string(),
        }
    }
}

#[derive(Serialize)]
struct LeakReportResponse {
    stats: LeakReportStatsResponse,
    suspects: Vec<LeakSuspectResponse>,
    telemetry: Vec<LeakTelemetryResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replay_hash: Option<String>,
}

impl From<&LeakReport> for LeakReportResponse {
    fn from(report: &LeakReport) -> Self {
        Self {
            stats: LeakReportStatsResponse {
                suspect_count: report.stats.suspect_count,
                uncovered_count: report.stats.uncovered_count,
                partial_bleed_count: report.stats.partial_bleed_count,
                class_mismatch_count: report.stats.class_mismatch_count,
                locale_skipped_count: report.stats.locale_skipped_count,
            },
            suspects: report
                .suspects
                .iter()
                .map(LeakSuspectResponse::from)
                .collect(),
            telemetry: report
                .telemetry
                .iter()
                .map(LeakTelemetryResponse::from)
                .collect(),
            replay_hash: report.replay_hash.clone(),
        }
    }
}

#[derive(Serialize)]
struct LeakReportStatsResponse {
    suspect_count: usize,
    uncovered_count: usize,
    partial_bleed_count: usize,
    class_mismatch_count: usize,
    locale_skipped_count: usize,
}

#[derive(Serialize)]
struct LeakSuspectResponse {
    safety_net_id: String,
    raw_label: String,
    mapped_class: String,
    leak_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pipeline_class: Option<String>,
    span_len: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    field_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<f32>,
}

impl From<&gaze::LeakSuspect> for LeakSuspectResponse {
    fn from(suspect: &gaze::LeakSuspect) -> Self {
        let (leak_kind, pipeline_class) = match &suspect.kind {
            LeakKind::Uncovered => ("uncovered", None),
            LeakKind::PartialBleed { .. } => ("partial_bleed", None),
            LeakKind::ClassMismatch { pipeline_class, .. } => {
                ("class_mismatch", Some(pipeline_class.class_name()))
            }
            _ => ("unknown", None),
        };
        Self {
            safety_net_id: suspect.safety_net_id.clone(),
            raw_label: suspect.raw_label.clone(),
            mapped_class: suspect.class.class_name(),
            leak_kind: leak_kind.to_string(),
            pipeline_class,
            span_len: suspect.span.end.saturating_sub(suspect.span.start),
            field_path: suspect.field_path.clone(),
            score: suspect.score,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum LeakTelemetryResponse {
    LocaleSkipped {
        safety_net_id: String,
        document_kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        field_path: Option<String>,
    },
}

impl From<&LeakReportTelemetry> for LeakTelemetryResponse {
    fn from(event: &LeakReportTelemetry) -> Self {
        match event {
            LeakReportTelemetry::LocaleSkipped {
                safety_net_id,
                document_kind,
                field_path,
            } => Self::LocaleSkipped {
                safety_net_id: safety_net_id.clone(),
                document_kind: document_kind_label(*document_kind).to_string(),
                field_path: field_path.clone(),
            },
            _ => Self::LocaleSkipped {
                safety_net_id: "unknown".to_string(),
                document_kind: "unknown".to_string(),
                field_path: None,
            },
        }
    }
}

fn document_kind_label(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Structured => "structured",
        DocumentKind::Text => "text",
        _ => "unknown",
    }
}

pub(crate) fn enforce_safety_net_mode(
    report: &LeakReport,
    mode: SafetyNetMode,
    fallback: SafetyNetFallback,
) -> std::result::Result<(), CliError> {
    let suspected_leaks = report.stats.uncovered_count + report.stats.partial_bleed_count;
    if suspected_leaks > 0 {
        match mode {
            SafetyNetMode::Strict => {
                return Err(CliError::SafetyNetFailure {
                    variant: "SuspectedLeak",
                });
            }
            SafetyNetMode::Tolerant => emit_tolerant_deprecation_warning(),
            SafetyNetMode::Redact | SafetyNetMode::Resolve => {
                if fallback == SafetyNetFallback::Tolerant {
                    emit_tolerant_deprecation_warning();
                }
            }
        }
    }
    if report.stats.class_mismatch_count > 0 {
        emit_safety_net_warning("ClassMismatch", report.stats.class_mismatch_count);
    }
    Ok(())
}

pub(crate) fn validate_safety_net_tolerant_gate(
    mode: SafetyNetMode,
    fallback: SafetyNetFallback,
) -> std::result::Result<(), CliError> {
    if (mode == SafetyNetMode::Tolerant || fallback == SafetyNetFallback::Tolerant)
        && std::env::var_os("GAZE_ALLOW_TOLERANT").is_none()
    {
        return Err(CliError::SafetyNetFailure {
            variant: "TolerantModeDisabled",
        });
    }
    Ok(())
}

pub(crate) fn safety_net_policy(
    mode: SafetyNetMode,
    fallback: SafetyNetFallback,
) -> gaze::SafetyNetPolicy {
    gaze::SafetyNetPolicy::new(
        match mode {
            SafetyNetMode::Strict => gaze::SafetyNetMode::Strict,
            SafetyNetMode::Tolerant => gaze::SafetyNetMode::Tolerant,
            SafetyNetMode::Redact => gaze::SafetyNetMode::Redact,
            SafetyNetMode::Resolve => gaze::SafetyNetMode::Resolve,
        },
        match fallback {
            SafetyNetFallback::Strict => gaze::SafetyNetFallback::Strict,
            SafetyNetFallback::Tolerant => gaze::SafetyNetFallback::Tolerant,
            SafetyNetFallback::Redact => gaze::SafetyNetFallback::Redact,
        },
    )
}

fn emit_tolerant_deprecation_warning() {
    eprintln!(
        "warning: tolerant mode downgrades suspect leaks; deprecated v0.9, removal candidate v0.10."
    );
}

fn emit_safety_net_warning(variant: &'static str, count: usize) {
    eprintln!(r#"{{"warning":"SafetyNet","variant":"{variant}","count":{count}}}"#);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::build::resolve_ner_threshold;

    fn policy_with_ner_threshold(threshold: f32) -> Policy {
        let mut session = gaze::SessionPolicy::default();
        session.scope = gaze::SessionScope::Persistent;
        session.ttl_secs = Some(86_400);

        let mut ner = gaze::NerPolicy::default();
        ner.threshold = threshold;

        let mut rulepacks = gaze::RulepackPolicy::default();
        rulepacks.bundled = vec!["core".to_string()];

        let mut policy = Policy::default();
        policy.session = session;
        policy.rules = vec![gaze::RuleSpec::Default {
            action: gaze::Action::Preserve,
        }];
        policy.ner = Some(ner);
        policy.rulepacks = rulepacks;
        policy
    }

    #[test]
    fn t_cli_ner_threshold_overrides_policy_value() {
        let policy = policy_with_ner_threshold(0.5);

        let threshold = resolve_ner_threshold(Some(0.3), Some(&policy));

        assert_eq!(threshold, 0.3);
    }

    #[test]
    fn cli_ner_threshold_uses_policy_then_default() {
        let policy = policy_with_ner_threshold(0.5);

        assert_eq!(resolve_ner_threshold(None, Some(&policy)), 0.5);
        assert_eq!(
            resolve_ner_threshold(None, None),
            gaze::DEFAULT_NER_THRESHOLD
        );
    }
}
