//! Ghostwriter CLI — sanitize and restore JSON requests over stdin/stdout.

use std::io::{self, Read, Write};

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};

use ghostwriter::{restore, sanitize, RestoreRequest, SanitizeRequest};

#[derive(Parser, Debug)]
#[command(name = "ghostwriter", version, about = "Deterministic PII sanitization for LLM prompts")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Read a SanitizeRequest JSON from stdin; write SanitizeResponse JSON to stdout.
    Sanitize,
    /// Read a RestoreRequest JSON from stdin; write RestoreResponse JSON to stdout.
    Restore,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Sanitize => run_sanitize(),
        Cmd::Restore => run_restore(),
    }
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf)
}

fn run_sanitize() -> Result<()> {
    let raw = read_stdin()?;
    let req: SanitizeRequest =
        serde_json::from_str(&raw).context("parsing SanitizeRequest JSON")?;
    let resp = sanitize(req).map_err(|e| anyhow::anyhow!("sanitize failed: {e}"))?;
    let json = serde_json::to_string(&resp).context("serializing SanitizeResponse")?;
    writeln!(io::stdout(), "{json}").context("writing stdout")?;
    Ok(())
}

fn run_restore() -> Result<()> {
    let raw = read_stdin()?;
    let req: RestoreRequest =
        serde_json::from_str(&raw).context("parsing RestoreRequest JSON")?;
    let resp = restore(req).map_err(|e| anyhow::anyhow!("restore failed: {e}"))?;
    let json = serde_json::to_string(&resp).context("serializing RestoreResponse")?;
    writeln!(io::stdout(), "{json}").context("writing stdout")?;
    Ok(())
}
