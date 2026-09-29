use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::repo::repo_root;

/// Private orchestration state (todo, scratchpad, process and audit ids) must not leak into
/// public repository text. GitHub issue and PR numbers (`#723`) are fine and never match.
const PATTERN: &str = r"(?i)\b(?:solo\s+(?:todos?|scratchpads?|process(?:es)?|\d{4,5})|todos?\s+#?\d{3,4}|scratchpads?\s+#?\d{4,5}|audit\s+#?\d{4,5})\b";

/// Extensions of prose and source files. Data files (`json`, `lock`) carry hashes and corpus ids
/// that look like ids, so they are out of scope.
const TEXT_EXTENSIONS: &[&str] = &["md", "rs", "toml", "py", "sh", "yml", "yaml", "txt"];

/// Tracked paths that are not public repository text.
const EXCLUDED_PREFIXES: &[&str] = &[".claude/"];

#[derive(Debug, PartialEq, Eq)]
struct Finding {
    file: PathBuf,
    line: usize,
    text: String,
}

pub fn run() -> Result<()> {
    let root = repo_root()?;
    let files = tracked_text_files(&root)?;
    if files.is_empty() {
        bail!("no-internal-ids: scanned zero files");
    }
    let mut findings = Vec::new();
    for file in &files {
        // A file that is not UTF-8 is not prose or source; skip it.
        let Ok(text) = fs::read_to_string(root.join(file)) else {
            continue;
        };
        findings.extend(scan_text(file, &text)?);
    }
    if findings.is_empty() {
        println!("no-internal-ids: passed ({} files)", files.len());
        return Ok(());
    }
    eprintln!("no-internal-ids: public text carries private tracker ids");
    for finding in &findings {
        eprintln!(
            "  {}:{}: {}",
            finding.file.display(),
            finding.line,
            finding.text.trim()
        );
    }
    bail!("no-internal-ids failed; delete the id and keep the meaning (PR and issue numbers stay)")
}

fn tracked_text_files(root: &Path) -> Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .context("run git ls-files")?;
    if !output.status.success() {
        bail!("git ls-files failed");
    }
    Ok(String::from_utf8(output.stdout)?
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter(|path| is_scanned(path))
        .map(PathBuf::from)
        .collect())
}

fn is_scanned(path: &str) -> bool {
    !EXCLUDED_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
        && Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| TEXT_EXTENSIONS.contains(&ext))
}

fn scan_text(file: &Path, text: &str) -> Result<Vec<Finding>> {
    let pattern = Regex::new(PATTERN)?;
    Ok(text
        .lines()
        .enumerate()
        .filter(|(_, line)| pattern.is_match(line))
        .map(|(index, line)| Finding {
            file: file.to_path_buf(),
            line: index + 1,
            text: line.to_string(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures are assembled from pieces so this file does not trip its own gate.
    fn join(parts: &[&str]) -> String {
        parts.concat()
    }

    #[test]
    fn no_internal_ids_flags_each_private_id_shape() {
        for shape in [
            join(&["(solo", " todo", " #1234)"]),
            join(&["Solo", " Todo", " 1234: fix"]),
            join(&["solo", " todos", " #1234 and #5678"]),
            join(&["see scratch", "pad", " 12345"]),
            join(&["tracked in to", "do #4003"]),
            join(&["Solo", " process", " 4593"]),
            join(&["(aud", "it 7201 S01-F1)"]),
        ] {
            let findings = scan_text(Path::new("a.rs"), &format!("// {shape}")).unwrap();
            assert_eq!(findings.len(), 1, "{shape} must be flagged");
            assert_eq!(findings[0].line, 1);
        }
    }

    #[test]
    fn no_internal_ids_passes_clean_prose_and_public_numbers() {
        for clean in [
            "Fixed in PR #723, see issue #45.",
            "A todo list of items.",
            "RFC 3849 documentation range",
            "audit S05-F2 finding",
            "scratchpad buffers are reused",
            "the 2026-09-27 ruling",
        ] {
            let findings = scan_text(Path::new("a.md"), clean).unwrap();
            assert!(findings.is_empty(), "{clean} must pass");
        }
    }

    #[test]
    fn no_internal_ids_reports_the_line_of_the_hit() {
        let text = join(&["clean\nalso clean\nbad solo", " todo", " 9999"]);
        let findings = scan_text(Path::new("a.md"), &text).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 3);
    }

    #[test]
    fn no_internal_ids_scans_text_files_only() {
        assert!(is_scanned("CHANGELOG.md"));
        assert!(is_scanned("crates/gaze/src/session.rs"));
        assert!(!is_scanned("docs/reference/benchmarks/comparison.json"));
        assert!(!is_scanned("scripts/bench/uv.lock"));
        assert!(!is_scanned(".claude/skills/release-notes/SKILL.md"));
    }
}
