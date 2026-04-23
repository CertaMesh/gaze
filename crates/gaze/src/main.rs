//! Gaze CLI — pipe-mode `clean` / `restore` for LLM-facing integrations.
//!
//! See `docs/roadmap/v0.3/cli.md` for the design spec and
//! `docs/roadmap/v0.3/laravel.md` for the host-side integration contract.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "gaze", version, about = "Channel-agnostic PII redaction for LLM pipes")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

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
        /// Persistent session TTL in seconds. Default 24h — matches typical queue retention.
        #[arg(long, default_value_t = 86_400)]
        session_ttl: u64,
    },
    /// Read `{session_blob, text}` JSON from stdin; emit `{text}` JSON to stdout.
    Restore {
        /// Output format. Only `json` is supported today.
        #[arg(long, default_value = "json")]
        format: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Clean { .. } => {
            eprintln!(r#"{{"error":"NotImplemented","exit":3}}"#);
            ExitCode::from(3)
        }
        Cmd::Restore { .. } => {
            eprintln!(r#"{{"error":"NotImplemented","exit":3}}"#);
            ExitCode::from(3)
        }
    }
}
