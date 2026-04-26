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
    Action, DictionaryBundle, DictionarySource, DocumentKind, LocaleTag, Policy, RawDocument,
    RedactionEntry, RedactionLogger, Result as GazeResult, RuleSpec, RulepackPolicy, Scope,
    SensitiveSnapshot, Session, SessionPolicy, SessionScope, SqliteLogger, TypedContext,
};

use crate::clean_overrides::CleanOverrides;
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
        Some(policy_for_rulepack_overrides(&clean_overrides))
    } else {
        None
    };
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
        .map(DictionaryBundle::from_context)
        .unwrap_or_default();
    let rulepack_dictionaries =
        dictionary_terms_from_rulepacks(&loaded_rulepacks).map_err(map_pipeline_error)?;
    let active_policy = loaded_policy.as_ref().or(cli_rulepack_policy.as_ref());
    let policy_bundle = active_policy
        .as_ref()
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
        active_policy.and_then(|policy| policy.locale.as_deref()),
        Some(&rulepack_default_locales),
    );
    let resolved_ner_threshold = resolve_ner_threshold(cli_ner_threshold, active_policy);

    let pipeline = match active_policy {
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

    let session = match active_policy {
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
    let clean_doc = pipeline
        .redact_with_detect_context(
            &session,
            RawDocument::Text(raw),
            locale_chain.as_slice(),
            &dictionaries,
            detect_fields,
        )
        .map_err(|_| CliError::Pipeline)?;

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
    };
    let json = serde_json::to_string(&response).map_err(|_| CliError::Pipeline)?;
    println!("{json}");
    Ok(())
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

fn policy_for_rulepack_overrides(clean_overrides: &CleanOverrides) -> Policy {
    let base = Policy {
        session: SessionPolicy {
            scope: SessionScope::Persistent,
            ttl_secs: Some(86_400),
        },
        detectors: Vec::new(),
        dictionaries: Vec::new(),
        rules: vec![RuleSpec::Default {
            action: Action::Tokenize,
        }],
        ner: None,
        rulepacks: RulepackPolicy {
            bundled: Vec::new(),
            paths: Vec::new(),
        },
        locale: None,
    };
    clean_overrides.apply_to(&base)
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
            audit: audit_db.map(SqliteLogger::new).transpose()?,
        })
    }
}

impl RedactionLogger for CountingLogger {
    fn log(&self, entry: &RedactionEntry) -> GazeResult<()> {
        if let Some(audit) = &self.audit {
            audit.log(entry)?;
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
