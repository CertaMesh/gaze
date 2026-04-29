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
    LeakKind, LeakReport, LeakReportTelemetry, LocaleTag, Policy, RawDocument, RedactionEntry,
    RedactionLogError, RedactionLogger, Result as GazeResult, RuleSpec, Rulepack, RulepackPolicy,
    RulepackSource, Scope, SensitiveSnapshot, Session, SessionPolicy, SessionScope, TypedContext,
};
use gaze_audit::{LeakSuspectLogEntry, SqliteLogger};

use crate::clean_overrides::CleanOverrides;
use crate::commands::{
    OpenAiFilterOperatingPoint, SafetyNetKind, SafetyNetMode, DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES,
    DEFAULT_SAFETY_NET_TIMEOUT_MS,
};
use crate::error::CliError;
use crate::io::{read_stdin_text, require_json_format};
use crate::pipeline::build::{
    build_context_pipeline, build_pipeline_from_policy, build_stub_pipeline,
    dictionary_terms_from_rulepacks, empty_fields, load_rulepacks, map_pipeline_error,
    map_policy_error, merged_rulepack_default_locales, resolve_ner_threshold,
    validate_ner_threshold, ArcLogger,
};

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
    pub(crate) openai_filter_command: Option<&'a Path>,
    pub(crate) openai_filter_checkpoint: Option<&'a Path>,
    pub(crate) openai_filter_operating_point: Option<OpenAiFilterOperatingPoint>,
    pub(crate) safety_net_timeout_ms: u64,
    pub(crate) safety_net_input_limit_bytes: usize,
    pub(crate) safety_net_mode: SafetyNetMode,
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
        .map_err(|_| CliError::PolicyConfig)?;
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
    let rulepack_default_locales = merged_rulepack_default_locales(&loaded_rulepacks);
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
        )
        .map_err(map_pipeline_error)?
        .with_redaction_logger(ArcLogger(Arc::clone(&counter) as Arc<dyn RedactionLogger>)),
        None if context.is_some() => build_context_pipeline(
            context.as_ref().expect("checked context"),
            Arc::clone(&counter) as Arc<dyn RedactionLogger>,
        )
        .map_err(|_| CliError::PolicyConfig)?,
        None => {
            tracing::warn!("gaze clean running with stub pipeline because --policy was omitted");
            build_stub_pipeline(Arc::clone(&counter) as Arc<dyn RedactionLogger>)
                .map_err(|_| CliError::PolicyConfig)?
        }
    };
    let pipeline = maybe_register_safety_net(pipeline, &options)?;

    let session = match effective_policy {
        Some(policy) => Session::from_policy_with_ttl_override(policy, options.session_ttl),
        None => Session::new(scope_for_cli_without_policy(
            clean_overrides.session_scope.as_ref(),
            options.session_ttl,
        )),
    }
    .map_err(|_| CliError::Pipeline)?;

    let detect_fields = match &context {
        Some(context) => &context.fields,
        None => empty_fields(),
    };
    let (clean_doc, leak_report) = if options.safety_net.is_some() {
        let (doc, _manifest, _report) = pipeline
            .clean_with_safety_net_detect_context(
                &session,
                RawDocument::Text(raw),
                locale_chain.as_slice(),
                &dictionaries,
                detect_fields,
            )
            .map_err(map_safety_net_pipeline_error)?;
        (doc, _report)
    } else {
        let doc = pipeline
            .redact_with_detect_context(
                &session,
                RawDocument::Text(raw),
                locale_chain.as_slice(),
                &dictionaries,
                detect_fields,
            )
            .map_err(|_| CliError::Pipeline)?;
        (doc, LeakReport::default())
    };
    counter
        .log_safety_net_report(&leak_report, &session, DocumentKind::Text)
        .map_err(|_| CliError::Pipeline)?;
    enforce_safety_net_mode(&leak_report, options.safety_net_mode)?;

    let clean_text = match clean_doc {
        gaze::CleanDocument::Text(text) => text,
        gaze::CleanDocument::Structured(_) => {
            unreachable!(
                "clean submits only RawDocument::Text; library cannot produce Structured output from Text input"
            )
        }
    };

    let snapshot: SensitiveSnapshot = session.export().map_err(|_| CliError::Pipeline)?;
    let session_blob = BASE64.encode(snapshot.into_bytes());

    let response = CleanResponse {
        clean_text,
        session_blob,
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
    println!("{json}");
    Ok(())
}

#[cfg(feature = "safety-net-openai")]
fn maybe_register_safety_net(
    pipeline: gaze::Pipeline,
    options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    use gaze_recognizers::safety_net::openai_filter::{
        OpenAiFilterSafetyNet, SubprocessOpenAiFilterConfig,
    };

    let Some(kind) = options.safety_net else {
        validate_no_openai_filter_options(options)?;
        return Ok(pipeline);
    };
    match kind {
        SafetyNetKind::OpenaiFilter => {
            let command = options.openai_filter_command.ok_or_else(|| {
                CliError::SafetyNetConfigDetail(
                    "--openai-filter-command is required for --safety-net=openai-filter"
                        .to_string(),
                )
            })?;
            let checkpoint = options.openai_filter_checkpoint.ok_or_else(|| {
                CliError::SafetyNetConfigDetail(
                    "--openai-filter-checkpoint is required for --safety-net=openai-filter"
                        .to_string(),
                )
            })?;
            let mut config = SubprocessOpenAiFilterConfig::new(command)
                .with_checkpoint_path(checkpoint)
                .with_timeout(Duration::from_millis(options.safety_net_timeout_ms))
                .with_max_input_bytes(options.safety_net_input_limit_bytes);
            if let Some(operating_point) = options.openai_filter_operating_point {
                let value = operating_point.as_opf_value();
                config = config
                    .with_args([
                        "--format",
                        "json",
                        "--output-mode",
                        "typed",
                        "--operating-point",
                        value,
                    ])
                    .with_decoding_param("operating_point", value);
            }
            Ok(pipeline.with_safety_net(OpenAiFilterSafetyNet::new(config)))
        }
    }
}

#[cfg(not(feature = "safety-net-openai"))]
fn maybe_register_safety_net(
    pipeline: gaze::Pipeline,
    options: &CleanOptions<'_>,
) -> std::result::Result<gaze::Pipeline, CliError> {
    if options.safety_net.is_some() {
        return Err(CliError::SafetyNetConfigDetail(
            "safety net requested but gaze-cli was not compiled with feature safety-net-openai"
                .to_string(),
        ));
    }
    validate_no_openai_filter_options(options)?;
    Ok(pipeline)
}

fn validate_no_openai_filter_options(
    options: &CleanOptions<'_>,
) -> std::result::Result<(), CliError> {
    if options.openai_filter_command.is_some()
        || options.openai_filter_checkpoint.is_some()
        || options.openai_filter_operating_point.is_some()
        || options.safety_net_timeout_ms != DEFAULT_SAFETY_NET_TIMEOUT_MS
        || options.safety_net_input_limit_bytes != DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES
    {
        return Err(CliError::SafetyNetConfigDetail(
            "openai-filter options require --safety-net=openai-filter".to_string(),
        ));
    }
    Ok(())
}

fn map_safety_net_pipeline_error(err: gaze::Error) -> CliError {
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
    }
}

fn clean_overrides_from_options(
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
    let rulepack_bundled = if options.rulepack_bundled.is_empty() {
        None
    } else {
        Some(options.rulepack_bundled.to_vec())
    };

    Ok(CleanOverrides {
        session_scope,
        ner_model_dir: options.ner_model_dir.clone(),
        ner_locale,
        rulepack_bundled,
        rulepack_paths: options.rulepack_paths.clone(),
    })
}

fn has_rulepack_overrides(options: &CleanOptions<'_>) -> bool {
    !options.rulepack_bundled.is_empty() || !options.rulepack_paths.is_empty()
}

fn policy_for_rulepack_overrides(
    clean_overrides: &CleanOverrides,
) -> std::result::Result<Policy, CliError> {
    let mut rules = class_rules_for_bundled_overrides(clean_overrides)?;
    rules.push(RuleSpec::Default {
        action: Action::Preserve,
    });
    let base = Policy {
        session: SessionPolicy {
            scope: SessionScope::Persistent,
            ttl_secs: Some(86_400),
        },
        detectors: Vec::new(),
        dictionaries: Vec::new(),
        rules,
        ner: None,
        rulepacks: RulepackPolicy {
            bundled: Vec::new(),
            paths: Vec::new(),
        },
        locale: None,
    };
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
        let contents = gaze_recognizers::embedded(bundle).ok_or(CliError::PolicyConfig)?;
        let rulepack = Rulepack::load(RulepackSource::Embedded(contents))
            .map_err(|_| CliError::PolicyConfig)?;
        classes.extend(rulepack.activated_classes());
    }
    Ok(classes
        .into_iter()
        .map(|class| RuleSpec::Class {
            class,
            action: Action::Tokenize,
        })
        .collect())
}

fn scope_for_cli_without_policy(scope: Option<&SessionScope>, ttl_secs: Option<u64>) -> Scope {
    match scope.unwrap_or(&SessionScope::Persistent) {
        SessionScope::Ephemeral => Scope::Ephemeral,
        SessionScope::Conversation => Scope::Conversation("cli".to_string()),
        SessionScope::Persistent => Scope::Persistent {
            ttl: Duration::from_secs(ttl_secs.unwrap_or(86_400)),
        },
    }
}

fn parse_cli_locales(raw: &[String]) -> std::result::Result<Option<Vec<LocaleTag>>, CliError> {
    if raw.is_empty() {
        return Ok(None);
    }
    raw.iter()
        .map(|locale| LocaleTag::parse(locale).map_err(|_| CliError::PolicyConfig))
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
            audit.log_safety_net(&entry)?;
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
    stats: Stats,
    leak_report: LeakReportResponse,
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
        }
    }
}

fn document_kind_label(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Structured => "structured",
        DocumentKind::Text => "text",
    }
}

fn enforce_safety_net_mode(
    report: &LeakReport,
    mode: SafetyNetMode,
) -> std::result::Result<(), CliError> {
    let suspected_leaks = report.stats.uncovered_count + report.stats.partial_bleed_count;
    if suspected_leaks > 0 {
        match mode {
            SafetyNetMode::Strict => {
                return Err(CliError::SafetyNetFailure {
                    variant: "SuspectedLeak",
                });
            }
            SafetyNetMode::Tolerant => emit_safety_net_warning("SuspectedLeak", suspected_leaks),
        }
    }
    if report.stats.class_mismatch_count > 0 {
        emit_safety_net_warning("ClassMismatch", report.stats.class_mismatch_count);
    }
    Ok(())
}

fn emit_safety_net_warning(variant: &'static str, count: usize) {
    eprintln!(r#"{{"warning":"SafetyNet","variant":"{variant}","count":{count}}}"#);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::build::resolve_ner_threshold;

    fn policy_with_ner_threshold(threshold: f32) -> Policy {
        Policy {
            session: gaze::SessionPolicy {
                scope: gaze::SessionScope::Persistent,
                ttl_secs: Some(86_400),
            },
            detectors: Vec::new(),
            dictionaries: Vec::new(),
            rules: vec![gaze::RuleSpec::Default {
                action: gaze::Action::Preserve,
            }],
            ner: Some(gaze::NerPolicy {
                model_dir: None,
                locale: None,
                threshold,
            }),
            rulepacks: gaze::RulepackPolicy {
                bundled: vec!["core".to_string()],
                paths: Vec::new(),
            },
            locale: None,
        }
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
