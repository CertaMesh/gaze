use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, ClapArgs)]
pub(crate) struct Args {
    /// Public text files to check before release publication.
    #[arg(required = true)]
    files: Vec<PathBuf>,
}

#[derive(Debug)]
struct Finding {
    file: PathBuf,
    line: usize,
    column: usize,
    detail: String,
}

#[derive(Debug, Deserialize)]
struct CleanResponse {
    #[serde(default)]
    entries: Vec<CleanEntry>,
}

#[derive(Debug, Deserialize)]
struct CleanEntry {
    class: String,
    token: String,
}

pub(crate) fn run(args: Args) -> Result<()> {
    let mut findings = Vec::new();

    for file in &args.files {
        let text = fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        findings.extend(scan_user_paths(file, &text)?);
        findings.extend(scan_existing_tokens(file, &text)?);
        findings.extend(scan_with_gaze_clean(file, &text)?);
    }

    if findings.is_empty() {
        println!(
            "scrub-public-text: passed ({} file{})",
            args.files.len(),
            if args.files.len() == 1 { "" } else { "s" }
        );
        return Ok(());
    }

    eprintln!("scrub-public-text: public text contains PII-shaped content");
    for finding in &findings {
        eprintln!(
            "  {}:{}:{}: {}",
            finding.file.display(),
            finding.line,
            finding.column,
            finding.detail
        );
    }
    bail!("scrub-public-text failed; scrub or pseudonymize the reported text before release")
}

fn scan_user_paths(file: &Path, text: &str) -> Result<Vec<Finding>> {
    let patterns = [
        (
            Regex::new(r"/Users/[^/\s]+/")?,
            "OS user path `/Users/<name>/`",
        ),
        (
            Regex::new(r"/home/[^/\s]+/")?,
            "OS user path `/home/<name>/`",
        ),
        (
            Regex::new(r"C:\\Users\\[^\\\s]+\\")?,
            "OS user path `C:\\Users\\<name>\\`",
        ),
    ];
    let mut findings = Vec::new();
    for (regex, label) in patterns {
        for hit in regex.find_iter(text) {
            let (line, column) = line_column(text, hit.start());
            findings.push(Finding {
                file: file.to_path_buf(),
                line,
                column,
                detail: label.to_string(),
            });
        }
    }
    Ok(findings)
}

fn scan_existing_tokens(file: &Path, text: &str) -> Result<Vec<Finding>> {
    let token = Regex::new(r"<(?:[0-9a-f]{8}:)?[A-Za-z][A-Za-z0-9:_-]*_\d+>")?;
    Ok(token
        .find_iter(text)
        .map(|hit| {
            let (line, column) = line_column(text, hit.start());
            Finding {
                file: file.to_path_buf(),
                line,
                column,
                detail: format!("existing gaze token {}", hit.as_str()),
            }
        })
        .collect())
}

fn scan_with_gaze_clean(file: &Path, text: &str) -> Result<Vec<Finding>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut child = Command::new(cargo)
        .args(["run", "-q", "-p", "gaze-cli", "--", "clean"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start gaze clean for {}", file.display()))?;

    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(text.as_bytes())
        .with_context(|| format!("failed to send {} to gaze clean", file.display()))?;

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for gaze clean on {}", file.display()))?;
    if !output.status.success() {
        bail!(
            "gaze clean failed for {}: {}",
            file.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let response: CleanResponse = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "failed to parse gaze clean JSON for {}: {}",
            file.display(),
            String::from_utf8_lossy(&output.stdout)
        )
    })?;

    Ok(response
        .entries
        .into_iter()
        .map(|entry| Finding {
            file: file.to_path_buf(),
            line: 1,
            column: 1,
            detail: format!("gaze clean emitted {} token {}", entry.class, entry.token),
        })
        .collect())
}

fn line_column(text: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (idx, ch) in text.char_indices() {
        if idx >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}
