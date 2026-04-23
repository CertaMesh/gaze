//! Gaze CLI — pipe-mode `clean` / `restore` for LLM-facing integrations.
//!
//! See `docs/roadmap/v0.3/cli.md` for the design spec and
//! `docs/roadmap/v0.3/laravel.md` for the host-side integration contract.

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use clap::{Parser, Subcommand};
use serde::Serialize;

use gaze::{
    Action, ClassRule, DefaultRule, DocumentKind, Pipeline, PiiClass, RawDocument,
    RedactionEntry, RedactionLogger, RegexDetector, Result as GazeResult, Scope, Session,
    SensitiveSnapshot,
};

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

/// Structured CLI error. Each variant maps to an exit code; only the variant
/// name reaches stderr so raw input or plaintext blob entries never leak into
/// caller logs (see docs/roadmap/v0.3/laravel.md "active stderr sanitization").
#[derive(Debug)]
enum CliError {
    StdinParse,
    PolicyConfig,
    Pipeline,
    Io,
}

impl CliError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::StdinParse => 1,
            Self::PolicyConfig => 2,
            Self::Pipeline => 3,
            Self::Io => 4,
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            Self::StdinParse => "StdinParse",
            Self::PolicyConfig => "PolicyConfig",
            Self::Pipeline => "Pipeline",
            Self::Io => "Io",
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Clean {
            policy,
            format,
            session_ttl,
        } => run_clean(policy.as_deref(), &format, session_ttl),
        Cmd::Restore { format } => run_restore(&format),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let payload = format!(
                r#"{{"error":"{}","exit":{}}}"#,
                err.variant_name(),
                err.exit_code()
            );
            eprintln!("{payload}");
            ExitCode::from(err.exit_code())
        }
    }
}

fn read_stdin() -> std::result::Result<String, CliError> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf).map_err(|_| CliError::Io)?;
    Ok(buf)
}

fn require_json_format(format: &str) -> std::result::Result<(), CliError> {
    if format == "json" {
        Ok(())
    } else {
        Err(CliError::PolicyConfig)
    }
}

fn run_clean(
    _policy: Option<&std::path::Path>,
    format: &str,
    session_ttl: u64,
) -> std::result::Result<(), CliError> {
    require_json_format(format)?;
    let raw = read_stdin()?;

    let counter = Arc::new(CountingLogger::default());
    let pipeline = build_stub_pipeline(Arc::clone(&counter) as Arc<dyn RedactionLogger>)
        .map_err(|_| CliError::PolicyConfig)?;

    let session = Session::new(Scope::Persistent {
        ttl: Duration::from_secs(session_ttl),
    })
    .map_err(|_| CliError::Pipeline)?;

    let clean_doc = pipeline
        .redact(&session, RawDocument::Text(raw))
        .map_err(|_| CliError::Pipeline)?;

    let clean_text = match clean_doc {
        gaze::CleanDocument::Text(text) => text,
        gaze::CleanDocument::Structured(_) => return Err(CliError::Pipeline),
    };

    let snapshot: SensitiveSnapshot = session.export().map_err(|_| CliError::Pipeline)?;
    let session_blob = BASE64.encode(snapshot.into_bytes());

    let response = CleanResponse {
        clean_text,
        session_blob,
        stats: Stats {
            detections: counter.detections.load(Ordering::Relaxed),
        },
    };
    let json = serde_json::to_string(&response).map_err(|_| CliError::Pipeline)?;
    println!("{json}");
    Ok(())
}

fn run_restore(format: &str) -> std::result::Result<(), CliError> {
    require_json_format(format)?;
    // Restore is implemented in the next step. Keeping the stub error so the
    // scaffold is already wired through the exit-code contract.
    Err(CliError::Pipeline)
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
        if !entry.conflict_loser && entry.document_kind == DocumentKind::Text {
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
}
