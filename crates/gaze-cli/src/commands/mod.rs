mod audit;
mod clean;
mod daemon;
#[cfg(feature = "document")]
mod document;
#[cfg(feature = "index")]
mod index;
#[cfg(feature = "mcp")]
mod mcp;
#[cfg(feature = "proxy")]
mod proxy;
mod restore;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::error::{CliError, RestoreMode};
use crate::io::DEFAULT_MAX_BYTES;

pub(crate) const DEFAULT_SAFETY_NET_TIMEOUT_MS: u64 = 5_000;
pub(crate) const DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES: usize = 1_048_576;

#[derive(Parser, Debug)]
#[command(
    name = "gaze",
    version,
    about = "Channel-agnostic PII redaction for LLM pipes"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
enum Cmd {
    /// Read raw text from stdin; emit `{clean_text, session_blob, stats}` JSON to stdout.
    Clean {
        /// Path to policy.toml. Required once the policy loader lands (issue #3).
        #[arg(long)]
        policy: Option<PathBuf>,
        /// Output format. Only `json` is supported today.
        #[arg(long, default_value = "json")]
        format: String,
        /// Override the persistent session TTL in seconds.
        #[arg(long)]
        session_ttl: Option<u64>,
        /// Override policy \[session].scope.
        #[arg(long)]
        session_scope: Option<String>,
        /// Active locale fallback chain, comma separated and priority ordered.
        #[arg(long, value_delimiter = ',')]
        locale: Vec<String>,
        /// Override policy \[ner] threshold. Must be between 0.0 and 1.0 inclusive.
        #[arg(long)]
        ner_threshold: Option<f32>,
        /// Override policy \[ner].model_dir.
        #[arg(long)]
        ner_model_dir: Option<PathBuf>,
        /// Override policy \[ner].locale.
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
        /// Optional observer-only privacy safety net.
        #[arg(long, value_enum)]
        safety_net: Option<SafetyNetKind>,
        /// v0.8 backend selector. When set with
        /// `--safety-net=<kind>`, this flag wins. Lets adopters swap the
        /// Pass-3 backend without re-typing the legacy `--safety-net` value.
        #[arg(long, value_enum)]
        safety_net_backend: Option<SafetyNetBackend>,
        /// Enable locale-aware Pass-3 safety-net registry dispatch.
        #[arg(long)]
        safety_net_registry: bool,
        /// Add one backend to the locale-aware safety-net registry. Repeatable.
        #[arg(long, value_enum)]
        safety_net_add: Vec<SafetyNetBackend>,
        /// Path to the local OpenAI Privacy Filter `opf` command.
        #[arg(long)]
        openai_filter_command: Option<PathBuf>,
        /// Path to the local OpenAI Privacy Filter checkpoint or model directory.
        #[arg(long)]
        openai_filter_checkpoint: Option<PathBuf>,
        /// OpenAI Privacy Filter operating point, when supported by the command.
        #[arg(long, value_enum)]
        openai_filter_operating_point: Option<OpenAiFilterOperatingPoint>,
        /// Device selection for the OpenAI safety-net subprocess (auto|cpu|cuda|mps). Default: auto (let opf decide).
        #[arg(long, value_enum, default_value_t = OpenAiFilterDevice::Auto)]
        openai_filter_device: OpenAiFilterDevice,
        /// Kiji DistilBERT runtime backend. Default: subprocess for compatibility.
        #[arg(long, value_enum, default_value_t = KijiBackend::Subprocess)]
        kiji_backend: KijiBackend,
        /// Kiji DistilBERT ONNX precision. Default: fp32.
        #[arg(long, value_enum, default_value_t = KijiDistilbertPrecision::Fp32)]
        kiji_distilbert_precision: KijiDistilbertPrecision,
        /// Locale list for the OpenAI Privacy Filter registry entry.
        #[arg(long, value_delimiter = ',')]
        opf_locales: Vec<String>,
        /// Alias for --openai-filter-command in registry examples.
        #[arg(long)]
        opf_command: Option<PathBuf>,
        /// Alias for --openai-filter-checkpoint in registry examples.
        #[arg(long)]
        opf_checkpoint: Option<PathBuf>,
        /// Path to the local Kiji DistilBERT subprocess command.
        #[arg(long)]
        kiji_distilbert_command: Option<PathBuf>,
        /// Path to the pinned Kiji DistilBERT model directory (must contain
        /// SHA256SUMS, labels.json, model.onnx, tokenizer.json).
        #[arg(long)]
        kiji_distilbert_model_dir: Option<PathBuf>,
        /// Locale list for the Kiji DistilBERT registry entry.
        #[arg(long, value_delimiter = ',')]
        kiji_distilbert_locales: Vec<String>,
        /// Safety-net subprocess timeout in milliseconds.
        #[arg(long, default_value_t = DEFAULT_SAFETY_NET_TIMEOUT_MS)]
        safety_net_timeout_ms: u64,
        /// Maximum clean-text bytes submitted to the safety net.
        #[arg(long, default_value_t = DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES)]
        safety_net_input_limit_bytes: usize,
        /// Safety-net handling mode for suspected leaks.
        #[arg(long, value_enum, default_value_t = SafetyNetMode::Resolve)]
        safety_net_mode: SafetyNetMode,
        /// Fallback when safety-net resolve or redact cannot complete.
        #[arg(long, value_enum, default_value_t = SafetyNetFallback::Redact)]
        safety_net_fallback: SafetyNetFallback,
    },
    /// Run a long-lived JSON-lines stdio cleaning daemon.
    Daemon {
        /// Path to policy.toml.
        #[arg(long)]
        policy: PathBuf,
        /// Optional observer-only privacy safety net.
        #[arg(long, value_enum)]
        safety_net: Option<SafetyNetKind>,
        /// v0.8 backend selector. When set with `--safety-net=<kind>`, this flag wins.
        #[arg(long, value_enum)]
        safety_net_backend: Option<SafetyNetBackend>,
        /// Exit when stdin is idle for this many seconds.
        #[arg(long, default_value_t = daemon::default_process_idle_timeout_secs())]
        idle_timeout: u64,
        /// Evict sessions idle for this many seconds.
        #[arg(long, default_value_t = daemon::default_session_idle_timeout_secs())]
        session_idle_timeout: u64,
        /// Maximum live sessions before LRU eviction.
        #[arg(long, default_value_t = daemon::default_session_cap())]
        session_cap: usize,
        /// Optional SQLite redaction-log database path.
        #[arg(long)]
        audit_db: Option<PathBuf>,
        /// Active locale fallback chain, comma separated and priority ordered.
        #[arg(long, value_delimiter = ',')]
        locale: Vec<String>,
        /// Override policy \[ner\] threshold. Must be between 0.0 and 1.0 inclusive.
        #[arg(long)]
        ner_threshold: Option<f32>,
        /// Override policy \[ner\].model_dir.
        #[arg(long)]
        ner_model_dir: Option<PathBuf>,
        /// Override policy \[ner\].locale.
        #[arg(long)]
        ner_locale: Option<String>,
        /// Path to the local OpenAI Privacy Filter `opf` command.
        #[arg(long)]
        openai_filter_command: Option<PathBuf>,
        /// Path to the local OpenAI Privacy Filter checkpoint or model directory.
        #[arg(long)]
        openai_filter_checkpoint: Option<PathBuf>,
        /// OpenAI Privacy Filter operating point, when supported by the command.
        #[arg(long, value_enum)]
        openai_filter_operating_point: Option<OpenAiFilterOperatingPoint>,
        /// Device selection for the OpenAI safety-net subprocess.
        #[arg(long, value_enum, default_value_t = OpenAiFilterDevice::Auto)]
        openai_filter_device: OpenAiFilterDevice,
        /// Kiji DistilBERT runtime backend.
        #[arg(long, value_enum, default_value_t = KijiBackend::Subprocess)]
        kiji_backend: KijiBackend,
        /// Path to the local Kiji DistilBERT subprocess command.
        #[arg(long)]
        kiji_distilbert_command: Option<PathBuf>,
        /// Path to the pinned Kiji DistilBERT model directory.
        #[arg(long)]
        kiji_distilbert_model_dir: Option<PathBuf>,
        /// Locale list for the Kiji DistilBERT backend.
        #[arg(long, value_delimiter = ',')]
        kiji_distilbert_locales: Vec<String>,
        /// Safety-net subprocess timeout in milliseconds.
        #[arg(long, default_value_t = DEFAULT_SAFETY_NET_TIMEOUT_MS)]
        safety_net_timeout_ms: u64,
        /// Maximum clean-text bytes submitted to the safety net.
        #[arg(long, default_value_t = DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES)]
        safety_net_input_limit_bytes: usize,
        /// Safety-net handling mode for suspected leaks.
        #[arg(long, value_enum, default_value_t = SafetyNetMode::Resolve)]
        safety_net_mode: SafetyNetMode,
        /// Fallback when safety-net resolve or redact cannot complete.
        #[arg(long, value_enum, default_value_t = SafetyNetFallback::Redact)]
        safety_net_fallback: SafetyNetFallback,
    },
    /// Read `{session_blob, text}` JSON from stdin; emit `{text}` JSON to stdout.
    Restore {
        /// Output format. Only `json` is supported today.
        #[arg(long, default_value = "json")]
        format: String,
        /// Unknown-token handling during restore.
        #[arg(long, value_enum, default_value_t = RestoreMode::Strict)]
        restore_mode: RestoreMode,
        /// Alias for `--restore-mode` used by restore-boundary telemetry.
        #[arg(long = "policy", value_enum)]
        restore_policy: Option<RestoreMode>,
        /// Emit restore telemetry in the JSON response.
        #[arg(long)]
        telemetry: bool,
        /// Optional SQLite redaction-log database path for restore telemetry rows.
        #[arg(long)]
        audit_db: Option<PathBuf>,
        /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
    },
    /// Query, export, or maintain redaction-log metadata without reading raw PII payloads.
    Audit {
        #[command(subcommand)]
        command: AuditCmd,
    },
    /// Ingest a document (PNG/JPG/PDF) into a split SafeBundle.
    ///
    /// Requires the binary to be built with `--features document`.
    #[cfg(feature = "document")]
    Document {
        #[command(subcommand)]
        command: DocumentCmd,
    },
    /// Owner-side local text/Markdown index ingest and search.
    ///
    /// Requires the binary to be built with `--features index`.
    #[cfg(feature = "index")]
    Index {
        #[command(subcommand)]
        command: IndexCmd,
    },
    /// Install, diagnose, or run the Gaze MCP stdio server.
    ///
    /// Requires the binary to be built with `--features mcp`.
    #[cfg(feature = "mcp")]
    Mcp {
        #[command(subcommand)]
        command: McpCmd,
    },
    /// Run or manage the Gaze LLM proxy.
    ///
    /// Requires the binary to be built with `--features proxy`.
    #[cfg(feature = "proxy")]
    Proxy {
        #[command(subcommand)]
        command: ProxyCmd,
    },
}

#[cfg(feature = "document")]
#[derive(Subcommand, Debug)]
enum DocumentCmd {
    /// Run OCR + Gaze redact on `<input>` and split agent-safe files from owner restore material.
    #[command(group(
        clap::ArgGroup::new("document_output")
            .required(true)
            .args(["out", "agent_out"])
    ))]
    Clean {
        /// Source file. Must be `.png`, `.jpg`, `.jpeg`, or `.pdf` (single-page).
        input: PathBuf,
        /// Convenience output root. Creates `<PATH>/agent` and `<PATH>/owner`.
        #[arg(long, conflicts_with_all = ["agent_out", "owner_out"])]
        out: Option<PathBuf>,
        /// Agent-visible output directory for `clean.md` and `report.json`.
        #[arg(long, requires = "owner_out")]
        agent_out: Option<PathBuf>,
        /// Owner-only output directory for `manifest.json`.
        #[arg(long, requires = "agent_out")]
        owner_out: Option<PathBuf>,
    },
}

#[cfg(feature = "index")]
#[derive(Subcommand, Debug)]
enum IndexCmd {
    /// Ingest `.txt` and `.md` files into a local owner-side index.
    Ingest {
        /// Directory containing text/Markdown files.
        dir: PathBuf,
        /// Local index domain id.
        #[arg(long, default_value_t = index::default_domain())]
        domain: String,
        /// Owner-side index directory. Defaults to `GAZE_INDEX_PATH` or `./.gaze-index/`.
        #[arg(long)]
        index_path: Option<PathBuf>,
        /// Safety-net fallback for residual suspects after resolver pass.
        #[arg(long, value_enum, default_value_t = index::OnResidual::Redact)]
        on_residual: index::OnResidual,
    },
    /// Search the local owner-side index by one entity.
    Search {
        /// Entity value to search for. It is tokenized before authorization/search.
        entity: String,
        /// Local index domain id.
        #[arg(long, default_value_t = index::default_domain())]
        domain: String,
        /// Entity class: name, email, org, organization, or `custom:<name>`.
        #[arg(long = "class", value_parser = index::parse_index_class)]
        class: Option<gaze::PiiClass>,
        /// Owner-side index directory. Defaults to `GAZE_INDEX_PATH` or `./.gaze-index/`.
        #[arg(long)]
        index_path: Option<PathBuf>,
    },
}

#[cfg(feature = "mcp")]
#[derive(Subcommand, Debug)]
enum McpCmd {
    /// Install Gaze as an MCP stdio server in a supported client config.
    Install {
        /// Client config to update.
        #[arg(long, value_enum)]
        client: mcp::Client,
        /// Agent guidance file to create/update.
        #[arg(long)]
        agents_md: Option<PathBuf>,
        /// Print planned changes without writing files.
        #[arg(long)]
        dry_run: bool,
        /// Skip the marker-fenced AGENTS.md skill section.
        #[arg(long)]
        skip_agents_md: bool,
    },
    /// Diagnose MCP server dependencies and client configuration.
    Doctor {
        /// Agent guidance file to check.
        #[arg(long)]
        agents_md: Option<PathBuf>,
        /// Exit non-zero when any warning is present.
        #[arg(long)]
        strict: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run the Gaze MCP stdio server.
    Serve {
        /// Directory where MCP manifest call records are written.
        #[arg(long)]
        manifest_dir: Option<PathBuf>,
        /// Maximum file size accepted by `gaze_read_file`.
        #[arg(long)]
        max_file_size: Option<u64>,
    },
    /// Run the policy-gated MCP bridge over stdio.
    Bridge {
        /// Bridge TOML configuration.
        #[arg(long)]
        config: PathBuf,
        /// Override [session].dir from the config.
        #[arg(long)]
        session_dir: Option<PathBuf>,
        /// Load config and discover downstream surface without serving.
        #[arg(long)]
        dry_run: bool,
        /// Print namespaced downstream tools and denied resource/prompt counts.
        #[arg(long)]
        print_tools: bool,
    },
}

#[cfg(feature = "proxy")]
#[derive(Subcommand, Debug)]
enum ProxyCmd {
    /// Run proxy in the foreground.
    Serve {
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: std::net::SocketAddr,
        #[arg(long, default_value = "https://api.openai.com")]
        upstream_openai: url::Url,
        #[arg(long, default_value = "https://api.anthropic.com")]
        upstream_anthropic: url::Url,
        #[arg(long, default_value = "https://generativelanguage.googleapis.com")]
        upstream_gemini: url::Url,
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long, default_value = "core")]
        rulepack: String,
        #[arg(long, default_value = "30m")]
        session_ttl: String,
        #[arg(long = "_foreground-daemon", hide = true)]
        foreground_daemon: bool,
    },
    /// Start proxy as a detached daemon.
    Start {
        #[arg(long)]
        bind: Option<std::net::SocketAddr>,
        #[arg(long)]
        upstream_openai: Option<url::Url>,
        #[arg(long)]
        upstream_anthropic: Option<url::Url>,
        #[arg(long)]
        upstream_gemini: Option<url::Url>,
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long)]
        rulepack: Option<String>,
        #[arg(long)]
        session_ttl: Option<String>,
    },
    /// Stop the detached proxy daemon.
    Stop {
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = "10s")]
        timeout: String,
    },
    /// Show proxy daemon status.
    Status,
    /// Print proxy daemon logs.
    Logs {
        #[arg(long)]
        follow: bool,
    },
    /// Restart proxy daemon using persisted config.
    Restart {
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = "10s")]
        timeout: String,
    },
    /// Install macOS launchd auto-start integration.
    InstallLaunchd,
    /// Uninstall macOS launchd auto-start integration.
    UninstallLaunchd,
    /// Install Linux systemd user auto-start integration.
    InstallSystemdUser,
    /// Uninstall Linux systemd user auto-start integration.
    UninstallSystemdUser,
}

#[derive(Subcommand, Debug)]
enum AuditCmd {
    /// Print filtered audit metadata rows as tab-separated values.
    Query {
        /// SQLite redaction-log database path.
        #[arg(long)]
        audit_db: PathBuf,
        /// Filter by PII class, such as `email`, `name`, or `custom:term`.
        #[arg(long = "class")]
        pii_class: Option<String>,
        /// Filter by source recognizer name.
        #[arg(long)]
        source: Option<String>,
        /// Filter by action, such as `tokenize`, `redact`, or `preserve`.
        #[arg(long)]
        action: Option<String>,
        /// Filter by document kind, such as `text` or `structured`.
        #[arg(long)]
        document_kind: Option<String>,
        /// Include rows created at or after this ISO 8601 timestamp.
        #[arg(long = "from")]
        from_iso8601: Option<String>,
        /// Include rows created at or before this ISO 8601 timestamp.
        #[arg(long = "to")]
        to_iso8601: Option<String>,
        /// Filter by opaque audit session id.
        #[arg(long = "session")]
        session_id: Option<String>,
        /// Include only rows with an ambiguity side-channel record.
        #[arg(long)]
        has_ambiguity: bool,
        /// Filter by ambiguity reason, such as `no-anchor`.
        #[arg(long)]
        ambiguity_reason: Option<String>,
        /// Filter by collision family identifier.
        #[arg(long)]
        collision_family: Option<String>,
        /// Filter by collision variant identifier.
        #[arg(long)]
        collision_variant: Option<String>,
        /// Include only restore telemetry rows.
        #[arg(long)]
        restore_events: bool,
    },
    /// Export filtered audit metadata rows.
    Export {
        /// SQLite redaction-log database path.
        #[arg(long)]
        audit_db: PathBuf,
        /// Export format.
        #[arg(long, value_enum, default_value = "jsonl")]
        format: audit::ExportFormat,
        /// Optional output file. Defaults to stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Filter by PII class, such as `email`, `name`, or `custom:term`.
        #[arg(long = "class")]
        pii_class: Option<String>,
        /// Filter by source recognizer name.
        #[arg(long)]
        source: Option<String>,
        /// Filter by action, such as `tokenize`, `redact`, or `preserve`.
        #[arg(long)]
        action: Option<String>,
        /// Filter by document kind, such as `text` or `structured`.
        #[arg(long)]
        document_kind: Option<String>,
        /// Include rows created at or after this ISO 8601 timestamp.
        #[arg(long = "from")]
        from_iso8601: Option<String>,
        /// Include rows created at or before this ISO 8601 timestamp.
        #[arg(long = "to")]
        to_iso8601: Option<String>,
        /// Filter by opaque audit session id.
        #[arg(long = "session")]
        session_id: Option<String>,
        /// Include only rows with an ambiguity side-channel record.
        #[arg(long)]
        has_ambiguity: bool,
        /// Filter by ambiguity reason, such as `no-anchor`.
        #[arg(long)]
        ambiguity_reason: Option<String>,
        /// Filter by collision family identifier.
        #[arg(long)]
        collision_family: Option<String>,
        /// Filter by collision variant identifier.
        #[arg(long)]
        collision_variant: Option<String>,
        /// Include only restore telemetry rows.
        #[arg(long)]
        restore_events: bool,
    },
    /// Purge audit metadata rows older than an ISO 8601 UTC timestamp.
    Purge {
        /// SQLite redaction-log database path.
        #[arg(long)]
        audit_db: PathBuf,
        /// Purge rows where created_at is before this ISO 8601 UTC timestamp.
        #[arg(long)]
        before: String,
        /// Count matching rows without deleting them.
        #[arg(long, alias = "count")]
        dry_run: bool,
    },
    /// Query safety-net leak metadata without changing redaction-log output.
    SafetyNet {
        #[command(subcommand)]
        command: SafetyNetAuditCmd,
    },
}

#[derive(Subcommand, Debug)]
enum SafetyNetAuditCmd {
    /// Print filtered safety-net metadata rows as tab-separated values.
    Query {
        /// SQLite redaction-log database path.
        #[arg(long)]
        audit_db: PathBuf,
        /// Filter by safety-net leak kind.
        #[arg(long)]
        leak_kind: Option<String>,
        /// Filter by raw backend label.
        #[arg(long)]
        raw_label: Option<String>,
        /// Filter by mapped Gaze class.
        #[arg(long)]
        mapped_class: Option<String>,
        /// Filter by structured field path.
        #[arg(long)]
        field_path: Option<String>,
        /// Include rows created at or after this ISO 8601 timestamp.
        #[arg(long = "from")]
        from_iso8601: Option<String>,
        /// Include rows created at or before this ISO 8601 timestamp.
        #[arg(long = "to")]
        to_iso8601: Option<String>,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SafetyNetKind {
    OpenaiFilter,
    KijiDistilbert,
}

/// v0.8 forward-compatible backend selector.
///
/// `--safety-net-backend` lets adopters pick which observer-only backend runs
/// at Pass-3 when more than one is wired in. Defaults to `openai-filter`,
/// which preserves the v0.6/v0.7 single-backend behavior.
#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SafetyNetBackend {
    OpenaiFilter,
    KijiDistilbert,
}

impl From<SafetyNetKind> for SafetyNetBackend {
    fn from(kind: SafetyNetKind) -> Self {
        match kind {
            SafetyNetKind::OpenaiFilter => Self::OpenaiFilter,
            SafetyNetKind::KijiDistilbert => Self::KijiDistilbert,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenAiFilterOperatingPoint {
    HighRecall,
    Balanced,
    HighPrecision,
}

impl OpenAiFilterOperatingPoint {
    #[cfg(feature = "safety-net-openai")]
    pub(crate) fn as_opf_value(self) -> &'static str {
        match self {
            Self::HighRecall => "high-recall",
            Self::Balanced => "balanced",
            Self::HighPrecision => "high-precision",
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenAiFilterDevice {
    Auto,
    Cpu,
    Cuda,
    Mps,
}

impl OpenAiFilterDevice {
    #[cfg(feature = "safety-net-openai")]
    pub(crate) fn as_opf_value(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Cpu => Some("cpu"),
            Self::Cuda => Some("cuda"),
            Self::Mps => Some("mps"),
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KijiBackend {
    Subprocess,
    Ort,
    #[cfg(feature = "runtime-tract")]
    Tract,
    #[cfg(feature = "runtime-candle")]
    Candle,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KijiDistilbertPrecision {
    Fp32,
    Int8,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SafetyNetMode {
    Strict,
    Tolerant,
    Redact,
    Resolve,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SafetyNetFallback {
    Strict,
    Tolerant,
    Redact,
}

pub(crate) fn dispatch(cli: Cli) -> std::result::Result<(), CliError> {
    match cli.cmd {
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
            safety_net,
            safety_net_backend,
            safety_net_registry,
            safety_net_add,
            openai_filter_command,
            openai_filter_checkpoint,
            openai_filter_operating_point,
            openai_filter_device,
            kiji_backend,
            kiji_distilbert_precision,
            opf_locales,
            opf_command,
            opf_checkpoint,
            kiji_distilbert_command,
            kiji_distilbert_model_dir,
            kiji_distilbert_locales,
            safety_net_timeout_ms,
            safety_net_input_limit_bytes,
            safety_net_mode,
            safety_net_fallback,
        } => clean::run(clean::Args {
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
            safety_net,
            safety_net_backend,
            safety_net_registry,
            safety_net_add,
            openai_filter_command,
            openai_filter_checkpoint,
            openai_filter_operating_point,
            openai_filter_device,
            kiji_backend,
            kiji_distilbert_precision,
            opf_locales,
            opf_command,
            opf_checkpoint,
            kiji_distilbert_command,
            kiji_distilbert_model_dir,
            kiji_distilbert_locales,
            safety_net_timeout_ms,
            safety_net_input_limit_bytes,
            safety_net_mode,
            safety_net_fallback,
        }),
        Cmd::Restore {
            format,
            restore_mode,
            restore_policy,
            telemetry,
            audit_db,
            max_bytes,
        } => restore::run(restore::Args {
            format,
            restore_mode: restore_policy.unwrap_or(restore_mode),
            telemetry,
            audit_db,
            max_bytes,
        }),
        Cmd::Daemon {
            policy,
            safety_net,
            safety_net_backend,
            idle_timeout,
            session_idle_timeout,
            session_cap,
            audit_db,
            locale,
            ner_threshold,
            ner_model_dir,
            ner_locale,
            openai_filter_command,
            openai_filter_checkpoint,
            openai_filter_operating_point,
            openai_filter_device,
            kiji_backend,
            kiji_distilbert_command,
            kiji_distilbert_model_dir,
            kiji_distilbert_locales,
            safety_net_timeout_ms,
            safety_net_input_limit_bytes,
            safety_net_mode,
            safety_net_fallback,
        } => daemon::run(daemon::Args {
            policy,
            safety_net,
            safety_net_backend,
            idle_timeout_secs: idle_timeout,
            session_idle_timeout_secs: session_idle_timeout,
            session_cap,
            audit_db,
            locale,
            ner_threshold,
            ner_model_dir,
            ner_locale,
            openai_filter_command,
            openai_filter_checkpoint,
            openai_filter_operating_point,
            openai_filter_device,
            kiji_backend,
            kiji_distilbert_command,
            kiji_distilbert_model_dir,
            kiji_distilbert_locales,
            safety_net_timeout_ms,
            safety_net_input_limit_bytes,
            safety_net_mode,
            safety_net_fallback,
        }),
        Cmd::Audit { command } => match command {
            AuditCmd::Query {
                audit_db,
                pii_class,
                source,
                action,
                document_kind,
                from_iso8601,
                to_iso8601,
                session_id,
                has_ambiguity,
                ambiguity_reason,
                collision_family,
                collision_variant,
                restore_events,
            } => audit::query(audit::Args {
                audit_db,
                class: pii_class,
                source,
                action,
                document_kind,
                from_iso8601,
                to_iso8601,
                session_id,
                has_ambiguity,
                ambiguity_reason,
                collision_family,
                collision_variant,
                restore_events_only: restore_events,
            }),
            AuditCmd::Export {
                audit_db,
                format,
                output,
                pii_class,
                source,
                action,
                document_kind,
                from_iso8601,
                to_iso8601,
                session_id,
                has_ambiguity,
                ambiguity_reason,
                collision_family,
                collision_variant,
                restore_events,
            } => audit::export(
                audit::Args {
                    audit_db,
                    class: pii_class,
                    source,
                    action,
                    document_kind,
                    from_iso8601,
                    to_iso8601,
                    session_id,
                    has_ambiguity,
                    ambiguity_reason,
                    collision_family,
                    collision_variant,
                    restore_events_only: restore_events,
                },
                format,
                output,
            ),
            AuditCmd::Purge {
                audit_db,
                before,
                dry_run,
            } => audit::purge(audit::PurgeArgs {
                audit_db,
                before,
                dry_run,
            }),
            AuditCmd::SafetyNet { command } => match command {
                SafetyNetAuditCmd::Query {
                    audit_db,
                    leak_kind,
                    raw_label,
                    mapped_class,
                    field_path,
                    from_iso8601,
                    to_iso8601,
                } => audit::query_safety_net(audit::SafetyNetArgs {
                    audit_db,
                    leak_kind,
                    raw_label,
                    mapped_class,
                    field_path,
                    from_iso8601,
                    to_iso8601,
                }),
            },
        },
        #[cfg(feature = "document")]
        Cmd::Document { command } => match command {
            DocumentCmd::Clean {
                input,
                out,
                agent_out,
                owner_out,
            } => document::run_clean(document::CleanArgs {
                input,
                out,
                agent_out,
                owner_out,
            }),
        },
        #[cfg(feature = "index")]
        Cmd::Index { command } => match command {
            IndexCmd::Ingest {
                dir,
                domain,
                index_path,
                on_residual,
            } => index::ingest(index::IngestArgs {
                dir,
                domain,
                index_path,
                on_residual,
            }),
            IndexCmd::Search {
                entity,
                domain,
                class,
                index_path,
            } => index::search(index::SearchArgs {
                entity,
                domain,
                class,
                index_path,
            }),
        },
        #[cfg(feature = "mcp")]
        Cmd::Mcp { command } => match command {
            McpCmd::Install {
                client,
                agents_md,
                dry_run,
                skip_agents_md,
            } => mcp::install(mcp::InstallArgs {
                client,
                agents_md,
                dry_run,
                skip_agents_md,
            }),
            McpCmd::Doctor {
                agents_md,
                strict,
                json,
            } => mcp::doctor(mcp::DoctorArgs {
                agents_md,
                strict,
                json,
            }),
            McpCmd::Serve {
                manifest_dir,
                max_file_size,
            } => mcp::serve(mcp::ServeArgs {
                manifest_dir,
                max_file_size,
            }),
            McpCmd::Bridge {
                config,
                session_dir,
                dry_run,
                print_tools,
            } => mcp::bridge(mcp::BridgeArgs {
                config,
                session_dir,
                dry_run,
                print_tools,
            }),
        },
        #[cfg(feature = "proxy")]
        Cmd::Proxy { command } => match command {
            ProxyCmd::Serve {
                bind,
                upstream_openai,
                upstream_anthropic,
                upstream_gemini,
                policy,
                rulepack,
                session_ttl,
                foreground_daemon,
            } => proxy::serve(proxy::ServeArgs {
                bind,
                upstream_openai,
                upstream_anthropic,
                upstream_gemini,
                policy,
                rulepack,
                session_ttl,
                foreground_daemon,
            }),
            ProxyCmd::Start {
                bind,
                upstream_openai,
                upstream_anthropic,
                upstream_gemini,
                policy,
                rulepack,
                session_ttl,
            } => proxy::start(proxy::StartArgs {
                bind,
                upstream_openai,
                upstream_anthropic,
                upstream_gemini,
                policy,
                rulepack,
                session_ttl,
            }),
            ProxyCmd::Stop { force, timeout } => proxy::stop(proxy::StopArgs { force, timeout }),
            ProxyCmd::Status => proxy::status(),
            ProxyCmd::Logs { follow } => proxy::logs(follow),
            ProxyCmd::Restart { force, timeout } => {
                proxy::restart(proxy::RestartArgs { force, timeout })
            }
            ProxyCmd::InstallLaunchd => proxy::install_launchd(),
            ProxyCmd::UninstallLaunchd => proxy::uninstall_launchd(),
            ProxyCmd::InstallSystemdUser => proxy::install_systemd_user(),
            ProxyCmd::UninstallSystemdUser => proxy::uninstall_systemd_user(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_filter_device_defaults_to_auto() {
        let cli = Cli::parse_from(["gaze", "clean"]);

        let Cmd::Clean {
            openai_filter_device,
            ..
        } = cli.cmd
        else {
            unreachable!("expected clean command");
        };

        assert_eq!(openai_filter_device, OpenAiFilterDevice::Auto);
    }
}
