mod audit;
mod clean;
mod restore;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::error::{CliError, RestoreMode};
use crate::io::DEFAULT_MAX_BYTES;

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
    /// Query or export redaction-log metadata without reading raw PII payloads.
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
    },
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
            } => audit::query(audit::Args {
                audit_db,
                class: pii_class,
                source,
                action,
                document_kind,
                from_iso8601,
                to_iso8601,
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
            } => audit::export(
                audit::Args {
                    audit_db,
                    class: pii_class,
                    source,
                    action,
                    document_kind,
                    from_iso8601,
                    to_iso8601,
                },
                format,
                output,
            ),
        },
    }
}
