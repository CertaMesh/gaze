//! Gaze CLI — pipe-mode `clean` / `restore` for LLM-facing integrations.
//!
//! See `docs/roadmap/v0.3/cli.md` for the design spec and
//! `docs/roadmap/v0.3/laravel.md` for the host-side integration contract.

use std::ops::Range;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
mod clean_overrides;
mod error;
mod io;
mod logger;

use clap::{Parser, Subcommand};
use regex::Regex;
use serde::{Deserialize, Serialize};

use gaze::{
    Action, ClassRule, DefaultRule, DictionaryBundle, DictionarySource, DocumentKind, LocaleChain,
    LocaleTag, PiiClass, Pipeline, Policy, PolicyError, RawDocument, RawMatch, RedactionEntry,
    RedactionLogger, Result as GazeResult, RuleSpec, Rulepack, RulepackDict, RulepackPolicy,
    RulepackSource, Scope, SensitiveSnapshot, Session, SessionPolicy, SessionScope, SqliteLogger,
    TypedContext, DEFAULT_NER_THRESHOLD,
};
use gaze_recognizers::{DictionaryRecognizer, RegexDetector};

use crate::clean_overrides::CleanOverrides;
use crate::error::{CliError, RestoreMode, RestoreWarning};
use crate::io::{read_stdin_bytes, read_stdin_text, require_json_format, DEFAULT_MAX_BYTES};

#[derive(Parser, Debug)]
#[command(
    name = "gaze",
    version,
    about = "Channel-agnostic PII redaction for LLM pipes"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
enum Cmd {
    /// Read raw text from stdin; emit `{clean_text, session_blob, stats}` JSON to stdout.
    Clean {
        /// Path to policy.toml. Required once the policy loader lands (solo #3).
        #[arg(long)]
        policy: Option<PathBuf>,
        /// Output format. Only `json` is supported today.
        #[arg(long, default_value = "json")]
        format: String,
        /// Override the persistent session TTL in seconds.
        #[arg(long)]
        session_ttl: Option<u64>,
        /// Override policy [session].scope.
        #[arg(long)]
        session_scope: Option<String>,
        /// Active locale fallback chain, comma separated and priority ordered.
        #[arg(long, value_delimiter = ',')]
        locale: Vec<String>,
        /// Override policy [ner] threshold. Must be between 0.0 and 1.0 inclusive.
        #[arg(long)]
        ner_threshold: Option<f32>,
        /// Override policy [ner].model_dir.
        #[arg(long)]
        ner_model_dir: Option<PathBuf>,
        /// Override policy [ner].locale.
        #[arg(long)]
        ner_locale: Option<String>,
        /// Override policy.rulepacks.bundled. Comma-separated and repeatable.
        #[arg(long, value_delimiter = ',')]
        rulepack_bundled: Vec<String>,
        /// Override policy.rulepacks.paths. Repeatable.
        #[arg(long = "rulepack-path")]
        rulepack_paths: Vec<PathBuf>,
        /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
        /// Path to a typed Context JSON envelope. stdin remains raw text.
        #[arg(long)]
        context_json: Option<PathBuf>,
        /// Optional SQLite redaction-log database path.
        #[arg(long)]
        audit_db: Option<PathBuf>,
    },
    /// Read `{session_blob, text}` JSON from stdin; emit `{text}` JSON to stdout.
    Restore {
        /// Output format. Only `json` is supported today.
        #[arg(long, default_value = "json")]
        format: String,
        /// Unknown-token handling during restore.
        #[arg(long, value_enum, default_value_t = RestoreMode::Strict)]
        restore_mode: RestoreMode,
        /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
    },
}

fn main() -> ExitCode {
    logger::install_panic_hook();

    // Test-only panic trigger. Lets the integration suite prove the panic
    // hook sanitizes stderr under `RUST_BACKTRACE=1`. Gated by an env var
    // so no production invocation can stumble into it.
    if std::env::var_os("GAZE_TEST_PANIC").is_some() {
        panic!("gaze test-only panic trigger");
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // --help and --version are surfaced by clap as Err variants whose
            // intent is "print info to stdout and exit 0"; they are not argv
            // failures and must bypass the sanitizer so `gaze --version`
            // prints the crate version cleanly (required by the homebrew test
            // block).
            use clap::error::ErrorKind;
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = err.print();
                return ExitCode::SUCCESS;
            }
            // Real argv errors: clap's default handler would dump usage text
            // to stderr before our sanitizer runs. Route them through the
            // standard stderr line so the host wrapper can parse a variant
            // even on malformed argv.
            CliError::PolicyConfig.emit_stderr();
            return ExitCode::from(CliError::PolicyConfig.exit_code());
        }
    };

    let result = match cli.cmd {
        Cmd::Clean {
            policy,
            format,
            session_ttl,
            session_scope,
            locale,
            ner_threshold,
            ner_model_dir,
            ner_locale,
            rulepack_bundled,
            rulepack_paths,
            max_bytes,
            context_json,
            audit_db,
        } => run_clean(CleanOptions {
            policy: policy.as_deref(),
            format: &format,
            session_ttl,
            session_scope: session_scope.as_deref(),
            locale: &locale,
            ner_threshold,
            ner_model_dir,
            ner_locale: ner_locale.as_deref(),
            rulepack_bundled: &rulepack_bundled,
            rulepack_paths,
            max_bytes,
            context_json: context_json.as_deref(),
            audit_db: audit_db.as_deref(),
        }),
        Cmd::Restore {
            format,
            restore_mode,
            max_bytes,
        } => run_restore(&format, restore_mode, max_bytes),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            err.emit_stderr();
            ExitCode::from(err.exit_code())
        }
    }
}

struct CleanOptions<'a> {
    policy: Option<&'a std::path::Path>,
    format: &'a str,
    session_ttl: Option<u64>,
    session_scope: Option<&'a str>,
    locale: &'a [String],
    ner_threshold: Option<f32>,
    ner_model_dir: Option<PathBuf>,
    ner_locale: Option<&'a str>,
    rulepack_bundled: &'a [String],
    rulepack_paths: Vec<PathBuf>,
    max_bytes: u64,
    context_json: Option<&'a std::path::Path>,
    audit_db: Option<&'a std::path::Path>,
}

fn run_clean(options: CleanOptions<'_>) -> std::result::Result<(), CliError> {
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
    let policy_bundle = loaded_policy
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
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(
        cli_locales.as_deref(),
        loaded_policy
            .as_ref()
            .and_then(|policy| policy.locale.as_deref()),
        Some(&rulepack_default_locales),
    );
    let resolved_ner_threshold = resolve_ner_threshold(cli_ner_threshold, loaded_policy.as_ref());

    let pipeline = match &loaded_policy {
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

    let session = match &loaded_policy {
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
            action: Action::Preserve,
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

fn run_restore(
    format: &str,
    restore_mode: RestoreMode,
    max_bytes: u64,
) -> std::result::Result<(), CliError> {
    require_json_format(format)?;
    let stdin_bytes = read_stdin_bytes(max_bytes)?;

    let request: RestoreRequest =
        serde_json::from_slice(&stdin_bytes).map_err(|_| CliError::StdinParse)?;

    let blob_bytes = BASE64
        .decode(request.session_blob.as_bytes())
        .map_err(|_| CliError::StdinParse)?;

    let session =
        Session::import(SensitiveSnapshot::from(blob_bytes)).map_err(|err| match err {
            gaze::Error::InvalidSnapshotSignature => CliError::InvalidSignature,
            gaze::Error::InvalidSnapshotVersion(_) => CliError::InvalidBlobVersion,
            gaze::Error::InvalidSnapshotPayload => CliError::InvalidBlobVersion,
            gaze::Error::BlobExpired { .. } => CliError::BlobExpired,
            _ => CliError::Pipeline,
        })?;

    let pass1 = restore_pass1(&session, &request.text)?;
    let restore_warning = restore_pass2_validate(
        &pass1.text,
        &pass1.substitution_spans,
        &session,
        restore_mode,
    )?;

    let response = RestoreResponse {
        text: pass1.text,
        restore_warning,
    };
    let json = serde_json::to_string(&response).map_err(|_| CliError::Pipeline)?;
    println!("{json}");
    Ok(())
}

fn map_policy_error(err: PolicyError) -> CliError {
    match err {
        PolicyError::Io(_) => CliError::PolicyOpen,
        PolicyError::UnsupportedRuleKind(_) => {
            CliError::PolicyConfigDetail("column rules not supported in CLI mode")
        }
        _ => CliError::PolicyConfig,
    }
}

fn map_pipeline_error(err: gaze::Error) -> CliError {
    match err {
        gaze::Error::Policy(policy_err) => map_policy_error(policy_err),
        gaze::Error::Rulepack(_) => CliError::PolicyConfig,
        _ => CliError::Pipeline,
    }
}

fn build_pipeline_from_policy(
    policy: &Policy,
    rulepacks: &[Rulepack],
    context: Option<&TypedContext>,
    locale_chain: &LocaleChain,
    ner_threshold: f32,
) -> GazeResult<Pipeline> {
    let empty_context = TypedContext {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    };
    gaze_assembly::build_pipeline(
        policy,
        context.unwrap_or(&empty_context),
        rulepacks,
        locale_chain,
        Some(ner_threshold),
    )
    .map_err(|err| match err {
        gaze_assembly::BuildError::Policy(err) => gaze::Error::Policy(err),
        gaze_assembly::BuildError::Rulepack(err) => gaze::Error::Rulepack(err),
        gaze_assembly::BuildError::Pipeline(err) => err,
    })
}

fn validate_ner_threshold(threshold: f32) -> std::result::Result<f32, PolicyError> {
    if (0.0..=1.0).contains(&threshold) {
        Ok(threshold)
    } else {
        Err(PolicyError::NerThresholdOutOfRange { value: threshold })
    }
}

fn resolve_ner_threshold(cli_threshold: Option<f32>, policy: Option<&Policy>) -> f32 {
    cli_threshold
        .or_else(|| policy.and_then(|policy| policy.ner.as_ref().map(|ner| ner.threshold)))
        .unwrap_or(DEFAULT_NER_THRESHOLD)
}

fn load_rulepacks(policy: &Policy) -> GazeResult<Vec<Rulepack>> {
    let mut rulepacks = Vec::new();
    for bundled in &policy.rulepacks.bundled {
        let contents = load_embedded_rulepack_contents(bundled)?;
        rulepacks.push(Rulepack::load(RulepackSource::Embedded(contents))?);
    }
    for path in &policy.rulepacks.paths {
        rulepacks.push(Rulepack::load(RulepackSource::Path(path.clone()))?);
    }
    Ok(rulepacks)
}

fn load_embedded_rulepack_contents(id: &str) -> GazeResult<&'static str> {
    gaze_recognizers::embedded(id).ok_or_else(|| {
        gaze::Error::Policy(PolicyError::BundledRulepackUnknown {
            value: id.to_string(),
        })
    })
}

fn dictionary_terms_from_rulepacks(rulepacks: &[Rulepack]) -> GazeResult<Vec<RulepackDict>> {
    let mut dictionaries = Vec::new();
    for rulepack in rulepacks {
        for recognizer in &rulepack.recognizers {
            let RawMatch::Dictionary {
                terms,
                terms_file,
                terms_from_context,
                case_sensitive,
            } = &recognizer.matcher
            else {
                continue;
            };
            if terms_from_context.is_some() {
                continue;
            }
            let mut all_terms = terms.clone();
            if let Some(path) = terms_file {
                let file = std::fs::read_to_string(path).map_err(|err| {
                    gaze::Error::Policy(PolicyError::BadDictionary {
                        name: recognizer.id.clone(),
                        reason: format!("failed to read terms_file: {err}"),
                    })
                })?;
                all_terms.extend(
                    file.lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .map(str::to_string),
                );
            }
            if all_terms.is_empty() {
                return Err(gaze::Error::Policy(PolicyError::BadDictionary {
                    name: recognizer.id.clone(),
                    reason: "dictionary matcher requires terms, terms_file, or terms_from_context"
                        .to_string(),
                }));
            }
            if !case_sensitive && all_terms.iter().any(|term| !term.is_ascii()) {
                return Err(gaze::Error::Policy(PolicyError::BadDictionary {
                    name: recognizer.id.clone(),
                    reason:
                        "unicode dictionary insensitive matching unsupported in v0.4.0, use case_sensitive = true"
                            .to_string(),
                }));
            }
            dictionaries.push(RulepackDict {
                name: recognizer.id.clone(),
                terms: all_terms,
                case_sensitive: *case_sensitive,
            });
        }
    }
    Ok(dictionaries)
}

fn empty_fields() -> &'static serde_json::Map<String, serde_json::Value> {
    static EMPTY_FIELDS: OnceLock<serde_json::Map<String, serde_json::Value>> = OnceLock::new();
    EMPTY_FIELDS.get_or_init(serde_json::Map::new)
}

fn merged_rulepack_default_locales(rulepacks: &[Rulepack]) -> Vec<LocaleTag> {
    let mut locales = Vec::new();
    for rulepack in rulepacks {
        for locale in &rulepack.default_locales {
            if !locales.iter().any(|existing| existing == locale) {
                locales.push(locale.clone());
            }
        }
    }
    locales
}

/// Pass 1 — exact-literal alternation built from `session.tokens()`.
///
/// Sorts tokens longest-first so a format-preserved email like
/// `email1.<session>@gaze-fake.invalid` wins over a substring match like `<Email_1>`. Bare
/// format-preserving tokens stay wrapped in `\b` word boundaries so a token
/// cannot be swallowed inside an adjacent identifier (the
/// `hostName_1s-record` regression in `docs/roadmap/v0.3/cli.md` §"Test
/// strategy" #5). Wrapped counter tokens intentionally skip `\b`: `<` and
/// `>` are explicit delimiters, and `\b` would miss `See <Email_1>.` because
/// it does not fire across non-word characters. Empty session map is a no-op:
/// `Regex::new("")` would match everywhere, so short-circuit.
struct RestorePass1 {
    text: String,
    substitution_spans: Vec<Range<usize>>,
}

fn restore_pass1(session: &Session, text: &str) -> std::result::Result<RestorePass1, CliError> {
    let mut tokens = session.tokens();
    if tokens.is_empty() {
        return Ok(RestorePass1 {
            text: text.to_string(),
            substitution_spans: Vec::new(),
        });
    }
    tokens.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    let pattern = tokens
        .iter()
        .map(|token| {
            let escaped = regex::escape(token);
            if token.starts_with('<') && token.ends_with('>') {
                escaped
            } else {
                format!(r"\b(?:{escaped})\b")
            }
        })
        .collect::<Vec<_>>()
        .join("|");
    let re = Regex::new(&pattern).map_err(|_| CliError::Pipeline)?;

    let mut out = String::with_capacity(text.len());
    let mut substitution_spans = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(text) {
        out.push_str(&text[last..m.start()]);
        let real = session
            .restore_strict(m.as_str())
            .map_err(|_| CliError::Pipeline)?;
        let substitution_start = out.len();
        out.push_str(&real);
        substitution_spans.push(substitution_start..out.len());
        last = m.end();
    }
    out.push_str(&text[last..]);
    Ok(RestorePass1 {
        text: out,
        substitution_spans,
    })
}

/// Pass 2 — shape-validator over Pass-1 output.
///
/// Any remaining token-shaped substring means the LLM invented a token the
/// session never emitted → `UnknownToken`. The canonical grammar lives in
/// `gaze::token_shape`, so the CLI no longer re-encodes token shapes locally.
fn restore_pass2_validate(
    text: &str,
    substitution_spans: &[Range<usize>],
    session: &Session,
    mode: RestoreMode,
) -> std::result::Result<Vec<RestoreWarning>, CliError> {
    let mut warnings = Vec::new();
    let mut substitution_cursor = 0usize;
    for matched in gaze::token_shape::pattern().find_iter(text) {
        if is_inside_substitution_span(
            matched.start(),
            matched.end(),
            substitution_spans,
            &mut substitution_cursor,
        ) {
            continue;
        }
        let matched_text = matched.as_str();
        if gaze::token_shape::is_trap(matched_text) {
            match mode {
                RestoreMode::Strict => {
                    return Err(CliError::UnknownToken {
                        token: matched_text.to_string(),
                    })
                }
                RestoreMode::Tolerant => warnings.push(RestoreWarning {
                    variant: "UnknownToken".to_string(),
                    token: matched_text.to_string(),
                }),
            }
            continue;
        }
        if session.contains_token(matched_text) {
            continue;
        }
        match mode {
            RestoreMode::Strict => {
                return Err(CliError::UnknownToken {
                    token: matched_text.to_string(),
                })
            }
            RestoreMode::Tolerant => warnings.push(RestoreWarning {
                variant: "UnknownToken".to_string(),
                token: matched_text.to_string(),
            }),
        }
    }
    Ok(warnings)
}

fn is_inside_substitution_span(
    start: usize,
    end: usize,
    spans: &[Range<usize>],
    cursor: &mut usize,
) -> bool {
    while spans.get(*cursor).is_some_and(|span| span.end <= start) {
        *cursor += 1;
    }

    spans
        .get(*cursor)
        .is_some_and(|span| span.start <= start && end <= span.end)
}

#[derive(Deserialize)]
struct RestoreRequest {
    session_blob: String,
    text: String,
}

#[derive(Serialize)]
struct RestoreResponse {
    text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    restore_warning: Vec<RestoreWarning>,
}

/// Stub pipeline used until the policy.toml loader (solo #3) lands.
/// Ships only a regex email detector + tokenize rule so the CLI contract can
/// be exercised end-to-end; richer detectors arrive with the loader.
fn build_stub_pipeline(logger: Arc<dyn RedactionLogger>) -> GazeResult<Pipeline> {
    Pipeline::builder()
        .detector(RegexDetector::emails()?)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(ArcLogger(logger))
        .build()
}

fn build_context_pipeline(
    context: &TypedContext,
    logger: Arc<dyn RedactionLogger>,
) -> GazeResult<Pipeline> {
    let mut builder = Pipeline::builder();
    for name in context.dictionaries.keys() {
        let class = context
            .class_map
            .get(name)
            .cloned()
            .unwrap_or_else(|| PiiClass::custom(name));
        builder = builder
            .recognizer(DictionaryRecognizer::new(
                format!("context/{name}"),
                class.clone(),
                name,
                context.dictionaries[name].case_sensitive,
                "counter",
            ))
            .rule(ClassRule::new(class, Action::Tokenize));
    }
    builder
        .rule(DefaultRule::new(Action::Preserve))
        .redaction_logger(ArcLogger(logger))
        .build()
}

struct CountingLogger {
    detections: AtomicU64,
    audit: Option<SqliteLogger>,
}

impl CountingLogger {
    fn new(audit_db: Option<&std::path::Path>) -> GazeResult<Self> {
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

/// Adapter that lets `PipelineBuilder::redaction_logger` (which takes ownership
/// of a concrete `RedactionLogger`) accept a shared `Arc<dyn RedactionLogger>`.
/// The Arc keeps the handle alive for post-redact counter inspection.
struct ArcLogger(Arc<dyn RedactionLogger>);

impl RedactionLogger for ArcLogger {
    fn log(&self, entry: &RedactionEntry) -> GazeResult<()> {
        self.0.log(entry)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

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
        assert_eq!(resolve_ner_threshold(None, None), DEFAULT_NER_THRESHOLD);
    }
    fn empty_session() -> Session {
        Session::new(Scope::Ephemeral).expect("session")
    }

    #[test]
    fn pass2_cursor_scan_handles_dense_substitution_spans() {
        let spans = (0..1_000)
            .map(|i| {
                let start = i * 16;
                start..start + 8
            })
            .collect::<Vec<_>>();
        let mut cursor = 0usize;
        let started = Instant::now();

        for (expected_cursor, span) in spans.iter().enumerate() {
            assert!(
                is_inside_substitution_span(span.start, span.end, &spans, &mut cursor),
                "span {expected_cursor} should be exempt"
            );
            assert_eq!(
                cursor, expected_cursor,
                "cursor must advance monotonically to the current span"
            );
        }

        assert!(
            started.elapsed() < Duration::from_millis(50),
            "dense cursor scan exceeded the regression budget"
        );
    }

    #[test]
    fn pass2_cursor_scan_handles_token_shaped_text_inside_substituted_span() {
        let session = empty_session();
        let text = "Order_42";
        let spans = std::iter::once(0..text.len()).collect::<Vec<_>>();
        let mut cursor = 0usize;

        assert!(is_inside_substitution_span(
            0,
            text.len(),
            &spans,
            &mut cursor
        ));
        assert_eq!(cursor, 0, "cursor stays on the containing span");
        let warnings = restore_pass2_validate(text, &spans, &session, RestoreMode::Strict).unwrap();

        assert!(warnings.is_empty());
    }

    #[test]
    fn pass2_cursor_scan_traps_adjacent_hallucinated_tokens_outside_substituted_span() {
        let session = empty_session();
        let text = "Alice_1<Email_999>";
        let spans = std::iter::once(0..7).collect::<Vec<_>>();
        let mut cursor = 0usize;

        assert!(is_inside_substitution_span(0, 7, &spans, &mut cursor));
        assert_eq!(cursor, 0);
        assert!(!is_inside_substitution_span(
            7,
            text.len(),
            &spans,
            &mut cursor
        ));
        assert_eq!(
            cursor, 1,
            "cursor must advance past the adjacent completed substitution span"
        );
        match restore_pass2_validate(text, &spans, &session, RestoreMode::Strict) {
            Err(CliError::UnknownToken { token }) => assert_eq!(token, "<Email_999>"),
            Err(other) => panic!("unexpected error: {other:?}"),
            Ok(_) => panic!("expected adjacent hallucinated token to fail"),
        }
    }

    #[test]
    fn pass2_cursor_scan_tolerant_mode_reports_first_unknown_token() {
        let session = empty_session();
        let text = "Alice_1 <Email_999> <Name_100>";
        let spans = std::iter::once(0..7).collect::<Vec<_>>();
        let mut cursor = 0usize;

        assert!(is_inside_substitution_span(0, 7, &spans, &mut cursor));
        assert!(!is_inside_substitution_span(8, 19, &spans, &mut cursor));
        assert_eq!(
            cursor, 1,
            "cursor remains advanced after leaving the substitution span"
        );
        let warnings =
            restore_pass2_validate(text, &spans, &session, RestoreMode::Tolerant).unwrap();

        assert_eq!(warnings[0].variant, "UnknownToken");
        assert_eq!(warnings[0].token, "<Email_999>");
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
