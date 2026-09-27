use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, ClapArgs)]
pub(crate) struct Args {
    /// Include published Markdown and one hop of its local docs links.
    #[arg(long)]
    published: bool,
    /// Public text files to check before release publication.
    #[arg(required_unless_present = "published")]
    files: Vec<PathBuf>,
}

pub(crate) struct PublishedFiles {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) crate_readmes: usize,
    pub(crate) published_crates: usize,
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
    let root = crate::repo::repo_root()?;
    let mut files = BTreeSet::new();
    files.extend(args.files);
    if args.published {
        let published = published_files(&root)?;
        if published.crate_readmes != published.published_crates {
            bail!("published crate README count does not match published crate count");
        }
        files.extend(published.paths);
    }

    for file in &files {
        let text = fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        findings.extend(scan_user_paths(file, &text)?);
        findings.extend(scan_existing_tokens(file, &text)?);
        let masked = mask_known_loopback_bind(&mask_allowlisted_urls(&text)?)?;
        findings.extend(scan_with_gaze_clean(file, &masked)?);
    }

    if findings.is_empty() {
        println!(
            "scrub-public-text: passed ({} file{})",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
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

/// Resolve the files released directly and current docs linked from them.
/// Keep this set shared between PR tests and tag-time publication checks.
pub(crate) fn published_files(root: &Path) -> Result<PublishedFiles> {
    let mut paths = BTreeSet::from([
        PathBuf::from("CHANGELOG.md"),
        PathBuf::from("UPGRADE.md"),
        PathBuf::from("README.md"),
    ]);
    let readme = Regex::new(r#"(?m)^readme\s*=\s*"([^"]+)""#)?;
    let unpublished = Regex::new(r"(?m)^publish\s*=\s*false\s*$")?;
    let mut crate_readmes = 0;
    let mut published_crates = 0;

    for entry in fs::read_dir(root.join("crates")).context("list workspace crates")? {
        let crate_dir = entry?.path();
        let manifest = crate_dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let manifest_text = fs::read_to_string(&manifest)
            .with_context(|| format!("read {}", manifest.display()))?;
        let is_published = !unpublished.is_match(&manifest_text);
        published_crates += usize::from(is_published);
        let crate_readme = if let Some(captures) = readme.captures(&manifest_text) {
            crate_dir.join(&captures[1])
        } else {
            crate_dir.join("README.md")
        };
        if !crate_readme.is_file() {
            if is_published {
                bail!("published crate has no README: {}", manifest.display());
            }
            continue;
        }
        if is_published {
            crate_readmes += 1;
        }
        paths.insert(crate_readme.strip_prefix(root)?.to_path_buf());
    }

    // Follow only one hop from directly published text, never recurse through docs.
    let cited_doc = Regex::new(r"docs/[A-Za-z0-9_./-]+\.md")?;
    for source in paths.clone() {
        let text = fs::read_to_string(root.join(&source))
            .with_context(|| format!("read {}", source.display()))?;
        for hit in cited_doc.find_iter(&text) {
            let path = PathBuf::from(hit.as_str());
            if path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            {
                bail!("invalid documentation link in {}", source.display());
            }
            if root.join(&path).is_file() {
                paths.insert(path);
            }
        }
    }

    Ok(PublishedFiles {
        paths: paths.into_iter().collect(),
        crate_readmes,
        published_crates,
    })
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
        r"^/CertaMesh/gaze(\.git|#license|/(pull|issues)/\d+|/releases(/tag/v\d+\.\d+\.\d+)?|/actions/workflows/test\.yml(/badge\.svg)?|/stargazers|/labels/good%20first%20issue|/compare/v\d+\.\d+\.\d+(-rc\.\d+)?\.\.\.(HEAD|v\d+\.\d+\.\d+(-rc\.\d+)?))?$",
    ),
    ("github.com", r"^/CertaMesh/gaze-ghostwriter$"),
    ("github.com", r"^/bblanchon/pdfium-binaries$"),
    ("github.com", r"^/openai/privacy-filter$"),
    (
        "github.com",
        r"^/EmpireTwo/gaze/(compare/v\d+\.\d+\.\d+(-rc\.\d+)?\.\.\.v\d+\.\d+\.\d+(-rc\.\d+)?|releases/tag/v\d+\.\d+\.\d+(-rc\.\d+)?)$",
    ),
    ("keepachangelog.com", r"^/en/1\.1\.0/$"),
    ("api.openai.com", r"^/?$"),
    ("api.anthropic.com", r"^/?$"),
    ("generativelanguage.googleapis.com", r"^/?$"),
    (
        "nationalnanpa.com",
        r"^/number_resource_info/555_numbers\.html$",
    ),
    ("huggingface.co", r"^/Wismut/nym-pii-multilingual-small$"),
    (
        "collectables.auspost.com.au",
        r"^/community-and-events/articles/postcodes-turn-50$",
    ),
    ("127.0.0.1:8787", r"^(/v1)?$"),
    (
        "semver.org",
        r"^(/|/spec/v\d+\.\d+\.\d+\.html)?(#spec-item-\d+)?$",
    ),
];

const PUBLIC_CRATE_SLUGS: &[&str] = &[
    "gaze-pii",
    "gaze-types",
    "gaze-audit",
    "gaze-inspection",
    "gaze-recognizers",
    "gaze-assembly",
    "gaze-model-setup",
    "gaze-mcp-core",
    "gaze-mcp-rmcp",
    "gaze-mcp-bridge",
    "gaze-document",
    "gaze-proxy",
    "gaze-proxy-dashboard",
    "gaze-token-bridge",
    "gaze-cli",
];

/// Historical release citations and crate-page source links have exact paths. A generic
/// `blob/...` rule would let arbitrary PII-shaped path segments bypass the scrub.
const PUBLIC_GITHUB_BLOBS: &[&str] = &[
    "/CertaMesh/gaze/blob/main/AGENTS.md#project-north-star",
    "/CertaMesh/gaze/blob/main/LICENSE-APACHE",
    "/CertaMesh/gaze/blob/main/LICENSE-MIT",
    "/CertaMesh/gaze/blob/main/docs/explanation/detection/ner-failclosed.md",
    "/CertaMesh/gaze/blob/main/docs/reference/metrics.md",
    "/CertaMesh/gaze/blob/main/docs/tutorials/getting-started.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025a-cfb3aed-scorecard.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025g-edfb167-scorecard.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025u-bfcf264-scorecard.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-consolidated-post-wave-scorecard.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-government-id-scorecard.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-post-wave-a8f7182-scorecard.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-benchmark.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-class-gap.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-class-gap.md#L32-L58",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9-ner-model-leaderboard.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9-safety-net-benchmark.md",
    "/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9.0-rc1-combined-revalidation.md",
    "/PIInuts/business/blob/main/research/v0.4.4-date-posture.md",
    "/PIInuts/business/blob/main/research/v0.4.4-phonenumber-audit.md",
    "/PIInuts/business/blob/main/research/v0.5-dylint-audit-gate.md",
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

/// The shipped proxy example binds to loopback. Mask only this complete TOML assignment;
/// other IPs and URLs still reach the detector.
fn mask_known_loopback_bind(text: &str) -> Result<String> {
    let bind = Regex::new(r#"(?m)^[ \t]*bind[ \t]*=[ \t]*"127\.0\.0\.1:8787"[ \t]*$"#)?;
    let mut masked = text.to_string();
    for hit in bind.find_iter(text) {
        masked.replace_range(hit.range(), &" ".repeat(hit.len()));
    }
    Ok(masked)
}

fn is_allowlisted_public_url(url: &str) -> bool {
    let (rest, loopback_http) = if let Some(rest) = url.strip_prefix("https://") {
        (rest, false)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (rest, true)
    } else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, path) = rest.split_at(authority_end);
    if loopback_http && host != "127.0.0.1:8787" {
        return false;
    }
    if host == "github.com" && PUBLIC_GITHUB_BLOBS.contains(&path) {
        return true;
    }
    if is_allowlisted_crate_url(host, path) {
        return true;
    }
    PUBLIC_URL_ALLOWLIST
        .iter()
        .any(|(allowed_host, path_pattern)| {
            host == *allowed_host
                && Regex::new(path_pattern)
                    .expect("allowlist path patterns are valid")
                    .is_match(path)
        })
}

fn is_allowlisted_crate_url(host: &str, path: &str) -> bool {
    match host {
        "crates.io" => path.strip_prefix("/crates/").is_some_and(|slug| {
            PUBLIC_CRATE_SLUGS.contains(&slug) || matches!(slug, "pdfium-render" | "rmcp")
        }),
        "docs.rs" => {
            if path == "/regex" {
                return true;
            }
            let Some(slug) = path.strip_prefix('/') else {
                return false;
            };
            let slug = slug.strip_suffix("/badge.svg").unwrap_or(slug);
            PUBLIC_CRATE_SLUGS.contains(&slug)
        }
        "img.shields.io" => {
            if path == "/github/stars/CertaMesh/gaze?style=social" {
                return true;
            }
            path.strip_prefix("/crates/v/")
                .or_else(|| path.strip_prefix("/crates/l/"))
                .and_then(|slug| slug.strip_suffix(".svg"))
                .is_some_and(|slug| PUBLIC_CRATE_SLUGS.contains(&slug))
        }
        _ => false,
    }
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
    fn published_set_covers_crate_readmes_and_one_hop_of_links() {
        let root = crate::repo::repo_root().expect("workspace root");
        let files = published_files(&root).expect("published Markdown");
        assert!(files.published_crates >= 15, "published crate count fell");
        assert_eq!(files.crate_readmes, files.published_crates);
        for path in [
            "docs/explanation/how-gaze-works.md",
            "docs/tutorials/getting-started.md",
            "docs/reference/cli.md",
        ] {
            assert!(files.paths.contains(&PathBuf::from(path)), "missing {path}");
        }
    }

    #[test]
    fn allowlist_accepts_fixed_public_release_links() {
        for url in [
            "https://github.com/CertaMesh/gaze",
            "https://github.com/CertaMesh/gaze/pull/203",
            "https://github.com/CertaMesh/gaze/issues/12",
            "https://github.com/CertaMesh/gaze/releases",
            "https://github.com/CertaMesh/gaze/releases/tag/v0.15.1",
            "https://semver.org",
            "https://semver.org/",
            "https://semver.org/spec/v2.0.0.html#spec-item-4",
            "https://github.com/CertaMesh/gaze/blob/main/docs/reference/metrics.md",
            "https://github.com/CertaMesh/gaze/compare/v0.14.0...HEAD",
            "https://crates.io/crates/gaze-pii",
            "https://docs.rs/gaze-pii/badge.svg",
            "https://docs.rs/regex",
            "https://img.shields.io/crates/v/gaze-pii.svg",
            "https://img.shields.io/github/stars/CertaMesh/gaze?style=social",
            "http://127.0.0.1:8787/v1",
            "https://api.openai.com/",
            "https://api.anthropic.com",
            "https://nationalnanpa.com/number_resource_info/555_numbers.html",
            "https://huggingface.co/Wismut/nym-pii-multilingual-small",
            "https://collectables.auspost.com.au/community-and-events/articles/postcodes-turn-50",
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
            "https://github.com/CertaMesh/gaze/blob/main/docs/123456789.md",
            "https://example.org/",
            "https://auspost.com.au/community-and-events/articles/postcodes-turn-50",
            "https://collectables.auspost.com.au/community-and-events/articles/postcodes-turn-50/x",
            // Free text in the path or fragment of an allowlisted host.
            "https://semver.org/DE89370400440532013000",
            "https://semver.org/spec/v2.0.0.html#4915112345678",
            "https://semver.org/spec/v2.0.0.html#spec-item-4-jane.doe",
            "https://github.com/CertaMesh/gaze/pull/7/jane.doe",
            "https://github.com/CertaMesh/gaze/issues/jane.doe",
            "https://github.com/CertaMesh/gaze/pull/+4915112345678",
            "https://github.com/CertaMesh/gaze/tree/DE89370400440532013000",
            "https://github.com/CertaMesh/gaze/releases/tag/v1.2.3-DE89370400440532013000",
            "https://github.com/CertaMesh/gaze/compare/v0.1.0...vAlice",
            "https://github.com/CertaMesh/gaze/blob/main/docs/alice@example.invalid.md",
            "https://github.com/CertaMesh/gaze/compare/v0.1.0...alice@example.invalid",
            "https://crates.io/crates/gaze-pii/alice@example.invalid",
            "https://crates.io/crates/gaze-alice",
            "https://docs.rs/gaze-pii/alice@example.invalid",
            "https://docs.rs/gaze-alice/badge.svg",
            "https://docs.rs/regex/alice@example.invalid",
            "https://img.shields.io/crates/v/gaze-pii/alice@example.invalid",
            "https://img.shields.io/crates/v/gaze-alice.svg",
            "http://127.0.0.1:8787/alice@example.invalid",
            "https://api.openai.com/alice@example.invalid",
            "https://huggingface.co/Wismut/nym-pii-multilingual-small/alice@example.invalid",
            "http://api.openai.com/",
        ] {
            assert!(!is_allowlisted_public_url(url), "{url} must be refused");
        }
    }

    #[test]
    fn loopback_bind_mask_does_not_hide_other_ip_text() {
        let text = "bind = \"127.0.0.1:8787\"\nupstream = \"http://127.0.0.1:8787/private\"\npeer = \"192.0.2.12\"\n";
        let masked = mask_known_loopback_bind(text).expect("mask loopback bind");
        assert!(!masked.contains("bind = \"127.0.0.1:8787\""));
        assert!(masked.contains("http://127.0.0.1:8787/private"));
        assert!(masked.contains("192.0.2.12"));
        assert_eq!(masked.len(), text.len());
    }

    #[test]
    fn changelog_public_links_fit_the_allowlist() {
        let changelog = include_str!("../../../CHANGELOG.md");
        let url = Regex::new(r#"https?://[^\s<>()\[\]"'`]+"#).expect("URL pattern");
        for hit in url.find_iter(changelog) {
            let candidate = hit
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', '!', '?']);
            assert!(is_allowlisted_public_url(candidate), "{candidate}");
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
