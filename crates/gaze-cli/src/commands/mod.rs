mod audit;
mod clean;
#[cfg(feature = "document")]
mod document;
#[cfg(feature = "mcp")]
mod mcp;
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
        /// v0.8 backend selector. Defaults to `openai-filter`. When set with
        /// `--safety-net=<kind>`, this flag wins. Lets adopters swap the
        /// Pass-3 backend without re-typing the legacy `--safety-net` value.
        #[arg(long, value_enum, default_value_t = SafetyNetBackend::OpenaiFilter)]
        safety_net_backend: SafetyNetBackend,
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
        /// Path to the local Kiji DistilBERT subprocess command.
        #[arg(long)]
        kiji_distilbert_command: Option<PathBuf>,
        /// Path to the pinned Kiji DistilBERT model directory (must contain
        /// SHA256SUMS, labels.json, model.onnx, tokenizer.json).
        #[arg(long)]
        kiji_distilbert_model_dir: Option<PathBuf>,
        /// Safety-net subprocess timeout in milliseconds.
        #[arg(long, default_value_t = DEFAULT_SAFETY_NET_TIMEOUT_MS)]
        safety_net_timeout_ms: u64,
        /// Maximum clean-text bytes submitted to the safety net.
        #[arg(long, default_value_t = DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES)]
        safety_net_input_limit_bytes: usize,
        /// Safety-net handling mode for suspected leaks.
        #[arg(long, value_enum, default_value_t = SafetyNetMode::Strict)]
        safety_net_mode: SafetyNetMode,
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
    /// Query, export, or maintain redaction-log metadata without reading raw PII payloads.
    Audit {
        #[command(subcommand)]
        command: AuditCmd,
    },
    /// Ingest a document (PNG/JPG/PDF) into a SafeBundle (clean.md + manifest + report).
    ///
    /// Requires the binary to be built with `--features document`.
    #[cfg(feature = "document")]
    Document {
        #[command(subcommand)]
        command: DocumentCmd,
    },
    /// Install, diagnose, or run the Gaze MCP stdio server.
    ///
    /// Requires the binary to be built with `--features mcp`.
    #[cfg(feature = "mcp")]
    Mcp {
        #[command(subcommand)]
        command: McpCmd,
    },
}

#[cfg(feature = "document")]
#[derive(Subcommand, Debug)]
enum DocumentCmd {
    /// Run OCR + Gaze redact on `<input>`, write `clean.md`, `manifest.json`,
    /// and `report.json` to `--out`.
    Clean {
        /// Source file. Must be `.png`, `.jpg`, `.jpeg`, or `.pdf` (single-page).
        input: PathBuf,
        /// Output directory. Created if missing.
        #[arg(long)]
        out: PathBuf,
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
pub(crate) enum SafetyNetMode {
    Strict,
    Tolerant,
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
            openai_filter_command,
            openai_filter_checkpoint,
            openai_filter_operating_point,
            openai_filter_device,
            kiji_distilbert_command,
            kiji_distilbert_model_dir,
            safety_net_timeout_ms,
            safety_net_input_limit_bytes,
            safety_net_mode,
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
            openai_filter_command,
            openai_filter_checkpoint,
            openai_filter_operating_point,
            openai_filter_device,
            kiji_distilbert_command,
            kiji_distilbert_model_dir,
            safety_net_timeout_ms,
            safety_net_input_limit_bytes,
            safety_net_mode,
        }),
        Cmd::Restore {
            format,
            restore_mode,
            max_bytes,
        } => restore::run(restore::Args {
            format,
            restore_mode,
            max_bytes,
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
            DocumentCmd::Clean { input, out } => {
                document::run_clean(document::CleanArgs { input, out })
            }
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
