use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::shared_args::{
    KijiPrecisionArgs, OpenAiFilterSubprocessArgs, OpfRegistryArgs, RulepackOverrideArgs,
    SafetyNetLimitArgs, SafetyNetRegistryArgs,
};
use super::{
    KijiBackend, OpenAiFilterDevice, SafetyNetBackend, SafetyNetFallback, SafetyNetKind,
    SafetyNetMode,
};
use crate::error::CliError;
use crate::io::DEFAULT_MAX_BYTES;
use crate::pipeline::build::{map_policy_error, resolve_pipeline, validate_ner_threshold};
use crate::pipeline::run::{
    clean_overrides_from_options, enforce_safety_net_mode, entry_class_to_string,
    map_safety_net_pipeline_error, maybe_register_safety_net, safety_net_policy,
    validate_safety_net_tolerant_gate, CleanOptions,
};
use gaze::{
    Action, ConflictTier, DictionaryBundle, DocumentKind, EmittedTokenSpan, LeakReport, LocaleTag,
    PiiClass, Policy, RawDocument, RedactionEntry, RedactionLogError, RedactionLogger,
    Result as GazeResult, Session, SessionSnapshotEntry,
};
use gaze_audit::{LeakSuspectLogEntry, LeakSuspectLogger, SqliteLogger};

const DEFAULT_PROCESS_IDLE_TIMEOUT_SECS: u64 = 1_800;
const DEFAULT_SESSION_IDLE_TIMEOUT_SECS: u64 = 3_600;
const DEFAULT_SESSION_CAP: usize = 1_000;

/// The complete `gaze daemon` flag surface.
///
/// Declared once and flattened into `Cmd::Daemon`. The safety-net groups it
/// shares with `gaze clean` come from [`super::shared_args`], so the two verbs
/// cannot drift apart on those flags without a compile error.
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Path to policy.toml.
    #[arg(long)]
    pub(crate) policy: PathBuf,
    /// Optional observer-only privacy safety net.
    #[arg(long, value_enum)]
    pub(crate) safety_net: Option<SafetyNetKind>,
    /// v0.8 backend selector. When set with `--safety-net=<kind>`, this flag wins.
    #[arg(long, value_enum)]
    pub(crate) safety_net_backend: Option<SafetyNetBackend>,
    #[command(flatten)]
    pub(crate) safety_net_registry: SafetyNetRegistryArgs,
    /// Exit when stdin is idle for this many seconds.
    #[arg(long, default_value_t = default_process_idle_timeout_secs())]
    pub(crate) idle_timeout: u64,
    /// Evict sessions idle for this many seconds.
    #[arg(long, default_value_t = default_session_idle_timeout_secs())]
    pub(crate) session_idle_timeout: u64,
    /// Maximum live sessions before LRU eviction.
    #[arg(long, default_value_t = default_session_cap())]
    pub(crate) session_cap: usize,
    /// Optional SQLite redaction-log database path.
    #[arg(long)]
    pub(crate) audit_db: Option<PathBuf>,
    /// Active locale fallback chain, comma separated and priority ordered.
    #[arg(long, value_delimiter = ',')]
    pub(crate) locale: Vec<String>,
    /// Override policy \[ner\] threshold. Must be between 0.0 and 1.0 inclusive.
    #[arg(long)]
    pub(crate) ner_threshold: Option<f32>,
    /// Override policy \[ner\].model_dir.
    #[arg(long)]
    pub(crate) ner_model_dir: Option<PathBuf>,
    /// Override policy \[ner\].locale.
    #[arg(long)]
    pub(crate) ner_locale: Option<String>,
    #[command(flatten)]
    pub(crate) rulepacks: RulepackOverrideArgs,
    #[command(flatten)]
    pub(crate) openai_filter: OpenAiFilterSubprocessArgs,
    /// Device selection for the OpenAI safety-net subprocess.
    #[arg(long, value_enum, default_value_t = OpenAiFilterDevice::Auto)]
    pub(crate) openai_filter_device: OpenAiFilterDevice,
    /// Kiji DistilBERT runtime backend.
    #[arg(long, value_enum, default_value_t = KijiBackend::Subprocess)]
    pub(crate) kiji_backend: KijiBackend,
    #[command(flatten)]
    pub(crate) kiji_precision: KijiPrecisionArgs,
    #[command(flatten)]
    pub(crate) opf_registry: OpfRegistryArgs,
    /// Path to the local Kiji DistilBERT subprocess command.
    #[arg(long)]
    pub(crate) kiji_distilbert_command: Option<PathBuf>,
    /// Path to the pinned Kiji DistilBERT model directory.
    #[arg(long)]
    pub(crate) kiji_distilbert_model_dir: Option<PathBuf>,
    /// Locale list for the Kiji DistilBERT backend.
    #[arg(long, value_delimiter = ',')]
    pub(crate) kiji_distilbert_locales: Vec<String>,
    #[command(flatten)]
    pub(crate) safety_net_limits: SafetyNetLimitArgs,
}

pub(crate) fn run(args: Args) -> std::result::Result<(), CliError> {
    let shutdown = install_signal_flags()?;
    let mut daemon = Daemon::new(args)?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let idle_timeout = daemon.process_idle_timeout;
    let mut stdout = io::stdout().lock();
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                daemon.last_stdin_activity = Instant::now();
                daemon.evict_idle_sessions();
                let response = daemon.handle_line(&line);
                write_json_line(&mut stdout, &response)?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                daemon.evict_idle_sessions();
                if daemon.last_stdin_activity.elapsed() >= idle_timeout {
                    tracing::info!("gaze daemon exiting after stdin idle timeout");
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    stdout.flush().map_err(|_| CliError::Io)?;
    Ok(())
}

fn install_signal_flags() -> std::result::Result<Arc<AtomicBool>, CliError> {
    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown))
        .map_err(|_| CliError::Io)?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))
        .map_err(|_| CliError::Io)?;
    Ok(shutdown)
}

fn write_json_line<W: Write>(writer: &mut W, response: &DaemonResponse) -> Result<(), CliError> {
    serde_json::to_writer(&mut *writer, response).map_err(|_| CliError::Pipeline)?;
    writer.write_all(b"\n").map_err(|_| CliError::Io)?;
    writer.flush().map_err(|_| CliError::Io)
}

struct Daemon {
    pipeline: gaze::Pipeline,
    policy: Policy,
    locale_chain: Vec<LocaleTag>,
    dictionaries: DictionaryBundle,
    safety_net_active: bool,
    safety_net_mode: SafetyNetMode,
    safety_net_fallback: SafetyNetFallback,
    logger: Arc<DaemonLogger>,
    sessions: HashMap<String, SessionEntry>,
    lru: VecDeque<String>,
    session_cap: usize,
    session_idle_timeout: Duration,
    process_idle_timeout: Duration,
    last_stdin_activity: Instant,
}

impl Daemon {
    fn new(args: Args) -> std::result::Result<Self, CliError> {
        if args.session_cap == 0 {
            return Err(CliError::PolicyConfigDetail(
                "--session-cap must be greater than zero".to_string(),
            ));
        }
        validate_safety_net_tolerant_gate(
            args.safety_net_limits.safety_net_mode,
            args.safety_net_limits.safety_net_fallback,
        )?;
        let logger =
            Arc::new(DaemonLogger::new(args.audit_db.as_deref()).map_err(|_| CliError::Pipeline)?);
        let options = clean_options(&args);
        let cli_ner_threshold = args
            .ner_threshold
            .map(validate_ner_threshold)
            .transpose()
            .map_err(map_policy_error)?;
        let clean_overrides = clean_overrides_from_options(&options)?;
        let resolved = resolve_pipeline(
            Some(&args.policy),
            &clean_overrides,
            &args.locale,
            cli_ner_threshold,
            None,
            Some(Arc::clone(&logger) as Arc<dyn RedactionLogger>),
        )?;
        let loaded_policy = resolved.policy.expect("daemon requires a policy path");
        let locale_chain = resolved.locale_chain;
        let dictionaries = resolved.dictionaries;
        let pipeline = maybe_register_safety_net(resolved.pipeline, &options)?;
        Ok(Self {
            pipeline,
            policy: loaded_policy,
            locale_chain: locale_chain.as_slice().to_vec(),
            dictionaries,
            // Mirrors `run_clean`: the registry activates the Pass-3 safety net exactly
            // as `--safety-net` does, so a registry-only daemon must map safety-net
            // failures to their typed variant instead of a generic pipeline error.
            safety_net_active: args.safety_net.is_some()
                || args.safety_net_registry.safety_net_registry,
            safety_net_mode: args.safety_net_limits.safety_net_mode,
            safety_net_fallback: args.safety_net_limits.safety_net_fallback,
            logger,
            sessions: HashMap::new(),
            lru: VecDeque::new(),
            session_cap: args.session_cap,
            session_idle_timeout: Duration::from_secs(args.session_idle_timeout),
            process_idle_timeout: Duration::from_secs(args.idle_timeout),
            last_stdin_activity: Instant::now(),
        })
    }

    fn handle_line(&mut self, line: &str) -> DaemonResponse {
        let request = match serde_json::from_str::<DaemonRequest>(line) {
            Ok(request) if !request.session_id.trim().is_empty() => request,
            Ok(_) => return DaemonResponse::error(None, "ProtocolInvalid", "missing session_id"),
            Err(_) => return DaemonResponse::error(None, "JsonMalformed", "malformed JSON line"),
        };
        match self.clean_request(request) {
            Ok(response) => response,
            Err((session_id, error)) => DaemonResponse::error(
                Some(session_id),
                error.variant(),
                error.detail().unwrap_or("clean request failed"),
            ),
        }
    }

    fn clean_request(
        &mut self,
        request: DaemonRequest,
    ) -> std::result::Result<DaemonResponse, (String, DaemonError)> {
        self.ensure_session(&request.session_id)
            .map_err(|err| (request.session_id.clone(), err))?;
        let session = &self
            .sessions
            .get(&request.session_id)
            .expect("session inserted")
            .session;
        let policy = safety_net_policy(self.safety_net_mode, self.safety_net_fallback);
        let (clean_doc, manifest, leak_report) = self
            .pipeline
            .clean_with_safety_net_policy_detect_context(
                session,
                RawDocument::Text(request.text),
                &self.locale_chain,
                &self.dictionaries,
                policy,
            )
            .map_err(|err| {
                let cli_error = if self.safety_net_active {
                    map_safety_net_pipeline_error(err)
                } else {
                    CliError::Pipeline
                };
                (request.session_id.clone(), DaemonError::Cli(cli_error))
            })?;
        self.logger
            .log_safety_net_report(&leak_report, session, DocumentKind::Text)
            .map_err(|_| {
                (
                    request.session_id.clone(),
                    DaemonError::Cli(CliError::Pipeline),
                )
            })?;
        enforce_safety_net_mode(&leak_report, policy)
            .map_err(|err| (request.session_id.clone(), DaemonError::Cli(err)))?;
        let clean_text = match clean_doc {
            gaze::CleanDocument::Text(text) => text,
            _ => return Err((request.session_id, DaemonError::Invariant)),
        };
        let tokens = session
            .snapshot_entries()
            .into_iter()
            .map(TokenJson::from)
            .collect();
        Ok(DaemonResponse::Clean {
            session_id: request.session_id,
            clean_text,
            manifest,
            tokens,
        })
    }

    fn ensure_session(&mut self, session_id: &str) -> std::result::Result<(), DaemonError> {
        self.evict_idle_sessions();
        if let Some(entry) = self.sessions.get_mut(session_id) {
            entry.last_seen = Instant::now();
            self.lru.retain(|existing| existing != session_id);
            self.lru.push_back(session_id.to_string());
            return Ok(());
        }
        while self.sessions.len() >= self.session_cap {
            if let Some(evicted) = self.lru.pop_front() {
                if let Some(entry) = self.sessions.remove(&evicted) {
                    self.log_eviction(&evicted, &entry, "lru");
                }
            } else {
                break;
            }
        }
        let session = Session::from_policy(&self.policy).map_err(|_| DaemonError::Invariant)?;
        self.sessions.insert(
            session_id.to_string(),
            SessionEntry {
                session,
                last_seen: Instant::now(),
            },
        );
        self.lru.push_back(session_id.to_string());
        Ok(())
    }

    fn evict_idle_sessions(&mut self) {
        let now = Instant::now();
        let expired = self
            .sessions
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.last_seen) >= self.session_idle_timeout)
            .map(|(session_id, _)| session_id.clone())
            .collect::<Vec<_>>();
        for session_id in expired {
            if let Some(entry) = self.sessions.remove(&session_id) {
                self.lru.retain(|existing| existing != &session_id);
                self.log_eviction(&session_id, &entry, "idle_timeout");
            }
        }
    }

    fn log_eviction(&self, session_id: &str, entry: &SessionEntry, reason: &str) {
        tracing::warn!(session_id = %session_id, reason = %reason, "gaze daemon evicted session");
        let _ = self.logger.log_eviction(&entry.session, reason);
    }
}

/// `CleanOptions::format` is read only by `require_json_format` inside `run_clean`.
/// The daemon never calls `run_clean`: its wire format is the JSONL protocol written
/// by `write_json_line` and is not selectable, which is why there is no `--format` on
/// `gaze daemon`. The field is inert here, and set to the encoding the daemon does in
/// fact emit rather than to an arbitrary string.
const DAEMON_CLEAN_FORMAT: &str = "json";

/// `CleanOptions::max_bytes` bounds `read_stdin_text`, which only `run_clean` calls.
/// The daemon reads its own newline-delimited frames from stdin and never goes through
/// that path, so the field is inert; bounding one daemon request is a protocol concern
/// needing a typed over-limit response, which is not smuggled in here. It carries
/// `clean`'s default rather than `u64::MAX` so that if a later change ever does route
/// the daemon through this field it inherits a bound instead of an unbounded read.
const DAEMON_CLEAN_MAX_BYTES: u64 = DEFAULT_MAX_BYTES;

fn clean_options(args: &Args) -> CleanOptions<'_> {
    CleanOptions {
        policy: Some(args.policy.as_path()),
        format: DAEMON_CLEAN_FORMAT,
        // Both are `[session]` keys in policy.toml, which is mandatory on `gaze daemon`,
        // so `None` ("no CLI override") still leaves the operator able to set them.
        //
        // `session_ttl` additionally has no live consumer on this path: only `run_clean`
        // passes it to `Session::from_policy_with_ttl_override`, while `ensure_session`
        // builds every session with `Session::from_policy`, which supplies `None`. Adding
        // a `--session-ttl` flag and wiring it only here would therefore be accepted and
        // then silently ignored -- strictly worse than not offering it. Closing that gap
        // means threading the override into `ensure_session`, not adding a flag.
        //
        // `--session-ttl` would also read as governing the daemon's own session registry,
        // which `--session-idle-timeout` and `--session-cap` own instead.
        session_ttl: None,
        session_scope: None,
        locale: &args.locale,
        ner_threshold: args.ner_threshold,
        ner_model_dir: args.ner_model_dir.clone(),
        ner_locale: args.ner_locale.as_deref(),
        rulepack_bundled: &args.rulepacks.rulepack_bundled,
        rulepack_paths: args.rulepacks.rulepack_paths.clone(),
        max_bytes: DAEMON_CLEAN_MAX_BYTES,
        // A typed context envelope describes one document. The daemon serves many
        // documents across many sessions from one process, so a process-wide
        // `--context-json` would apply one document's context to every session's every
        // request. That belongs in the per-request protocol frame, not in a flag.
        context_json: None,
        audit_db: args.audit_db.as_deref(),
        safety_net: args.safety_net,
        safety_net_backend: args.safety_net_backend,
        safety_net_registry: args.safety_net_registry.safety_net_registry,
        safety_net_add: &args.safety_net_registry.safety_net_add,
        openai_filter_command: args.openai_filter.openai_filter_command.as_deref(),
        openai_filter_checkpoint: args.openai_filter.openai_filter_checkpoint.as_deref(),
        openai_filter_operating_point: args.openai_filter.openai_filter_operating_point,
        openai_filter_device: args.openai_filter_device,
        kiji_backend: args.kiji_backend,
        kiji_distilbert_precision: args.kiji_precision.kiji_distilbert_precision,
        opf_locales: &args.opf_registry.opf_locales,
        opf_command: args.opf_registry.opf_command.as_deref(),
        opf_checkpoint: args.opf_registry.opf_checkpoint.as_deref(),
        kiji_distilbert_command: args.kiji_distilbert_command.as_deref(),
        kiji_distilbert_model_dir: args.kiji_distilbert_model_dir.as_deref(),
        kiji_distilbert_locales: &args.kiji_distilbert_locales,
        safety_net_timeout_ms: args.safety_net_limits.safety_net_timeout_ms,
        safety_net_input_limit_bytes: args.safety_net_limits.safety_net_input_limit_bytes,
        safety_net_mode: args.safety_net_limits.safety_net_mode,
        safety_net_fallback: args.safety_net_limits.safety_net_fallback,
    }
}

struct SessionEntry {
    session: Session,
    last_seen: Instant,
}

struct DaemonLogger {
    detections: AtomicU64,
    audit: Option<SqliteLogger>,
}

impl DaemonLogger {
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

    fn log_eviction(&self, session: &Session, reason: &str) -> Result<(), RedactionLogError> {
        let entry = RedactionEntry::new(
            "daemon.session_eviction",
            PiiClass::Custom("daemon_session".to_string()),
            Action::Preserve,
            Some(reason.to_string()),
            DocumentKind::Text,
            false,
            ConflictTier::None,
            chrono::Utc::now().timestamp_millis(),
            Some(session.audit_session_id().to_string()),
        )
        .with_provenance_metadata(
            Some("daemon".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("custom:daemon_session".to_string()),
            Some("custom:daemon_session".to_string()),
            None,
            None,
        );
        self.log(&entry)
    }
}

impl RedactionLogger for DaemonLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        let mut entry = entry.clone();
        entry.provenance_stage = Some("daemon".to_string());
        if let Some(audit) = &self.audit {
            audit
                .log(&entry)
                .map_err(|err| RedactionLogError::Sqlite(err.to_string()))?;
        }
        if !entry.conflict_loser
            && entry.document_kind == DocumentKind::Text
            && entry.action != Action::Preserve
        {
            self.detections.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct DaemonRequest {
    session_id: String,
    text: String,
}

#[derive(Serialize)]
#[serde(untagged)]
enum DaemonResponse {
    Clean {
        session_id: String,
        clean_text: String,
        manifest: Vec<EmittedTokenSpan>,
        tokens: Vec<TokenJson>,
    },
    Error {
        session_id: Option<String>,
        error: &'static str,
        detail: String,
    },
}

impl DaemonResponse {
    fn error(session_id: Option<String>, error: &'static str, detail: impl Into<String>) -> Self {
        Self::Error {
            session_id,
            error,
            detail: detail.into(),
        }
    }
}

#[derive(Serialize)]
struct TokenJson {
    class: String,
    token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    family: Option<String>,
}

impl From<SessionSnapshotEntry> for TokenJson {
    fn from(entry: SessionSnapshotEntry) -> Self {
        Self {
            class: entry_class_to_string(&entry.class),
            token: entry.token,
            family: Some(entry.family),
        }
    }
}

enum DaemonError {
    Cli(CliError),
    Invariant,
}

impl DaemonError {
    fn variant(&self) -> &'static str {
        match self {
            Self::Cli(error) => match error {
                CliError::SafetyNetFailure { variant } => variant,
                CliError::SafetyNetConfigDetail(_) => "SafetyNetConfig",
                CliError::PolicyConfigDetail(_) => "PolicyConfig",
                CliError::PolicyOpen => "PolicyOpen",
                CliError::Pipeline => "Pipeline",
                CliError::Io => "Io",
                _ => "CliError",
            },
            Self::Invariant => "PipelineInvariant",
        }
    }

    fn detail(&self) -> Option<&str> {
        match self {
            Self::Cli(CliError::SafetyNetConfigDetail(detail))
            | Self::Cli(CliError::PolicyConfigDetail(detail)) => Some(detail.as_str()),
            Self::Cli(_) => Some("gaze daemon request failed closed"),
            Self::Invariant => Some("unexpected non-text clean document"),
        }
    }
}

pub(crate) fn default_process_idle_timeout_secs() -> u64 {
    DEFAULT_PROCESS_IDLE_TIMEOUT_SECS
}

pub(crate) fn default_session_idle_timeout_secs() -> u64 {
    DEFAULT_SESSION_IDLE_TIMEOUT_SECS
}

pub(crate) fn default_session_cap() -> usize {
    DEFAULT_SESSION_CAP
}
