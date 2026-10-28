use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::shared_args::{
    NymArgs, OpenAiFilterSubprocessArgs, OpfRegistryArgs, RulepackOverrideArgs, SafetyNetLimitArgs,
    SafetyNetRegistryArgs,
};
use super::{
    OpenAiFilterDevice, SafetyNetBackend, SafetyNetFallback, SafetyNetKind, SafetyNetMode,
};
use crate::error::CliError;
use crate::error::RestoreMode;
use crate::io::DEFAULT_MAX_BYTES;
use crate::pipeline::build::{
    map_policy_error, policy_warnings, resolve_pipeline, validate_ner_threshold,
};
use crate::pipeline::run::{
    clean_overrides_from_options, enforce_safety_net_mode, entry_class_to_string, map_clean_error,
    map_context_error, maybe_register_safety_net, policy_nym_was_overridden, safety_net_policy,
    validate_safety_net_tolerant_gate, wire_ordered_entries, CleanOptions,
    CORE_EXTENDED_DEPRECATION,
};
use crate::restore::session::restore_pass2_validate;
use gaze::{
    Action, ConflictTier, DictionaryBundle, DocumentKind, EmittedTokenSpan, LeakReport, PiiClass,
    Policy, RawDocument, RedactionEntry, RedactionLogError, RedactionLogger, Result as GazeResult,
    Rulepack, Session, SessionSnapshotEntry, TypedContext,
};
use gaze_audit::SqliteLogger;

mod input;
mod v2;

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
    /// Safety nets to run. Repeatable; "none" disables policy selection for this run.
    #[arg(long, value_enum)]
    pub(crate) safety_net: Vec<SafetyNetKind>,
    /// v0.8 backend selector. Replaces one `--safety-net=<kind>`; cannot select from a list.
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
    #[command(flatten)]
    pub(crate) opf_registry: OpfRegistryArgs,
    #[command(flatten)]
    pub(crate) nym: NymArgs,
    #[command(flatten)]
    pub(crate) safety_net_limits: SafetyNetLimitArgs,
}

pub(crate) fn run(args: Args) -> std::result::Result<(), CliError> {
    let startup = gaze_types::perf_span!("daemon_startup_ns");
    let shutdown = install_signal_flags()?;
    let deprecated_core_extended = args
        .rulepacks
        .rulepack_bundled
        .iter()
        .any(|bundle| bundle == "core-extended");
    let mut daemon = Daemon::new(args)?;
    if deprecated_core_extended {
        eprintln!("warning: {CORE_EXTENDED_DEPRECATION}");
    }
    for warning in policy_warnings(&daemon.policy, &daemon.pipeline) {
        eprintln!("{warning}");
    }
    let frame_limit = daemon_frame_max_bytes();
    let (sender, receiver) = input::channel(frame_limit);
    std::thread::spawn(move || {
        input::read_frames(io::stdin().lock(), sender, frame_limit);
    });

    let idle_timeout = daemon.process_idle_timeout;
    let mut stdout = io::stdout().lock();
    drop(startup);
    #[cfg(feature = "perf-timers")]
    crate::perf::ready();
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                daemon.last_stdin_activity = Instant::now();
                daemon.evict_idle_sessions();
                let terminal = matches!(frame, input::Frame::FrameTooLarge { .. });
                let response = match frame {
                    input::Frame::Line(line) => daemon.handle_line(&line),
                    input::Frame::FrameTooLarge { limit } => DaemonResponse::frame_too_large(limit),
                };
                write_json_line(&mut stdout, &response)?;
                if terminal {
                    break;
                }
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
    locale_chain: gaze::LocaleChain,
    rulepacks: Vec<Rulepack>,
    dictionaries: DictionaryBundle,
    safety_net_mode: SafetyNetMode,
    safety_net_fallback: SafetyNetFallback,
    logger: Arc<DaemonLogger>,
    sessions: HashMap<String, SessionEntry>,
    v2: v2::Registry,
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
        let loaded_policy = resolved.policy;
        let locale_chain = resolved.locale_chain;
        let dictionaries = resolved.dictionaries;
        let pipeline = maybe_register_safety_net(resolved.pipeline, &options, &loaded_policy)?;
        if policy_nym_was_overridden(&options, &loaded_policy) {
            eprintln!("notice: command line disabled policy safety net nym");
        }
        Ok(Self {
            pipeline,
            policy: loaded_policy,
            locale_chain,
            rulepacks: resolved.rulepacks,
            dictionaries,
            safety_net_mode: args.safety_net_limits.safety_net_mode,
            safety_net_fallback: args.safety_net_limits.safety_net_fallback,
            logger,
            sessions: HashMap::new(),
            v2: v2::Registry::new().map_err(|_| CliError::Pipeline)?,
            lru: VecDeque::new(),
            session_cap: args.session_cap,
            session_idle_timeout: Duration::from_secs(args.session_idle_timeout),
            process_idle_timeout: Duration::from_secs(args.idle_timeout),
            last_stdin_activity: Instant::now(),
        })
    }

    fn handle_line(&mut self, line: &str) -> DaemonResponse {
        if v2::is_v2(line) {
            return DaemonResponse::V2(self.handle_v2(line));
        }
        #[cfg(feature = "perf-timers")]
        let _perf = crate::perf::Document::from_env();
        // Keep context opaque so only the shared parser applies its depth and size limits.
        let envelope =
            serde_json::from_str::<HashMap<String, Box<serde_json::value::RawValue>>>(line);
        let (version, parsed) = match envelope {
            Ok(envelope) if envelope.contains_key("context") => {
                let version = envelope
                    .get("version")
                    .and_then(|raw| serde_json::from_str::<u32>(raw.get()).ok());
                (version, serde_json::from_str::<DaemonRequest>(line))
            }
            _ => {
                // Preserve the legacy no-context shape and duplicate-key behavior.
                let value: serde_json::Value = match serde_json::from_str(line) {
                    Ok(value) => value,
                    Err(_) => return DaemonResponse::error("JsonMalformed", "malformed JSON line"),
                };
                let version = value
                    .get("version")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|version| u32::try_from(version).ok());
                (version, serde_json::from_value::<DaemonRequest>(value))
            }
        };
        let request = match parsed {
            Ok(request) if !request.session_id.trim().is_empty() => request,
            Ok(_) => {
                return DaemonResponse::error_versioned(
                    version,
                    "ProtocolInvalid",
                    "missing session_id",
                )
            }
            Err(_) if version.is_none() => {
                return DaemonResponse::error("JsonMalformed", "malformed JSON line")
            }
            Err(_) => {
                return DaemonResponse::error_versioned(
                    version,
                    "ProtocolInvalid",
                    "invalid request shape",
                )
            }
        };
        if self.v2.contains(&request.session_id) {
            return DaemonResponse::error_versioned(
                request.version,
                "ProtocolInvalid",
                "invalid daemon request",
            );
        }
        let session_id = request.session_id.clone();
        let version = request.version;
        let result = match (request.version, request.op.as_deref()) {
            (None, None) => match request.text {
                Some(text) => self.clean_request(session_id.clone(), text, false, request.context),
                None => return DaemonResponse::error("JsonMalformed", "malformed JSON line"),
            },
            (Some(1), Some("open")) if request.text.is_none() && request.context.0.is_none() => {
                self.evict_idle_sessions();
                if self.sessions.contains_key(&session_id) {
                    Err((session_id.clone(), DaemonError::SessionExists))
                } else {
                    self.ensure_session(&session_id)
                        .map(|()| DaemonResponse::Opened {
                            version: 1,
                            session_id: session_id.clone(),
                            op: "open",
                        })
                        .map_err(|error| (session_id.clone(), error))
                }
            }
            (Some(1), Some("clean")) => match request.text {
                Some(text) => self.clean_request(session_id.clone(), text, true, request.context),
                None => Err((session_id.clone(), DaemonError::ProtocolInvalid)),
            },
            (Some(1), Some("restore")) if request.context.0.is_none() => match request.text {
                Some(text) => self.restore_request(session_id.clone(), text),
                None => Err((session_id.clone(), DaemonError::ProtocolInvalid)),
            },
            (Some(1), Some("close")) if request.text.is_none() && request.context.0.is_none() => {
                self.evict_idle_sessions();
                if self.sessions.remove(&session_id).is_some() {
                    self.lru.retain(|id| id != &session_id);
                    Ok(DaemonResponse::Closed {
                        version: 1,
                        session_id: session_id.clone(),
                        op: "close",
                    })
                } else {
                    Err((session_id.clone(), DaemonError::SessionMissing))
                }
            }
            (Some(1), _) | (None, Some(_)) => {
                Err((session_id.clone(), DaemonError::ProtocolInvalid))
            }
            (Some(_), _) => Err((session_id.clone(), DaemonError::ProtocolVersion)),
        };
        match result {
            Ok(response) => response,
            Err((_, error)) => DaemonResponse::failure(version, &error),
        }
    }

    fn clean_request(
        &mut self,
        session_id: String,
        text: String,
        versioned: bool,
        context: RequestContext,
    ) -> std::result::Result<DaemonResponse, (String, DaemonError)> {
        let request_inputs = context
            .0
            .map(|raw| {
                let context = TypedContext::from_json_str(raw.get()).map_err(map_context_error)?;
                let pipeline = gaze_assembly::pipeline_with_context(
                    &self.pipeline,
                    &self.policy,
                    &context,
                    &self.rulepacks,
                    &self.locale_chain,
                )
                .map_err(|_| {
                    CliError::PolicyConfigDetail("invalid context JSON configuration".into())
                })?;
                let dictionaries = DictionaryBundle::merge(
                    self.dictionaries.clone(),
                    gaze::dictionary_bundle_from_context(&context),
                );
                Ok::<_, CliError>((pipeline, dictionaries))
            })
            .transpose()
            .map_err(|error| (session_id.clone(), DaemonError::Cli(error)))?;

        if versioned {
            self.touch_session(&session_id)
        } else {
            self.ensure_session(&session_id)
        }
        .map_err(|err| (session_id.clone(), err))?;
        let session = &self
            .sessions
            .get(&session_id)
            .expect("session inserted")
            .session;
        let (pipeline, dictionaries) = request_inputs
            .as_ref()
            .map(|(pipeline, dictionaries)| (pipeline, dictionaries))
            .unwrap_or((&self.pipeline, &self.dictionaries));
        let policy = safety_net_policy(self.safety_net_mode, self.safety_net_fallback);
        let (clean_doc, manifest, leak_report) = pipeline
            .clean_with_safety_net_policy_detect_context(
                session,
                RawDocument::Text(text),
                self.locale_chain.as_slice(),
                dictionaries,
                policy,
            )
            .map_err(|err| {
                (
                    session_id.clone(),
                    DaemonError::Cli(map_clean_error(pipeline, err)),
                )
            })?;
        self.logger
            .log_safety_net_report(&leak_report, session, DocumentKind::Text)
            .map_err(|_| (session_id.clone(), DaemonError::Cli(CliError::Pipeline)))?;
        enforce_safety_net_mode(&leak_report, policy)
            .map_err(|err| (session_id.clone(), DaemonError::Cli(err)))?;
        let clean_text = match clean_doc {
            gaze::CleanDocument::Text(text) => text,
            _ => return Err((session_id, DaemonError::Invariant)),
        };
        let tokens = wire_ordered_entries(session.snapshot_entries(), &clean_text, &manifest)
            .into_iter()
            .map(TokenJson::from)
            .collect();
        Ok(DaemonResponse::Clean {
            version: versioned.then_some(1),
            op: versioned.then_some("clean"),
            session_id,
            clean_text,
            manifest,
            tokens,
        })
    }

    fn restore_request(
        &mut self,
        session_id: String,
        text: String,
    ) -> std::result::Result<DaemonResponse, (String, DaemonError)> {
        self.touch_session(&session_id)
            .map_err(|error| (session_id.clone(), error))?;
        let session = &self
            .sessions
            .get(&session_id)
            .expect("session touched")
            .session;
        let assessment = session
            .assess_restore_text(&text)
            .map_err(|_| (session_id.clone(), DaemonError::RestoreFailed))?;
        restore_pass2_validate(&assessment, RestoreMode::Strict).map_err(|error| {
            let variant = if matches!(error, CliError::UnknownToken { .. }) {
                DaemonError::UnknownToken
            } else {
                DaemonError::RestoreFailed
            };
            (session_id.clone(), variant)
        })?;
        Ok(DaemonResponse::Restored {
            version: 1,
            op: "restore",
            session_id,
            text: assessment.into_restored().text,
        })
    }

    fn touch_session(&mut self, session_id: &str) -> std::result::Result<(), DaemonError> {
        self.evict_idle_sessions();
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or(DaemonError::SessionMissing)?;
        entry.last_seen = Instant::now();
        self.lru.retain(|existing| existing != session_id);
        self.lru.push_back(session_id.to_string());
        Ok(())
    }

    fn ensure_session(&mut self, session_id: &str) -> std::result::Result<(), DaemonError> {
        self.evict_idle_sessions();
        if let Some(entry) = self.sessions.get_mut(session_id) {
            entry.last_seen = Instant::now();
            self.lru.retain(|existing| existing != session_id);
            self.lru.push_back(session_id.to_string());
            return Ok(());
        }
        self.make_session_room();
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
        for session in self.v2.evict_idle(self.session_idle_timeout) {
            self.log_eviction(&session, "idle_timeout");
        }
        let now = Instant::now();
        let mut expired = self
            .sessions
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.last_seen) >= self.session_idle_timeout)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        // `sessions` is a `HashMap`: sort so eviction audit rows and stderr lines come out
        // in the same order on every run.
        expired.sort();
        for evicted in expired {
            if let Some(entry) = self.sessions.remove(&evicted) {
                self.lru.retain(|existing| existing != &evicted);
                self.log_eviction(&entry.session, "idle_timeout");
            }
        }
    }

    fn log_eviction(&self, session: &Session, reason: &str) {
        let audit_session_id = session.audit_session_id();
        tracing::warn!(audit_session_id = %audit_session_id, reason = %reason, "gaze daemon evicted session");
        if let Err(err) = self.logger.log_eviction(session, reason) {
            let audit_session_id = serde_json::to_string(audit_session_id)
                .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
            let reason = serde_json::to_string(reason)
                .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
            let error_code = match &err {
                RedactionLogError::Sqlite(_) => "Sqlite",
                RedactionLogError::Backend(_) => "Backend",
                _ => "Unknown",
            };
            let detail = serde_json::to_string(error_code)
                .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
            eprintln!(
                r#"{{"error":"AuditWriteFailed","audit_session_id":{},"reason":{},"detail":{}}}"#,
                audit_session_id, reason, detail
            );
        }
    }
}

/// `CleanOptions::format` is read only by `require_json_format` inside `run_clean`.
/// The daemon never calls `run_clean`: its wire format is the JSONL protocol written
/// by `write_json_line` and is not selectable, which is why there is no `--format` on
/// `gaze daemon`. The field is inert here, and set to the encoding the daemon does in
/// fact emit rather than to an arbitrary string.
const DAEMON_CLEAN_FORMAT: &str = "json";

/// `CleanOptions::max_bytes` bounds `read_stdin_text`, which only `run_clean` calls.
/// The daemon reads its own bounded frames; this inert CleanOptions field also
/// supplies the base for its separate wire limit.
const DAEMON_CLEAN_MAX_BYTES: u64 = DEFAULT_MAX_BYTES;

fn daemon_frame_max_bytes() -> usize {
    DAEMON_CLEAN_MAX_BYTES
        .checked_mul(16)
        .and_then(|limit| usize::try_from(limit).ok())
        .expect("daemon frame limit must fit usize")
}

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
        // request. The clean frame accepts `context` instead.
        context_json: None,
        audit_db: args.audit_db.as_deref(),
        safety_net: &args.safety_net,
        safety_net_backend: args.safety_net_backend,
        safety_net_registry: args.safety_net_registry.safety_net_registry,
        safety_net_add: &args.safety_net_registry.safety_net_add,
        openai_filter_command: args.openai_filter.openai_filter_command.as_deref(),
        openai_filter_checkpoint: args.openai_filter.openai_filter_checkpoint.as_deref(),
        openai_filter_operating_point: args.openai_filter.openai_filter_operating_point,
        openai_filter_device: args.openai_filter_device,
        opf_locales: &args.opf_registry.opf_locales,
        opf_command: args.opf_registry.opf_command.as_deref(),
        opf_checkpoint: args.opf_registry.opf_checkpoint.as_deref(),
        nym_model_dir: args.nym.nym_model_dir.as_deref(),
        nym_intra_threads: args.nym.nym_intra_threads,
        safety_net_timeout_ms: args.safety_net_limits.safety_net_timeout_ms,
        safety_net_input_limit_bytes: args.safety_net_limits.safety_net_input_limit_bytes,
        safety_net_mode: args.safety_net_limits.safety_net_mode,
        safety_net_fallback: args.safety_net_limits.safety_net_fallback,
        // Sessions stay in the daemon's per-`session_id` registry; only one-shot
        // `gaze clean` writes a session file.
        session_out: None,
    }
}

struct SessionEntry {
    session: Session,
    last_seen: Instant,
}

struct DaemonLogger {
    detections: AtomicU64,
    audit: Option<SqliteLogger>,
    deferred: Mutex<Option<AuditBuffer>>,
}

impl DaemonLogger {
    fn new(audit_db: Option<&Path>) -> GazeResult<Self> {
        Ok(Self {
            detections: AtomicU64::new(0),
            deferred: Mutex::new(None),
            audit: audit_db
                .map(SqliteLogger::new)
                .transpose()
                .map_err(|err| gaze::Error::Sqlite(err.to_string()))?,
        })
    }

    // Requests execute serially through &mut Daemon. The loaded and context
    // pipelines share this sink, so a scope covers every synchronous detector.
    fn defer_redactions(&self, limit: usize) -> Result<DeferredRedactions<'_>, RedactionLogError> {
        let mut buffer = self.deferred.lock().map_err(|_| audit_buffer_error())?;
        if buffer.is_some() {
            return Err(RedactionLogError::Backend(
                "audit scope already active".into(),
            ));
        }
        *buffer = Some(AuditBuffer {
            entries: Vec::new(),
            heap_bytes: 0,
            limit,
            exceeded: false,
        });
        Ok(DeferredRedactions(self))
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
        audit.log_safety_net_report(
            report,
            document_kind,
            created_at,
            Some(session.audit_session_id().to_string()),
        )
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

impl DaemonLogger {
    fn log_committed(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
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

impl RedactionLogger for DaemonLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        if let Some(buffer) = self
            .deferred
            .lock()
            .map_err(|_| audit_buffer_error())?
            .as_mut()
        {
            buffer.reserve_entry(entry)?;
            buffer.entries.push(entry.clone());
            return Ok(());
        }
        self.log_committed(entry)
    }
}

fn audit_buffer_error() -> RedactionLogError {
    RedactionLogError::Backend("audit scope unavailable".into())
}

struct AuditBuffer {
    entries: Vec<RedactionEntry>,
    heap_bytes: usize,
    limit: usize,
    exceeded: bool,
}

impl AuditBuffer {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(
                self.entries
                    .capacity()
                    .saturating_mul(std::mem::size_of::<RedactionEntry>()),
            )
            .saturating_add(self.heap_bytes)
    }

    fn reserve_entry(&mut self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        // The serialized metadata includes every nested field. Charge sixteen
        // times its size for cloned heap members, container and allocator overhead,
        // in addition to the allocated entry slots. Counting never retains JSON.
        let mut size = AuditSize(0);
        if serde_json::to_writer(&mut size, entry).is_err() {
            self.exceeded = true;
            return Err(audit_capacity_error());
        }
        let heap = size.0.saturating_mul(16);
        let capacity = if self.entries.len() == self.entries.capacity() {
            self.entries.capacity().saturating_mul(2).max(4)
        } else {
            self.entries.capacity()
        };
        let requested = std::mem::size_of::<Self>()
            .saturating_add(capacity.saturating_mul(std::mem::size_of::<RedactionEntry>()))
            .saturating_add(self.heap_bytes)
            .saturating_add(heap);
        if requested > self.limit {
            self.exceeded = true;
            return Err(audit_capacity_error());
        }
        if self
            .entries
            .try_reserve_exact(capacity - self.entries.len())
            .is_err()
        {
            self.exceeded = true;
            return Err(audit_capacity_error());
        }
        self.heap_bytes = self.heap_bytes.saturating_add(heap);
        Ok(())
    }
}

struct AuditSize(usize);
impl Write for AuditSize {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn audit_capacity_error() -> RedactionLogError {
    RedactionLogError::Backend("audit capacity".into())
}

struct DeferredRedactions<'a>(&'a DaemonLogger);

impl DeferredRedactions<'_> {
    fn bytes(&self) -> Result<usize, RedactionLogError> {
        Ok(self
            .0
            .deferred
            .lock()
            .map_err(|_| audit_buffer_error())?
            .as_ref()
            .expect("active audit scope")
            .bytes())
    }

    fn capacity_exceeded(&self) -> bool {
        self.0.deferred.lock().map_or(true, |slot| {
            slot.as_ref().is_some_and(|buffer| buffer.exceeded)
        })
    }

    fn reset_pass(&self) -> Result<(), RedactionLogError> {
        let mut slot = self.0.deferred.lock().map_err(|_| audit_buffer_error())?;
        let buffer = slot.as_mut().expect("active audit scope");
        buffer.entries.clear();
        buffer.heap_bytes = 0;
        buffer.exceeded = false;
        Ok(())
    }

    fn flush(self, reports: &[LeakReport], session_id: String) -> Result<(), RedactionLogError> {
        let mut entries = self
            .0
            .deferred
            .lock()
            .map_err(|_| audit_buffer_error())?
            .take()
            .expect("active audit scope")
            .entries;
        for entry in &mut entries {
            entry.provenance_stage = Some("daemon".to_string());
        }
        if let Some(audit) = &self.0.audit {
            audit
                .log_batch(
                    &entries,
                    reports,
                    DocumentKind::Text,
                    chrono::Utc::now().timestamp_millis(),
                    Some(session_id),
                )
                .map_err(|err| RedactionLogError::Sqlite(err.to_string()))?;
        }
        let detections = entries
            .iter()
            .filter(|entry| {
                !entry.conflict_loser
                    && entry.document_kind == DocumentKind::Text
                    && entry.action != Action::Preserve
            })
            .count();
        self.0
            .detections
            .fetch_add(detections as u64, Ordering::Relaxed);
        Ok(())
    }
}

impl Drop for DeferredRedactions<'_> {
    fn drop(&mut self) {
        self.0
            .deferred
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}

#[derive(Deserialize)]
struct DaemonRequest {
    version: Option<u32>,
    op: Option<String>,
    session_id: String,
    text: Option<String>,
    #[serde(default)]
    context: RequestContext,
}

#[derive(Default)]
struct RequestContext(Option<Box<serde_json::value::RawValue>>);

impl<'de> Deserialize<'de> for RequestContext {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // A present null must reach Context::from_json_str and fail closed.
        Box::<serde_json::value::RawValue>::deserialize(deserializer).map(|raw| Self(Some(raw)))
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum DaemonResponse {
    V2(Box<serde_json::value::RawValue>),
    Clean {
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        op: Option<&'static str>,
        session_id: String,
        clean_text: String,
        manifest: Vec<EmittedTokenSpan>,
        tokens: Vec<TokenJson>,
    },
    Opened {
        version: u32,
        op: &'static str,
        session_id: String,
    },
    Restored {
        version: u32,
        op: &'static str,
        session_id: String,
        text: String,
    },
    Closed {
        version: u32,
        op: &'static str,
        session_id: String,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<u32>,
        session_id: Option<String>,
        error: &'static str,
        detail: String,
        /// Closed refusal reason; only `Untransformable` sets it.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<&'static str>,
    },
}

impl DaemonResponse {
    fn frame_too_large(limit: usize) -> Self {
        Self::error(
            "FrameTooLarge",
            format!("daemon frame exceeds {limit} bytes"),
        )
    }

    fn failure(version: Option<u32>, error: &DaemonError) -> Self {
        Self::Error {
            version,
            session_id: None,
            error: error.variant(),
            detail: error
                .detail()
                .unwrap_or("daemon request failed closed")
                .into(),
            reason: error.reason(),
        }
    }

    fn error(error: &'static str, detail: impl Into<String>) -> Self {
        Self::error_versioned(None, error, detail)
    }

    fn error_versioned(
        version: Option<u32>,
        error: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self::Error {
            version,
            session_id: None,
            error,
            detail: detail.into(),
            reason: None,
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
    ProtocolInvalid,
    ProtocolVersion,
    SessionExists,
    SessionMissing,
    RestoreFailed,
    UnknownToken,
}

impl DaemonError {
    fn variant(&self) -> &'static str {
        match self {
            Self::Cli(error) => match error {
                CliError::SafetyNetFailure { variant } => variant,
                CliError::SafetyNetConfigDetail(_) => "SafetyNetConfig",
                CliError::PolicyConfigDetail(_) => "PolicyConfig",
                CliError::PolicyOpen | CliError::PolicyOpenDetail(_) => "PolicyOpen",
                CliError::Pipeline => "Pipeline",
                CliError::Io => "Io",
                CliError::Untransformable { .. } => "Untransformable",
                _ => "CliError",
            },
            Self::Invariant => "PipelineInvariant",
            Self::ProtocolInvalid => "ProtocolInvalid",
            Self::ProtocolVersion => "ProtocolVersion",
            Self::SessionExists => "SessionExists",
            Self::SessionMissing => "SessionMissing",
            Self::RestoreFailed => "RestoreFailed",
            Self::UnknownToken => "UnknownToken",
        }
    }

    fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Cli(CliError::Untransformable { reason }) => Some(reason.as_str()),
            _ => None,
        }
    }

    fn detail(&self) -> Option<&str> {
        match self {
            Self::Cli(CliError::SafetyNetConfigDetail(detail))
            | Self::Cli(CliError::PolicyConfigDetail(detail))
            | Self::Cli(CliError::PolicyOpenDetail(detail)) => Some(detail.as_str()),
            Self::Cli(CliError::Untransformable { .. }) => {
                Some("confirmed sensitive field cannot be transformed")
            }
            Self::Cli(_) => Some("gaze daemon request failed closed"),
            Self::Invariant => Some("unexpected non-text clean document"),
            Self::ProtocolInvalid => Some("invalid daemon request"),
            Self::ProtocolVersion => Some("unsupported daemon protocol version"),
            Self::SessionExists => Some("session already open"),
            Self::SessionMissing => Some("session not open"),
            Self::RestoreFailed => Some("restore failed closed"),
            Self::UnknownToken => Some("restore contains an unknown token"),
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

#[cfg(test)]
mod residual_wire_tests {
    use super::*;

    #[test]
    fn daemon_frame_default_is_sixteen_times_clean_limit() {
        assert_eq!(
            daemon_frame_max_bytes() as u64,
            DAEMON_CLEAN_MAX_BYTES.checked_mul(16).unwrap()
        );
        assert_eq!(daemon_frame_max_bytes(), 160 * 1024 * 1024);
    }

    #[test]
    fn untransformable_daemon_error_carries_only_the_closed_reason() {
        let mut wire = Vec::new();
        let error = DaemonError::Cli(CliError::Untransformable {
            reason: gaze_types::UntransformableReason::CallerValueTooShort,
        });
        write_json_line(&mut wire, &DaemonResponse::failure(Some(1), &error)).unwrap();
        assert_eq!(
            String::from_utf8(wire).unwrap(),
            "{\"version\":1,\"session_id\":null,\"error\":\"Untransformable\",\"detail\":\"confirmed sensitive field cannot be transformed\",\"reason\":\"caller_value_too_short\"}\n"
        );
        let mut wire = Vec::new();
        write_json_line(
            &mut wire,
            &DaemonResponse::failure(Some(1), &DaemonError::Cli(CliError::Pipeline)),
        )
        .unwrap();
        assert!(!String::from_utf8(wire).unwrap().contains("reason"));
    }

    #[test]
    fn daemon_frame_overflow_response_is_fixed_and_value_free() {
        let mut wire = Vec::new();
        write_json_line(&mut wire, &DaemonResponse::frame_too_large(8)).unwrap();
        assert_eq!(wire, b"{\"session_id\":null,\"error\":\"FrameTooLarge\",\"detail\":\"daemon frame exceeds 8 bytes\"}\n");
    }

    #[test]
    fn daemon_frame_session_responses_match_legacy_reader_bytes() {
        use clap::Parser;
        use std::io::{BufRead, BufReader, Cursor};
        #[derive(Parser)]
        struct TestArgs {
            #[command(flatten)]
            daemon: Args,
        }
        let dir = tempfile::tempdir().unwrap();
        let policy = dir.path().join("policy.toml");
        std::fs::write(
            &policy,
            r#"
schema_version = "0.1.0"
[session]
scope = "conversation"
[policy.rulepacks]
bundled = ["core"]
[[policy.custom_recognizers]]
kind = "regex"
name = "synthetic_email"
pattern = '[A-Za-z0-9._%+-]+@example[.]invalid'
class = "email"
safety_tier = "safe_default"
[[rule]]
kind = "class"
class = "email"
action = "tokenize"
[[rule]]
kind = "default"
action = "preserve"
"#,
        )
        .unwrap();
        // Same synthetic email and session lifecycle as tests/daemon_reversible.rs.
        let fixture = concat!(
            "{\"version\":1,\"op\":\"clean\",\"session_id\":\"run-a\",\"text\":\"alice@example.invalid\"}\r\n",
            "\n",
            "{\"version\":1,\"op\":\"clean\",\"session_id\":\"run-a\",\"text\":\"alice@example.invalid\"}\n",
            "{\"version\":1,\"op\":\"close\",\"session_id\":\"run-a\"}",
        ).as_bytes();
        let make_daemon = || {
            Daemon::new(
                TestArgs::parse_from(["daemon", "--policy", policy.to_str().unwrap()]).daemon,
            )
            .unwrap()
        };
        let mut legacy = make_daemon();
        let mut bounded = make_daemon();
        assert!(legacy.ensure_session("run-a").is_ok());
        assert!(bounded.ensure_session("run-a").is_ok());
        // Share the empty session identity so random token prefixes do not
        // obscure byte-for-byte wire parity.
        bounded.sessions.get_mut("run-a").unwrap().session =
            Session::import(legacy.sessions["run-a"].session.export().unwrap()).unwrap();
        let mut expected = Vec::new();
        for line in fixture.lines() {
            write_json_line(&mut expected, &legacy.handle_line(&line.unwrap())).unwrap();
        }
        let mut actual = Vec::new();
        std::thread::scope(|scope| {
            let (sender, receiver) = input::channel(1024);
            scope.spawn(move || {
                input::read_frames(
                    BufReader::with_capacity(1, Cursor::new(fixture)),
                    sender,
                    1024,
                )
            });
            for _ in 0..4 {
                let frame = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
                let input::Frame::Line(line) = frame else {
                    panic!("fixture must fit");
                };
                write_json_line(&mut actual, &bounded.handle_line(&line)).unwrap();
            }
        });
        assert_eq!(actual, expected);
        assert_eq!(actual.iter().filter(|byte| **byte == b'\n').count(), 4);
    }

    #[test]
    fn daemon_session_restores_adjacent_format_preserving_tokens() {
        use clap::Parser;
        #[derive(Parser)]
        struct TestArgs {
            #[command(flatten)]
            daemon: Args,
        }
        let dir = tempfile::tempdir().unwrap();
        let policy = dir.path().join("policy.toml");
        std::fs::write(
            &policy,
            r#"
[session]
scope = "persistent"
ttl_secs = 86400
[[policy.custom_recognizers]]
kind = "regex"
name = "synthetic_names"
pattern = 'Synthetic[0-9]+'
class = "name"
[[rule]]
kind = "class"
class = "name"
action = "format_preserve"
[[rule]]
kind = "default"
action = "preserve"
"#,
        )
        .unwrap();
        let args = TestArgs::parse_from(["daemon", "--policy", policy.to_str().unwrap()]);
        let mut daemon = Daemon::new(args.daemon).unwrap();
        for raw in ["Synthetic1Synthetic2", "Synthetic1Synthetic2Synthetic3"] {
            let response = daemon.handle_line(
                &serde_json::json!({
                    "session_id": "synthetic", "text": raw,
                })
                .to_string(),
            );
            let DaemonResponse::Clean { clean_text, .. } = response else {
                panic!("daemon clean must succeed");
            };
            let session = &daemon.sessions["synthetic"].session;
            assert_eq!(session.restore_strict_text(&clean_text).unwrap(), raw);
            assert_eq!(
                Session::import(session.export().unwrap())
                    .unwrap()
                    .restore_strict_text(&clean_text)
                    .unwrap(),
                raw
            );
        }
    }

    /// The daemon hands clients the manifest verbatim, and a client that treats
    /// one span as one recognized value would be wrong under residual coverage:
    /// a fragment is a replacement, not an entity.
    ///
    /// The contract stays truthful because the span type now says which it is.
    /// Pin that at this boundary rather than inferring it from the type's own
    /// tests, since this is where the JSON a client actually parses is produced.
    #[test]
    fn clean_responses_distinguish_whole_spans_from_residual_fragments() {
        let response = DaemonResponse::Clean {
            version: None,
            op: None,
            session_id: "s-1".to_string(),
            clean_text: "Email <tok> <frag> now".to_string(),
            manifest: vec![
                EmittedTokenSpan::new(6..11, 6..27, PiiClass::Email),
                EmittedTokenSpan::residual_fragment(12..18, 27..33, PiiClass::Email),
            ],
            tokens: Vec::new(),
        };
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
        let manifest = value["manifest"].as_array().unwrap();
        assert_eq!(manifest.len(), 2);

        // A whole span keeps the exact shape clients already parse: no new key.
        assert!(
            manifest[0].get("origin").is_none(),
            "a whole span must serialize exactly as before: {}",
            manifest[0]
        );
        // A fragment says so, so a client can choose not to count it as a value.
        assert_eq!(manifest[1]["origin"], "residual_fragment");
    }

    /// Flags a literal as an uncovered name; the `Redact` mode writes a marker over it.
    struct LiteralNet;

    impl gaze::SafetyNet for LiteralNet {
        fn id(&self) -> &str {
            "daemon.fixture"
        }
        fn supported_locales(&self) -> &[gaze::LocaleTag] {
            &[gaze::LocaleTag::Global]
        }
        fn check(
            &self,
            text: &str,
            _: gaze::SafetyNetContext<'_>,
        ) -> std::result::Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
            Ok(text
                .find("Schmidt")
                .map(|start| {
                    vec![gaze::LeakSuspect::new(
                        start..start + "Schmidt".len(),
                        PiiClass::Name,
                        self.id(),
                        Some(1.0),
                        gaze::LeakKind::Uncovered,
                        "person",
                        None,
                    )]
                })
                .unwrap_or_default())
        }
    }

    /// The daemon's `Clean` response carries the manifest verbatim and, separately, the session's
    /// restorable tokens. A safety-net redaction is now a manifest entry, so a client reading the
    /// manifest will meet one; it must be able to tell that entry is a one-way marker, and the
    /// `tokens` list -- what the client may hand back for restore -- must never advertise it.
    ///
    /// Built from a real pipeline run and a real session, assembled the way `clean_request` does,
    /// so the JSON under test is the JSON a client parses.
    #[test]
    fn a_redaction_marker_is_in_the_manifest_but_never_in_the_restorable_tokens() {
        let pipeline = gaze::Pipeline::builder()
            .rule(gaze::DefaultRule::new(gaze::Action::Preserve))
            .register_safety_net(LiteralNet)
            .build()
            .expect("pipeline");
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let (clean_doc, manifest, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                gaze::RawDocument::Text("Dear Schmidt, hello".into()),
                &[gaze::LocaleTag::Global],
                &gaze::DictionaryBundle::default(),
                gaze::SafetyNetPolicy::new(
                    gaze::SafetyNetMode::Redact,
                    gaze::SafetyNetFallback::Strict,
                ),
            )
            .expect("clean");
        let gaze::CleanDocument::Text(clean_text) = clean_doc else {
            panic!("text");
        };
        let response = DaemonResponse::Clean {
            version: None,
            op: None,
            session_id: "s-1".to_string(),
            clean_text: clean_text.clone(),
            manifest,
            tokens: session
                .snapshot_entries()
                .into_iter()
                .map(TokenJson::from)
                .collect(),
        };
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();

        let manifest = value["manifest"].as_array().unwrap();
        assert_eq!(manifest.len(), 1, "one entry, for the one redaction");
        let start = manifest[0]["clean_span"]["start"].as_u64().unwrap() as usize;
        let end = manifest[0]["clean_span"]["end"].as_u64().unwrap() as usize;
        let replacement = &clean_text[start..end];
        assert!(
            gaze::is_redaction_marker(replacement),
            "a client must be able to identify the entry as a marker: {replacement:?}"
        );
        assert!(
            value["tokens"]
                .as_array()
                .unwrap()
                .iter()
                .all(|token| token["token"] != replacement),
            "the marker must never be advertised as a restorable token"
        );
        assert!(value["tokens"].as_array().unwrap().is_empty());
    }
}
