mod audit;
mod clean;
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
        /// Optional observer-only privacy safety net.
        #[arg(long, value_enum)]
        safety_net: Option<SafetyNetKind>,
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
            openai_filter_command,
            openai_filter_checkpoint,
            openai_filter_operating_point,
            openai_filter_device,
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
            openai_filter_command,
            openai_filter_checkpoint,
            openai_filter_operating_point,
            openai_filter_device,
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
            } => audit::query(audit::Args {
                audit_db,
                class: pii_class,
                source,
                action,
                document_kind,
                from_iso8601,
                to_iso8601,
                session_id,
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
