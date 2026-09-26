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
        findings.extend(scan_with_gaze_clean(file, &mask_allowlisted_urls(&text)?)?);
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

/// Public URLs release text may cite verbatim, as `(host, path regex)`. The host must match
/// exactly (never as a suffix, so `semver.org.example` and `evilsemver.org` do not pass). The
/// regex is anchored and must match everything after the host (path and fragment), so a URL on an
/// allowlisted host cannot carry free text past the scrub, such as `https://semver.org/<IBAN>`.
const PUBLIC_URL_ALLOWLIST: &[(&str, &str)] = &[
    (
        "github.com",
        r"^/CertaMesh/gaze(/(pull|issues)/\d+|/releases(/tag/v\d+\.\d+\.\d+)?)?$",
    ),
    (
        "semver.org",
        r"^(/|/spec/v\d+\.\d+\.\d+\.html)?(#spec-item-\d+)?$",
    ),
];

/// Blanks every allowlisted URL with spaces before detection, keeping byte offsets. Any other URL,
/// including a lookalike, still reaches `gaze clean` and fails the gate.
fn mask_allowlisted_urls(text: &str) -> Result<String> {
    let url = Regex::new(r#"https?://[^\s<>()\[\]"'`]+"#)?;
    let mut masked = text.to_string();
    for hit in url.find_iter(text) {
        let candidate = hit
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if is_allowlisted_public_url(candidate) {
            let range = hit.start()..hit.start() + candidate.len();
            masked.replace_range(range, &" ".repeat(candidate.len()));
        }
    }
    Ok(masked)
}

fn is_allowlisted_public_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, path) = rest.split_at(authority_end);
    PUBLIC_URL_ALLOWLIST
        .iter()
        .any(|(allowed_host, path_pattern)| {
            host == *allowed_host
                && Regex::new(path_pattern)
                    .expect("allowlist path patterns are valid")
                    .is_match(path)
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_the_project_repo_and_semver_only() {
        for url in [
            "https://github.com/CertaMesh/gaze",
            "https://github.com/CertaMesh/gaze/pull/203",
            "https://github.com/CertaMesh/gaze/issues/12",
            "https://github.com/CertaMesh/gaze/releases",
            "https://github.com/CertaMesh/gaze/releases/tag/v0.15.1",
            "https://semver.org",
            "https://semver.org/",
            "https://semver.org/spec/v2.0.0.html#spec-item-4",
        ] {
            assert!(is_allowlisted_public_url(url), "{url} must be allowed");
        }
    }

    #[test]
    fn allowlist_refuses_lookalikes_and_other_urls() {
        for url in [
            "http://semver.org/spec/v2.0.0.html",
            "https://semver.org.example/spec",
            "https://evilsemver.org/",
            "https://www.semver.org/",
            "https://github.com/CertaMesh/gaze-fork/pull/1",
            "https://github.com/CertaMesh/gazette",
            "https://github.com/CertaMesh",
            "https://github.com/other/gaze",
            "https://github.com.example/CertaMesh/gaze",
            "https://github.com@example.org/CertaMesh/gaze",
            "https://github.com:8443/CertaMesh/gaze",
            "https://github.com/CertaMesh/gaze/issues?author=someone",
            "https://github.com/CertaMesh/gaze/blob/main/a%40b",
            "https://example.org/",
            // Free text in the path or fragment of an allowlisted host.
            "https://semver.org/DE89370400440532013000",
            "https://semver.org/spec/v2.0.0.html#4915112345678",
            "https://semver.org/spec/v2.0.0.html#spec-item-4-jane.doe",
            "https://github.com/CertaMesh/gaze/pull/7/jane.doe",
            "https://github.com/CertaMesh/gaze/issues/jane.doe",
            "https://github.com/CertaMesh/gaze/pull/+4915112345678",
            "https://github.com/CertaMesh/gaze/tree/DE89370400440532013000",
            "https://github.com/CertaMesh/gaze/releases/tag/v1.2.3-DE89370400440532013000",
        ] {
            assert!(!is_allowlisted_public_url(url), "{url} must be refused");
        }
    }

    #[test]
    fn masking_blanks_only_allowlisted_urls_and_keeps_offsets() {
        let text = "See https://semver.org/spec/v2.0.0.html, [pr](https://github.com/CertaMesh/gaze/pull/7) and https://semver.org.example/x.";
        let masked = mask_allowlisted_urls(text).unwrap();
        assert_eq!(masked.len(), text.len());
        assert!(!masked.contains("semver.org/spec"));
        assert!(!masked.contains("CertaMesh"));
        assert!(masked.contains("See "));
        assert!(masked.contains(", [pr]("));
        assert!(masked.contains("https://semver.org.example/x."));
    }
}
