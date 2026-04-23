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
        /// Persistent session TTL in seconds. Default 24h — matches typical queue retention.
        #[arg(long, default_value_t = 86_400)]
        session_ttl: u64,
        /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
    },
    /// Read `{session_blob, text}` JSON from stdin; emit `{text}` JSON to stdout.
    Restore {
        /// Output format. Only `json` is supported today.
        #[arg(long, default_value = "json")]
        format: String,
        /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
        #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
        max_bytes: u64,
    },
}

/// Structured CLI error. Each variant maps to an exit code; only the variant
/// name reaches stderr so raw input or plaintext blob entries never leak into
/// caller logs (see docs/roadmap/v0.3/cli.md "Stderr discipline").
#[derive(Debug)]
enum CliError {
    StdinParse,
    EmptyInput,
    InputTooLarge,
    InvalidEncoding,
    PolicyConfig,
    UnknownToken,
    InvalidSignature,
    InvalidBlobVersion,
    #[allow(dead_code)] // Reserved — emitted once solo #4 lands library TTL enforcement.
    BlobExpired,
    Pipeline,
    Io,
    #[allow(dead_code)] // Emitted once solo #3 lands policy.toml loader with file-open path.
    PolicyOpen,
}

impl CliError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::StdinParse
            | Self::EmptyInput
            | Self::InputTooLarge
            | Self::InvalidEncoding => 1,
            Self::PolicyConfig => 2,
            Self::UnknownToken
            | Self::InvalidSignature
            | Self::InvalidBlobVersion
            | Self::BlobExpired
            | Self::Pipeline => 3,
            Self::Io | Self::PolicyOpen => 4,
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            Self::StdinParse => "StdinParse",
            Self::EmptyInput => "EmptyInput",
            Self::InputTooLarge => "InputTooLarge",
            Self::InvalidEncoding => "InvalidEncoding",
            Self::PolicyConfig => "PolicyConfig",
            Self::UnknownToken => "UnknownToken",
            Self::InvalidSignature => "InvalidSignature",
            Self::InvalidBlobVersion => "InvalidBlobVersion",
            Self::BlobExpired => "BlobExpired",
            Self::Pipeline => "Pipeline",
            Self::Io => "Io",
            Self::PolicyOpen => "PolicyOpen",
        }
    }

    fn emit_stderr(&self) {
        eprintln!(
            r#"{{"error":"{}","exit":{}}}"#,
            self.variant_name(),
            self.exit_code()
        );
    }
}

/// Install a panic hook that prints a sanitized error line and exits 3.
/// Without this, a panic in `ort`, `regex`, or any other dep would leak a raw
/// backtrace to stderr whenever `RUST_BACKTRACE` is set — violating the
/// stderr discipline in docs/roadmap/v0.3/cli.md §"Stderr discipline".
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|_info| {
        eprintln!(r#"{{"error":"Pipeline","exit":3}}"#);
    }));
}

fn main() -> ExitCode {
    install_panic_hook();

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(_) => {
            // clap's default handler would dump usage text to stderr before our
            // sanitizer runs. Route argv errors through the standard stderr line
            // so the host wrapper can parse a variant even on malformed argv.
            CliError::PolicyConfig.emit_stderr();
            return ExitCode::from(CliError::PolicyConfig.exit_code());
        }
    };

    let result = match cli.cmd {
        Cmd::Clean {
            policy,
            format,
            session_ttl,
            max_bytes,
        } => run_clean(policy.as_deref(), &format, session_ttl, max_bytes),
        Cmd::Restore { format, max_bytes } => run_restore(&format, max_bytes),
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
    _policy: Option<&std::path::Path>,
    format: &str,
    session_ttl: u64,
    max_bytes: u64,
) -> std::result::Result<(), CliError> {
    require_json_format(format)?;
    let raw = read_stdin_text(max_bytes)?;

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
        },
    };
    let json = serde_json::to_string(&response).map_err(|_| CliError::Pipeline)?;
    println!("{json}");
    Ok(())
}

fn run_restore(format: &str, _max_bytes: u64) -> std::result::Result<(), CliError> {
    require_json_format(format)?;
    // Restore is implemented in the next step. Keeping the stub error so the
    // scaffold is already wired through the exit-code contract.
    // `max_bytes` will be consumed when the JSON parser is wired in.
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
}
