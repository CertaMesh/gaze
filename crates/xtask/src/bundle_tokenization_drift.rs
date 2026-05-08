use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use gaze::{PiiClass, Rulepack, RulepackSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u8 = 1;
const FIXTURE_PATH: &str = "crates/xtask/fixtures/drift-corpus.txt";
const EMBEDDED_RULEPACK_DIR: &str = "crates/gaze-recognizers/embedded";
const SNAPSHOT_DIR: &str = "crates/xtask/snapshots";

#[derive(Debug, Parser)]
pub struct Args {
    /// Rewrite committed bundle tokenization snapshots.
    #[arg(long, alias = "write-baseline")]
    regenerate_baseline: bool,
    /// Require a strict source + CHANGELOG acknowledgement for changed snapshots.
    #[arg(long)]
    verify_ack: bool,
}

pub fn run(args: Args) -> Result<()> {
    let root = std::env::current_dir().context("failed to resolve workspace root")?;
    let bundles = discover_bundles(&root)?;
    if bundles.is_empty() {
        bail!("bundle-tokenization-drift: no bundled rulepacks discovered");
    }

    let corpus = fs::read_to_string(root.join(FIXTURE_PATH))
        .with_context(|| format!("failed to read {FIXTURE_PATH}"))?;
    let fixtures_sha256 = sha256_hex(corpus.as_bytes());

    fs::create_dir_all(root.join(SNAPSHOT_DIR))
        .with_context(|| format!("failed to create {SNAPSHOT_DIR}"))?;

    let mut drifted = Vec::new();
    for bundle in bundles {
        let snapshot_path = root
            .join(SNAPSHOT_DIR)
            .join(format!("{}-no-policy.json", bundle.name));
        if !snapshot_path.exists() && !args.regenerate_baseline {
            bail!(
                "bundle-tokenization-drift: missing snapshot for bundle `{}` at {}",
                bundle.name,
                snapshot_path.display()
            );
        }

        let actual = run_bundle_snapshot(&root, &bundle, &corpus, &fixtures_sha256)?;
        assert_no_raw_value_leak(&actual, &corpus)?;
        let actual_json = serde_json::to_string_pretty(&actual)? + "\n";

        if args.regenerate_baseline {
            fs::write(&snapshot_path, actual_json)
                .with_context(|| format!("failed to write {}", snapshot_path.display()))?;
            println!(
                "bundle-tokenization-drift: wrote {}",
                snapshot_path.display()
            );
            continue;
        }

        let expected_json = fs::read_to_string(&snapshot_path)
            .with_context(|| format!("failed to read {}", snapshot_path.display()))?;
        let expected: Snapshot = serde_json::from_str(&expected_json)
            .with_context(|| format!("failed to parse {}", snapshot_path.display()))?;
        if expected != actual {
            drifted.push(format_snapshot_diff(&bundle.name, &expected, &actual));
        }
    }

    if !drifted.is_empty() {
        bail!(
            "bundle-tokenization-drift: snapshot drift detected\n{}",
            drifted.join("\n\n")
        );
    }

    if args.verify_ack {
        verify_ack(&root)?;
    }
    println!("bundle-tokenization-drift: passed");
    Ok(())
}

#[derive(Debug)]
struct Bundle {
    name: String,
    path: PathBuf,
    rulepack: Rulepack,
    activated_classes: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Snapshot {
    schema_version: u8,
    bundle: String,
    rulepack_version: String,
    fixtures_sha256: String,
    entries: Vec<SnapshotEntry>,
    stats: SnapshotStats,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct SnapshotEntry {
    recognizer_id: String,
    class: String,
    byte_span: ByteSpan,
    family: String,
    token_shape: String,
    count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct ByteSpan {
    start: usize,
    len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SnapshotStats {
    detections: u64,
    classes: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct CleanOutput {
    clean_text: String,
    session_blob: String,
    stats: CleanStats,
}

#[derive(Debug, Deserialize)]
struct CleanStats {
    detections: u64,
}

#[derive(Debug, Deserialize)]
struct RestoreOutput {
    text: String,
}

#[derive(Debug, Deserialize)]
struct AuditRow {
    source: String,
    class: String,
    action: String,
    conflict_loser: bool,
}

#[derive(Debug, Clone)]
struct TokenOccurrence {
    literal: String,
    family: String,
    shape: String,
}

fn discover_bundles(root: &Path) -> Result<Vec<Bundle>> {
    let mut bundles = Vec::new();
    let mut paths = fs::read_dir(root.join(EMBEDDED_RULEPACK_DIR))
        .with_context(|| format!("failed to read {EMBEDDED_RULEPACK_DIR}"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to list {EMBEDDED_RULEPACK_DIR}"))?;
    paths.sort();

    for path in paths {
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let rulepack = Rulepack::load(RulepackSource::Embedded(Box::leak(raw.into_boxed_str())))
            .with_context(|| format!("failed to parse {}", path.display()))?;
        if rulepack.recognizers.is_empty() {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| anyhow!("invalid bundle filename {}", path.display()))?
            .to_string();
        let activated_classes = rulepack
            .activated_classes()
            .into_iter()
            .map(class_to_snapshot_string)
            .collect();
        bundles.push(Bundle {
            name,
            path,
            rulepack,
            activated_classes,
        });
    }
    Ok(bundles)
}

fn run_bundle_snapshot(
    root: &Path,
    bundle: &Bundle,
    corpus: &str,
    fixtures_sha256: &str,
) -> Result<Snapshot> {
    let temp = tempfile::tempdir().context("failed to create drift tempdir")?;
    let audit_db = temp.path().join(format!("{}-audit.sqlite", bundle.name));

    let clean = run_with_stdin(
        root,
        Command::new("cargo")
            .args([
                "run",
                "-q",
                "-p",
                "gaze-cli",
                "--",
                "clean",
                "--rulepack-bundled",
                &bundle.name,
                "--audit-db",
            ])
            .arg(&audit_db),
        corpus,
    )
    .with_context(|| format!("failed to run clean for bundle `{}`", bundle.name))?;
    let clean: CleanOutput =
        serde_json::from_slice(&clean).context("failed to parse gaze clean JSON")?;

    let restored = restore_text(root, &clean.session_blob, &clean.clean_text, true)
        .context("failed to restore full clean_text")?;
    if restored != corpus {
        bail!(
            "bundle-tokenization-drift: restore mismatch for bundle `{}`",
            bundle.name
        );
    }

    let rows = export_audit_rows(root, &audit_db)?
        .into_iter()
        .filter(|row| row.action == "tokenize" && !row.conflict_loser)
        .collect::<Vec<_>>();
    let tokens = extract_tokens(&clean.clean_text)?;
    if rows.len() != tokens.len() {
        bail!(
            "bundle-tokenization-drift: bundle `{}` emitted {} audit rows but {} tokens",
            bundle.name,
            rows.len(),
            tokens.len()
        );
    }
    if clean.stats.detections != rows.len() as u64 {
        bail!(
            "bundle-tokenization-drift: bundle `{}` stats.detections={} but audit rows={}",
            bundle.name,
            clean.stats.detections,
            rows.len()
        );
    }

    let mut entries = BTreeMap::<(String, String, ByteSpan, String, String), u64>::new();
    let mut classes = BTreeMap::<String, u64>::new();
    for (row, token) in rows.iter().zip(tokens.iter()) {
        let class = canonical_audit_class(&row.class);
        if !bundle.activated_classes.contains(&class) {
            bail!(
                "bundle-tokenization-drift: bundle `{}` emitted class `{}` outside activated_classes {:?} from {}",
                bundle.name,
                class,
                bundle.activated_classes,
                bundle.path.display()
            );
        }
        let raw = restore_text(root, &clean.session_blob, &token.literal, false)
            .with_context(|| format!("failed to restore token `{}`", token.shape))?;
        let span = unique_span(corpus, &raw)?;
        *classes.entry(class.clone()).or_insert(0) += 1;
        *entries
            .entry((
                row.source.clone(),
                class,
                span,
                token.family.clone(),
                token.shape.clone(),
            ))
            .or_insert(0) += 1;
    }

    Ok(Snapshot {
        schema_version: SCHEMA_VERSION,
        bundle: bundle.name.clone(),
        rulepack_version: bundle.rulepack.rulepack_version.clone(),
        fixtures_sha256: fixtures_sha256.to_string(),
        entries: entries
            .into_iter()
            .map(
                |((recognizer_id, class, byte_span, family, token_shape), count)| SnapshotEntry {
                    recognizer_id,
                    class,
                    byte_span,
                    family,
                    token_shape,
                    count,
                },
            )
            .collect(),
        stats: SnapshotStats {
            detections: clean.stats.detections,
            classes,
        },
    })
}

fn run_with_stdin(root: &Path, command: &mut Command, stdin: &str) -> Result<Vec<u8>> {
    let mut child = command
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn command")?;
    child
        .stdin
        .as_mut()
        .context("failed to open child stdin")?
        .write_all(stdin.as_bytes())
        .context("failed to write child stdin")?;
    let output = child
        .wait_with_output()
        .context("failed to wait for command")?;
    if !output.status.success() {
        bail!(
            "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

fn run_stdout(root: &Path, command: &mut Command) -> Result<Vec<u8>> {
    let output = command
        .current_dir(root)
        .output()
        .context("failed to run command")?;
    if !output.status.success() {
        bail!(
            "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

fn restore_text(root: &Path, session_blob: &str, text: &str, tolerant: bool) -> Result<String> {
    let body = serde_json::json!({ "session_blob": session_blob, "text": text }).to_string();
    let mut command = Command::new("cargo");
    command.args(["run", "-q", "-p", "gaze-cli", "--", "restore"]);
    if tolerant {
        command.args(["--restore-mode", "tolerant"]);
    }
    let out = run_with_stdin(root, &mut command, &body)?;
    let restored: RestoreOutput =
        serde_json::from_slice(&out).context("failed to parse gaze restore JSON")?;
    Ok(restored.text)
}

fn export_audit_rows(root: &Path, audit_db: &Path) -> Result<Vec<AuditRow>> {
    let out = run_stdout(
        root,
        Command::new("cargo")
            .args([
                "run",
                "-q",
                "-p",
                "gaze-cli",
                "--",
                "audit",
                "export",
                "--audit-db",
            ])
            .arg(audit_db)
            .args(["--format", "jsonl"]),
    )?;
    String::from_utf8(out)
        .context("audit export was not UTF-8")?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("failed to parse audit JSONL row"))
        .collect()
}

fn extract_tokens(clean_text: &str) -> Result<Vec<TokenOccurrence>> {
    let bytes = clean_text.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while let Some(open_rel) = clean_text[cursor..].find('<') {
        let open = cursor + open_rel;
        let Some(close_rel) = clean_text[open..].find('>') else {
            break;
        };
        let close = open + close_rel + 1;
        let literal = &clean_text[open..close];
        let inner = &literal[1..literal.len() - 1];
        if let Some((_, rest)) = inner.split_once(':') {
            if let Some((class_part, _n)) = rest.rsplit_once('_') {
                let family = if class_part.starts_with("Custom:") {
                    "session_custom_counter"
                } else {
                    "session_builtin_counter"
                };
                tokens.push(TokenOccurrence {
                    literal: literal.to_string(),
                    family: family.to_string(),
                    shape: format!("<session:{class_part}_{{N}}>"),
                });
            }
        }
        cursor = close;
        if cursor >= bytes.len() {
            break;
        }
    }
    Ok(tokens)
}

fn unique_span(corpus: &str, raw: &str) -> Result<ByteSpan> {
    if raw.is_empty() {
        bail!("bundle-tokenization-drift: restored empty token");
    }
    let matches = corpus.match_indices(raw).collect::<Vec<_>>();
    if matches.len() != 1 {
        bail!(
            "bundle-tokenization-drift: raw fixture value restored from token is not unique; match_count={}",
            matches.len()
        );
    }
    Ok(ByteSpan {
        start: matches[0].0,
        len: raw.len(),
    })
}

fn assert_no_raw_value_leak(snapshot: &Snapshot, corpus: &str) -> Result<()> {
    let json = serde_json::to_string(snapshot)?;
    for entry in &snapshot.entries {
        let raw = &corpus[entry.byte_span.start..entry.byte_span.start + entry.byte_span.len];
        if raw.len() >= 4 && json.contains(raw) {
            bail!(
                "bundle-tokenization-drift: snapshot for `{}` contains raw fixture bytes for {}",
                snapshot.bundle,
                entry.recognizer_id
            );
        }
    }
    if json.contains("session_blob") || json.contains("created_at") {
        bail!(
            "bundle-tokenization-drift: snapshot for `{}` contains forbidden volatile audit/session field",
            snapshot.bundle
        );
    }
    Ok(())
}

fn format_snapshot_diff(bundle: &str, expected: &Snapshot, actual: &Snapshot) -> String {
    let expected_keys = expected
        .entries
        .iter()
        .map(entry_key)
        .collect::<BTreeSet<_>>();
    let actual_keys = actual
        .entries
        .iter()
        .map(entry_key)
        .collect::<BTreeSet<_>>();
    let removed = expected_keys.difference(&actual_keys).take(8);
    let added = actual_keys.difference(&expected_keys).take(8);
    let mut lines = vec![format!(
        "bundle `{bundle}` changed detections {} -> {}",
        expected.stats.detections, actual.stats.detections
    )];
    for key in removed {
        lines.push(format!("  removed {key}"));
    }
    for key in added {
        lines.push(format!("  added {key}"));
    }
    lines.join("\n")
}

fn entry_key(entry: &SnapshotEntry) -> String {
    format!(
        "recognizer={} class={} span={}+{} shape={} count={}",
        entry.recognizer_id,
        entry.class,
        entry.byte_span.start,
        entry.byte_span.len,
        entry.token_shape,
        entry.count
    )
}

fn class_to_snapshot_string(class: PiiClass) -> String {
    match class {
        PiiClass::Email => "Email".to_string(),
        PiiClass::Name => "Name".to_string(),
        PiiClass::Location => "Location".to_string(),
        PiiClass::Organization => "Organization".to_string(),
        PiiClass::Custom(name) => format!("custom:{name}"),
    }
}

fn canonical_audit_class(class: &str) -> String {
    match class {
        "email" => "Email".to_string(),
        "name" => "Name".to_string(),
        "location" => "Location".to_string(),
        "organization" => "Organization".to_string(),
        custom => custom.to_string(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn verify_ack(root: &Path) -> Result<()> {
    let changed_snapshots = changed_snapshot_files(root)?;
    if changed_snapshots.is_empty() {
        println!("bundle-tokenization-drift: verify-ack found no snapshot diff");
        return Ok(());
    }

    let bundles = changed_snapshots
        .iter()
        .map(|path| {
            path.strip_prefix("crates/xtask/snapshots/")
                .unwrap_or(path)
                .trim_end_matches("-no-policy.json")
                .to_string()
        })
        .collect::<BTreeSet<_>>();

    let mut missing = Vec::new();
    if !diff_contains_drift_ack(root)? {
        missing.push("missing added/changed `// drift-ack:` source or test comment".to_string());
    }
    let changelog_missing = changelog_missing_bundles(root, &bundles)?;
    if !changelog_missing.is_empty() {
        missing.push(format!(
            "missing CHANGELOG.md [Unreleased] ### Changed `[bundle-tokenization-drift]` line for bundle(s): {}",
            changelog_missing.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if !missing.is_empty() {
        bail!(
            "bundle-tokenization-drift: --verify-ack rejected snapshot changes: {}\nchanged snapshots:\n{}",
            missing.join("; "),
            changed_snapshots.into_iter().collect::<Vec<_>>().join("\n")
        );
    }

    println!(
        "bundle-tokenization-drift: verify-ack accepted snapshot diff for {}",
        bundles.into_iter().collect::<Vec<_>>().join(", ")
    );
    Ok(())
}

fn changed_snapshot_files(root: &Path) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    for args in [
        vec!["diff", "--name-only", "--", "crates/xtask/snapshots"],
        vec![
            "diff",
            "--cached",
            "--name-only",
            "--",
            "crates/xtask/snapshots",
        ],
    ] {
        collect_changed_snapshot_files(root, &args, &mut files)?;
    }
    if git_ref_exists(root, "origin/main")? {
        collect_changed_snapshot_files(
            root,
            &[
                "diff",
                "--name-only",
                "origin/main...HEAD",
                "--",
                "crates/xtask/snapshots",
            ],
            &mut files,
        )?;
    }
    Ok(files)
}

fn collect_changed_snapshot_files(
    root: &Path,
    args: &[&str],
    files: &mut BTreeSet<String>,
) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for line in String::from_utf8(output.stdout)?.lines() {
        if line.starts_with("crates/xtask/snapshots/") && line.ends_with("-no-policy.json") {
            files.insert(line.to_string());
        }
    }
    Ok(())
}

fn diff_contains_drift_ack(root: &Path) -> Result<bool> {
    let mut diffs = Vec::new();
    for args in [
        vec!["diff", "-U0"],
        vec!["diff", "--cached", "-U0"],
        vec!["diff", "-U0", "origin/main...HEAD"],
    ] {
        if args.contains(&"origin/main...HEAD") && !git_ref_exists(root, "origin/main")? {
            continue;
        }
        let output = Command::new("git")
            .args(&args)
            .current_dir(root)
            .output()
            .with_context(|| format!("failed to run git {}", args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "git {} failed:\n{}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        diffs.push(String::from_utf8(output.stdout)?);
    }
    Ok(diffs.iter().any(|diff| {
        diff.lines().any(|line| {
            line.starts_with('+') && !line.starts_with("+++") && line.contains("// drift-ack:")
        })
    }))
}

fn changelog_missing_bundles(root: &Path, bundles: &BTreeSet<String>) -> Result<BTreeSet<String>> {
    let changelog = fs::read_to_string(root.join("CHANGELOG.md"))
        .context("failed to read CHANGELOG.md for --verify-ack")?;
    let unreleased = section_between(&changelog, "## [Unreleased]", "\n## ")
        .ok_or_else(|| anyhow!("CHANGELOG.md is missing [Unreleased] section"))?;
    let changed = section_between(unreleased, "### Changed", "\n### ").unwrap_or("");
    Ok(bundles
        .iter()
        .filter(|bundle| {
            !changed
                .lines()
                .any(|line| line.contains("[bundle-tokenization-drift]") && line.contains(*bundle))
        })
        .cloned()
        .collect())
}

fn section_between<'a>(text: &'a str, start_marker: &str, next_marker: &str) -> Option<&'a str> {
    let start = text.find(start_marker)? + start_marker.len();
    let rest = &text[start..];
    let end = rest.find(next_marker).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn git_ref_exists(root: &Path, name: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", name])
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to check git ref {name}"))?;
    Ok(output.status.success())
}
