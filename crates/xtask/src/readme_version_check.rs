use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::{ensure_test_exists, run_behavioral_test, BehavioralTest};

const README_VERSION_CHECK_TESTS: &[BehavioralTest] = &[
    BehavioralTest {
        package: "xtask",
        test_target: None,
        name: "readme_version_check::tests::readme_version_check_accepts_matching_fixture",
    },
    BehavioralTest {
        package: "xtask",
        test_target: None,
        name: "readme_version_check::tests::readme_version_check_rejects_mismatched_fixture",
    },
];

pub fn run() -> Result<()> {
    println!(
        "readme-version-check: checking {} fixture tests",
        README_VERSION_CHECK_TESTS.len()
    );
    for test in README_VERSION_CHECK_TESTS {
        ensure_test_exists(*test)?;
    }
    for test in README_VERSION_CHECK_TESTS {
        run_behavioral_test("readme-version-check", *test)?;
    }

    let metadata = cargo_metadata()?;
    let workspace_members: HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();

    let mut checked = 0_usize;
    for package in metadata
        .packages
        .iter()
        .filter(|package| workspace_members.contains(package.id.as_str()))
        .filter(|package| package.name != "xtask")
        .filter(|package| {
            package
                .publish
                .as_ref()
                .is_none_or(|publish| !publish.is_empty())
        })
    {
        let readme = package.readme_path()?;
        let contents = std::fs::read_to_string(&readme)
            .with_context(|| format!("{}: failed to read {}", package.name, readme.display()))?;
        let mismatches = validate_readme_versions(&package.name, &package.version, &contents);
        if !mismatches.is_empty() {
            for mismatch in mismatches {
                eprintln!(
                    "readme-version-check: {} README pins {} but Cargo.toml version is {} ({})",
                    package.name, mismatch.found, package.version, mismatch.occurrence
                );
            }
            bail!(
                "readme-version-check: stale README version pin for {}",
                package.name
            );
        }
        checked += 1;
    }

    println!("readme-version-check: passed ({checked} published crates)");
    Ok(())
}

fn cargo_metadata() -> Result<Metadata> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("failed to run cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("failed to parse cargo metadata")
}

fn validate_readme_versions(crate_name: &str, expected: &str, contents: &str) -> Vec<Mismatch> {
    find_version_occurrences(crate_name, contents)
        .into_iter()
        .filter(|occurrence| occurrence.found != expected)
        .collect()
}

fn find_version_occurrences(crate_name: &str, contents: &str) -> Vec<Mismatch> {
    let mut occurrences = Vec::new();
    let equals_prefix = format!("{crate_name} = \"");
    let colon_prefix = format!("{crate_name}:");

    for (line_idx, line) in contents.lines().enumerate() {
        collect_quoted_versions(
            crate_name,
            &equals_prefix,
            line_idx + 1,
            line,
            &mut occurrences,
        );
        collect_colon_versions(
            crate_name,
            &colon_prefix,
            line_idx + 1,
            line,
            &mut occurrences,
        );
    }

    occurrences
}

fn collect_quoted_versions(
    crate_name: &str,
    prefix: &str,
    line_no: usize,
    line: &str,
    occurrences: &mut Vec<Mismatch>,
) {
    let mut rest = line;
    while let Some(start) = rest.find(prefix) {
        let after_prefix = &rest[start + prefix.len()..];
        let Some(end) = after_prefix.find('"') else {
            break;
        };
        let version = &after_prefix[..end];
        if looks_like_workspace_version(version) {
            occurrences.push(Mismatch {
                found: version.to_string(),
                occurrence: format!("{crate_name} = \"{version}\" at line {line_no}"),
            });
        }
        rest = &after_prefix[end + 1..];
    }
}

fn collect_colon_versions(
    crate_name: &str,
    prefix: &str,
    line_no: usize,
    line: &str,
    occurrences: &mut Vec<Mismatch>,
) {
    let mut rest = line;
    while let Some(start) = rest.find(prefix) {
        let after_prefix = &rest[start + prefix.len()..];
        let version_len = after_prefix
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+')))
            .unwrap_or(after_prefix.len());
        let version = &after_prefix[..version_len];
        if looks_like_workspace_version(version) {
            occurrences.push(Mismatch {
                found: version.to_string(),
                occurrence: format!("{crate_name}:{version} at line {line_no}"),
            });
        }
        rest = &after_prefix[version_len..];
    }
}

fn looks_like_workspace_version(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("0.") else {
        return false;
    };
    let mut parts = rest.splitn(3, '.');
    matches!(
        (parts.next(), parts.next()),
        (Some(minor), Some(patch))
            if !minor.is_empty()
                && minor.chars().all(|ch| ch.is_ascii_digit())
                && patch.chars().next().is_some_and(|ch| ch.is_ascii_digit())
    )
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    readme: Option<PathBuf>,
    publish: Option<Vec<String>>,
}

impl Package {
    fn readme_path(&self) -> Result<PathBuf> {
        let package_dir = self
            .manifest_path
            .parent()
            .with_context(|| format!("{}: Cargo.toml has no parent directory", self.name))?;
        let readme = self
            .readme
            .as_deref()
            .unwrap_or_else(|| Path::new("README.md"));
        let path = if readme.is_absolute() {
            readme.to_path_buf()
        } else {
            package_dir.join(readme)
        };
        if !path.exists() {
            bail!(
                "{}: published crate README missing at {}",
                self.name,
                path.display()
            );
        }
        Ok(path)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Mismatch {
    found: String,
    occurrence: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_version_check_accepts_matching_fixture() {
        let readme = r#"
```toml
[dependencies]
gaze-document = "0.9.0"
```

container image: gaze-document:0.9.0
"#;

        assert!(validate_readme_versions("gaze-document", "0.9.0", readme).is_empty());
    }

    #[test]
    fn readme_version_check_rejects_mismatched_fixture() {
        let readme = r#"
```toml
[dependencies]
gaze-document = "0.7.2"
```

container image: gaze-document:0.8.0
"#;

        let mismatches = validate_readme_versions("gaze-document", "0.9.0", readme);
        assert_eq!(mismatches.len(), 2);
        assert_eq!(mismatches[0].found, "0.7.2");
        assert_eq!(mismatches[1].found, "0.8.0");
    }
}
