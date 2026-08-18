//! Drift check for the committed `gaze --help` capture.
//!
//! The authoritative proof that a refactor left the CLI surface alone is
//! `scripts/verify/cli-help-surface.sh`, which builds two revisions in one run
//! and diffs them against each other. This test is the cheap companion: it
//! compares the running binary against the capture committed under
//! `tests/fixtures/cli-help/`, so a reviewer can see the surface without a
//! two-revision build and an accidental change gets a name.
//!
//! It is a golden comparison, so it is `#[ignore]`d — refreshing the fixture
//! makes it pass again, and that is exactly what a before/after harness must
//! never allow. Run it with:
//!
//!     cargo test -p gaze-cli --all-features --test cli_help_surface -- --ignored
//!
//! Refresh the fixture with `scripts/verify/cli-help-surface.sh --write-fixtures`,
//! which only writes when the before/after diff is empty.
//!
//! The fixture records the `--all-features` surface, so the whole file compiles
//! away unless every subcommand-bearing feature is on.
#![cfg(all(
    feature = "setup",
    feature = "document",
    feature = "index",
    feature = "mcp",
    feature = "proxy",
    feature = "dashboard",
    feature = "runtime-tract",
    feature = "runtime-candle"
))]

use std::path::Path;

use assert_cmd::Command;

/// Hidden command paths, which never appear under `Commands:` in help output.
/// Kept in step with `HIDDEN_PATHS` in `scripts/verify/cli-help-surface.sh`.
const HIDDEN_PATHS: &[&[&str]] = &[&["proxy", "_dashboard-child"]];

fn help_for(path: &[&str]) -> String {
    let mut cmd = Command::cargo_bin("gaze").expect("gaze binary");
    cmd.args(path).arg("--help");
    let out = cmd.output().expect("run gaze --help");
    assert!(
        out.status.success(),
        "`gaze {} --help` exited {:?}",
        path.join(" "),
        out.status.code()
    );
    String::from_utf8(out.stdout).expect("utf-8 help")
}

/// Subcommand names listed in a help text's `Commands:` block.
fn child_names(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_block = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            in_block = true;
            continue;
        }
        if in_block {
            if line.ends_with(':') && !line.starts_with(' ') {
                break;
            }
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if indent == 2 {
                if let Some(name) = trimmed.split_whitespace().next() {
                    if name != "help" && name.starts_with(|c: char| c.is_ascii_alphabetic()) {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    names
}

fn slug(path: &[&str]) -> String {
    if path.is_empty() {
        "root".to_string()
    } else {
        path.join("-")
    }
}

fn collect(path: &[&str], out: &mut Vec<(String, String)>) {
    let help = help_for(path);
    for child in child_names(&help) {
        let mut next: Vec<&str> = path.to_vec();
        next.push(&child);
        collect(&next, out);
    }
    out.push((slug(path), help));
}

#[test]
#[ignore = "golden comparison against a committed capture; the two-revision script is the real gate"]
fn help_output_matches_the_committed_capture() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli-help");

    let mut captured = Vec::new();
    collect(&[], &mut captured);
    for path in HIDDEN_PATHS {
        captured.push((slug(path), help_for(path)));
    }
    assert!(
        captured.len() > 10,
        "walked only {} commands; the traversal is broken",
        captured.len()
    );

    let mut mismatches = Vec::new();
    for (name, help) in &captured {
        let path = fixture_dir.join(format!("{name}.txt"));
        match std::fs::read_to_string(&path) {
            Ok(expected) => {
                if expected.trim_end() != help.trim_end() {
                    mismatches.push(format!("{name}: help differs from the committed capture"));
                }
            }
            Err(_) => mismatches.push(format!(
                "{name}: no committed capture at {}",
                path.display()
            )),
        }
    }

    let captured_names: Vec<&str> = captured.iter().map(|(name, _)| name.as_str()).collect();
    for entry in std::fs::read_dir(&fixture_dir).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        let file = entry.file_name();
        let name = file.to_string_lossy();
        let Some(stem) = name.strip_suffix(".txt") else {
            continue;
        };
        if !captured_names.contains(&stem) {
            mismatches.push(format!("{stem}: committed capture has no matching command"));
        }
    }

    assert!(
        mismatches.is_empty(),
        "CLI help surface drifted from tests/fixtures/cli-help:\n  {}\n\
         Re-run scripts/verify/cli-help-surface.sh to see the before/after diff.",
        mismatches.join("\n  ")
    );
}
