//! Gaze CLI — pipe-mode `clean` / `restore` for LLM-facing integrations.
//!
//! See `docs/roadmap/v0.3/cli.md` for the design spec and
//! `docs/roadmap/v0.3/laravel.md` for the host-side integration contract.

use std::io::{self, Read};
use std::ops::Range;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
mod error;

use clap::{Parser, Subcommand};
use regex::Regex;
use serde::{Deserialize, Serialize};

use gaze::{
    Action, ClassRule, ColumnRule, DefaultRule, DocumentKind, LocaleChain, LocaleTag, PiiClass,
    Pipeline, Policy, PolicyError, RawDocument, RawMatch, RedactionEntry, RedactionLogger,
    Result as GazeResult, RuleSpec, Rulepack, RulepackSource, Scope, SensitiveSnapshot, Session,
};
use gaze_recognizers::{NerDetector, NerOptions, NormalizerKind, RegexDetector, ValidatorKind};

use crate::error::{CliError, RestoreMode, RestoreWarning};

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

/// Default max-bytes cap for stdin. Keeps a runaway or attacker-controlled
/// upstream from OOM'ing the worker. Override with `--max-bytes`.
const DEFAULT_MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Subcommand, Debug)]
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
        /// Active locale fallback chain, comma separated and priority ordered.
        #[arg(long, value_delimiter = ',')]
        locale: Vec<String>,
        /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
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

/// Install a panic hook that prints a sanitized error line and exits 3.
/// Without this, a panic in `ort`, `regex`, or any other dep would leak a raw
/// backtrace to stderr whenever `RUST_BACKTRACE` is set — violating the
/// stderr discipline in docs/roadmap/v0.3/cli.md §"Stderr discipline".
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|_info| {
        eprintln!(r#"{{"error":"Pipeline","exit":3}}"#);
        // Force exit 3 so the host wrapper sees the documented code instead
        // of Rust's default 101. The hook runs BEFORE the runtime unwinds,
        // so `process::exit` here is the only way to guarantee both the
        // sanitized stderr line AND the contracted exit code.
        std::process::exit(3);
    }));
}

fn main() -> ExitCode {
    install_panic_hook();

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
            locale,
            max_bytes,
        } => run_clean(policy.as_deref(), &format, session_ttl, &locale, max_bytes),
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

/// Read stdin up to `max_bytes + 1` and return the bytes.
///
/// Reading one extra byte past the cap lets us distinguish "input exactly
/// at the limit" from "input exceeds the limit" without a second probe.
fn read_stdin_bytes(max_bytes: u64) -> std::result::Result<Vec<u8>, CliError> {
    let mut buf = Vec::new();
    let limit = max_bytes.saturating_add(1);
    io::stdin()
        .take(limit)
        .read_to_end(&mut buf)
        .map_err(|_| CliError::Io)?;
    if buf.len() as u64 > max_bytes {
        return Err(CliError::InputTooLarge);
    }
    Ok(buf)
}

/// Read stdin as UTF-8 text, enforcing the size cap. Distinguishes:
///   - 0 bytes              → `EmptyInput`     (exit 1)
///   - > max_bytes           → `InputTooLarge` (exit 1)
///   - non-UTF-8             → `InvalidEncoding` (exit 1)
///   - IO / OS error         → `Io`            (exit 4)
///
/// `clean` calls this; `restore` uses the bytes path directly since the
/// restore stdin is JSON and serde_json does its own UTF-8 validation.
fn read_stdin_text(max_bytes: u64) -> std::result::Result<String, CliError> {
    let bytes = read_stdin_bytes(max_bytes)?;
    if bytes.is_empty() {
        return Err(CliError::EmptyInput);
    }
    String::from_utf8(bytes).map_err(|_| CliError::InvalidEncoding)
}

fn require_json_format(format: &str) -> std::result::Result<(), CliError> {
    if format == "json" {
        Ok(())
    } else {
        Err(CliError::PolicyConfig)
    }
}

fn run_clean(
    policy: Option<&std::path::Path>,
    format: &str,
    session_ttl: Option<u64>,
    locale: &[String],
    max_bytes: u64,
) -> std::result::Result<(), CliError> {
    require_json_format(format)?;
    let raw = read_stdin_text(max_bytes)?;

    let counter = Arc::new(CountingLogger::default());
    let loaded_policy = match policy {
        Some(path) => Some(Policy::load_for_cli(path).map_err(map_policy_error)?),
        None => None,
    };
    let loaded_rulepacks = match &loaded_policy {
        Some(policy) => load_rulepacks(policy).map_err(map_pipeline_error)?,
        None => Vec::new(),
    };
    let rulepack_default_locales = merged_rulepack_default_locales(&loaded_rulepacks);
    let cli_locales = parse_cli_locales(locale)?;
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(
        cli_locales.as_deref(),
        loaded_policy
            .as_ref()
            .and_then(|policy| policy.locale.as_deref()),
        Some(&rulepack_default_locales),
    );

    let pipeline = match &loaded_policy {
        Some(policy) => build_pipeline_from_policy(policy, loaded_rulepacks)
            .map_err(map_pipeline_error)?
            .with_redaction_logger(ArcLogger(Arc::clone(&counter) as Arc<dyn RedactionLogger>)),
        None => {
            tracing::warn!("gaze clean running with stub pipeline because --policy was omitted");
            build_stub_pipeline(Arc::clone(&counter) as Arc<dyn RedactionLogger>)
                .map_err(|_| CliError::PolicyConfig)?
        }
    };

    let session = match &loaded_policy {
        Some(policy) => Session::from_policy_with_ttl_override(policy, session_ttl),
        None => Session::new(Scope::Persistent {
            ttl: Duration::from_secs(session_ttl.unwrap_or(86_400)),
        }),
    }
    .map_err(|_| CliError::Pipeline)?;

    let clean_doc = pipeline
        .redact_with_context(&session, RawDocument::Text(raw), locale_chain.as_slice())
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
        },
    };
    let json = serde_json::to_string(&response).map_err(|_| CliError::Pipeline)?;
    println!("{json}");
    Ok(())
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

fn build_pipeline_from_policy(policy: &Policy, rulepacks: Vec<Rulepack>) -> GazeResult<Pipeline> {
    let mut builder = Pipeline::builder();

    for detector in &policy.detectors {
        builder = match &detector.kind {
            gaze::DetectorKind::Regex => builder.detector(RegexDetector::with_source(
                &detector.pattern,
                detector.class.clone(),
                &detector.name,
            )?),
            gaze::DetectorKind::Unknown(kind) => {
                return Err(gaze::Error::Policy(PolicyError::BadTtl(format!(
                    "unknown detector.kind '{kind}'"
                ))))
            }
        };
    }

    let mut rulepack_recognizers =
        std::collections::BTreeMap::<String, (String, gaze::RecognizerSpec)>::new();
    for rulepack in rulepacks {
        for recognizer in rulepack.recognizers {
            tracing::info!(recognizer_id = %recognizer.id, "loaded rulepack recognizer");
            if let Some((first_pack, _)) = rulepack_recognizers.get(&recognizer.id) {
                return Err(gaze::Error::Rulepack(gaze::RulepackError::DuplicateId {
                    id: recognizer.id,
                    first_pack: first_pack.clone(),
                    second_pack: rulepack.rulepack_id.clone(),
                }));
            }
            rulepack_recognizers.insert(
                recognizer.id.clone(),
                (rulepack.rulepack_id.clone(), recognizer),
            );
        }
    }
    for (_, recognizer) in rulepack_recognizers
        .into_values()
        .filter(|(_, r)| r.enabled)
    {
        match recognizer.matcher {
            RawMatch::Regex { pattern } => {
                let exclusions = recognizer
                    .context
                    .as_ref()
                    .map(|context| context.exclusions.clone())
                    .unwrap_or_default();
                let validator_kind = recognizer
                    .validator
                    .as_ref()
                    .map(|validator| ValidatorKind::parse(&validator.kind))
                    .transpose()?;
                let normalizer_kind = recognizer
                    .normalizer
                    .as_ref()
                    .map(|normalizer| NormalizerKind::parse(&normalizer.kind))
                    .transpose()?;
                builder = builder.recognizer(RegexDetector::with_rulepack_fields(
                    &pattern,
                    recognizer.class,
                    &recognizer.id,
                    recognizer.locales,
                    recognizer.scoring.base,
                    recognizer.scoring.priority,
                    recognizer.token.family.as_deref().unwrap_or("counter"),
                    recognizer.token.format.as_deref().unwrap_or("{Class}_{n}"),
                    exclusions,
                    validator_kind,
                    normalizer_kind,
                )?);
            }
            RawMatch::Dictionary { .. } => {
                return Err(gaze::Error::Rulepack(
                    gaze::RulepackError::UnsupportedMatcher("Dictionary".to_string()),
                ))
            }
            RawMatch::Ner { .. } => {
                return Err(gaze::Error::Rulepack(
                    gaze::RulepackError::UnsupportedMatcher("Ner".to_string()),
                ))
            }
        }
    }

    for rule in &policy.rules {
        builder = match rule {
            RuleSpec::Class { class, action } => {
                builder.rule(ClassRule::new(class.clone(), *action))
            }
            RuleSpec::Column { column, action } => builder.rule(ColumnRule::new(column, *action)),
            RuleSpec::Default { action } => builder.rule(DefaultRule::new(*action)),
        };
    }

    if let Some(ner) = &policy.ner {
        if let Some(path) = &ner.model_dir {
            let detector = NerDetector::load_with_options(
                path,
                NerOptions {
                    locale: ner.locale.clone(),
                },
            )
            .map_err(|err| gaze::Error::Policy(PolicyError::NerLoad(err.to_string())))?;
            builder = builder.detector(detector);
        }
    }

    builder.build()
}

fn load_rulepacks(policy: &Policy) -> GazeResult<Vec<Rulepack>> {
    let mut rulepacks = Vec::new();
    for bundled in &policy.rulepacks.bundled {
        let contents = gaze_recognizers::embedded(bundled).ok_or_else(|| {
            gaze::Error::Policy(PolicyError::BadTtl(format!(
                "unknown bundled rulepack '{bundled}'"
            )))
        })?;
        rulepacks.push(Rulepack::load(RulepackSource::Embedded(contents))?);
    }
    for path in &policy.rulepacks.paths {
        rulepacks.push(Rulepack::load(RulepackSource::Path(path.clone()))?);
    }
    Ok(rulepacks)
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
/// `email1@example.test` wins over a substring match like `<Email_1>`. Bare
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
    for matched in gaze::token_shape::pattern().find_iter(text) {
        if is_inside_substitution_span(matched.start(), matched.end(), substitution_spans) {
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

fn is_inside_substitution_span(start: usize, end: usize, spans: &[Range<usize>]) -> bool {
    spans
        .iter()
        .any(|span| span.start <= start && end <= span.end)
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

#[derive(Default)]
struct CountingLogger {
    detections: AtomicU64,
}

impl RedactionLogger for CountingLogger {
    fn log(&self, entry: &RedactionEntry) -> GazeResult<()> {
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
}
