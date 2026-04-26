use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

mod no_tenant_knowledge;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    SymmetricPotemkin,
    ClassMapOverrideSafety,
    RecognizerCompositionValidator,
    NoTenantKnowledge,
    AuditMetadataOnly,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::SymmetricPotemkin => run_symmetric_potemkin_gate(),
        Command::ClassMapOverrideSafety => run_class_map_override_safety_gate(),
        Command::RecognizerCompositionValidator => run_recognizer_composition_validator_gate(),
        Command::NoTenantKnowledge => no_tenant_knowledge::run(),
        Command::AuditMetadataOnly => run_audit_metadata_only_gate(),
    }
}

#[derive(Debug, Clone, Copy)]
struct BehavioralTest {
    package: &'static str,
    test_target: Option<&'static str>,
    name: &'static str,
}

// Self-test path for reviewers:
// 1. Temporarily rename any test below, for example append `_disabled` to
//    `t21d_token_family_threads_from_recognizer_to_session`.
// 2. Run `cargo run -p xtask -- symmetric-potemkin`.
// 3. The gate must exit non-zero during the list phase, before any green
//    subset can mask the missing behavioral contract. Revert the rename.
const SYMMETRIC_POTEMKIN_TESTS: &[BehavioralTest] = &[
    BehavioralTest {
        package: "gaze",
        test_target: None,
        name: "pipeline::tests::t21d_token_family_threads_from_recognizer_to_session",
    },
    BehavioralTest {
        package: "gaze",
        test_target: None,
        name: "session::tests::snapshot_round_trip_two_families_same_class_raw_preserved_under_shared_counter",
    },
    BehavioralTest {
        package: "gaze",
        test_target: None,
        name: "session::tests::import_v0_4_0_snapshot_version_2_succeeds_with_default_family",
    },
    BehavioralTest {
        package: "gaze-recognizers",
        test_target: None,
        name: "regex::tests::regex_recognizer_uses_first_non_empty_capture_group",
    },
    BehavioralTest {
        package: "gaze-assembly",
        test_target: None,
        name: "tests::pattern_template_lowers_correctly_under_locale_chain_de",
    },
    BehavioralTest {
        package: "gaze-cli",
        test_target: Some("cli_pipe"),
        name: "t21g_pattern_template_uses_active_locale_de_when_en_loaded_after_de",
    },
    BehavioralTest {
        package: "gaze-cli",
        test_target: Some("cli_pipe"),
        name: "t21h_pattern_template_falls_back_to_global_when_locale_not_loaded",
    },
    BehavioralTest {
        package: "gaze-recognizers",
        test_target: None,
        name: "ner::tests::merge_bio_spans_returns_min_confidence_with_one_low_token",
    },
    BehavioralTest {
        package: "gaze-recognizers",
        test_target: None,
        name: "ner::tests::ner_recognizer_filters_below_threshold",
    },
    BehavioralTest {
        package: "gaze",
        test_target: None,
        name: "policy::tests::rejects_ner_threshold_out_of_range",
    },
    BehavioralTest {
        package: "gaze-cli",
        test_target: None,
        name: "pipeline::run::tests::t_cli_ner_threshold_overrides_policy_value",
    },
    BehavioralTest {
        package: "gaze-cli",
        test_target: Some("cli_pipe"),
        name: "t_cli_ner_threshold_out_of_range_fails_closed",
    },
    BehavioralTest {
        package: "gaze-assembly",
        test_target: None,
        name: "tests::t20_context_class_map_overrides_policy_dict_class",
    },
    BehavioralTest {
        package: "gaze-assembly",
        test_target: None,
        name: "tests::t20a_class_map_override_fails_closed_when_action_rule_uncovered",
    },
];

const RECOGNIZER_COMPOSITION_VALIDATOR_TESTS: &[BehavioralTest] = &[
    BehavioralTest {
        package: "gaze",
        test_target: None,
        name: "rulepack::tests::rulepack_load_fails_when_two_name_recognizers_omit_cooperates_with",
    },
    BehavioralTest {
        package: "gaze",
        test_target: None,
        name: "rulepack::tests::rulepack_load_accepts_same_class_pair_with_cooperates_with",
    },
];

/// Adversarial self-test: reviewer manually renames one of the listed
/// tests on a throwaway branch and verifies xtask exits non-zero.
/// Codifies meta-Potemkin guard per drawer gaze_architecture_12b32d53.
const CLASS_MAP_OVERRIDE_SAFETY_TESTS: &[BehavioralTest] = &[
    BehavioralTest {
        package: "gaze-assembly",
        test_target: None,
        name: "tests::t20_context_class_map_overrides_policy_dict_class",
    },
    BehavioralTest {
        package: "gaze-assembly",
        test_target: None,
        name: "tests::t20a_class_map_override_fails_closed_when_action_rule_uncovered",
    },
    BehavioralTest {
        package: "gaze-assembly",
        test_target: None,
        name: "tests::t20b_rulepack_context_dict_override_fails_closed_when_uncovered",
    },
    BehavioralTest {
        package: "gaze-cli",
        test_target: Some("cli_pipe"),
        name: "context_json_standalone_dictionary_detects_without_policy_entry",
    },
];

fn run_symmetric_potemkin_gate() -> Result<()> {
    println!(
        "symmetric_potemkin_gate: checking {} behavioral tests",
        SYMMETRIC_POTEMKIN_TESTS.len()
    );
    for test in SYMMETRIC_POTEMKIN_TESTS {
        ensure_test_exists(*test)?;
    }
    for test in SYMMETRIC_POTEMKIN_TESTS {
        run_behavioral_test("symmetric_potemkin_gate", *test)?;
    }
    println!("symmetric_potemkin_gate: passed");
    Ok(())
}

fn run_class_map_override_safety_gate() -> Result<()> {
    println!(
        "class_map_override_safety: checking {} behavioral tests",
        CLASS_MAP_OVERRIDE_SAFETY_TESTS.len()
    );
    for test in CLASS_MAP_OVERRIDE_SAFETY_TESTS {
        ensure_test_exists(*test)?;
    }
    for test in CLASS_MAP_OVERRIDE_SAFETY_TESTS {
        run_behavioral_test("class_map_override_safety", *test)?;
    }
    println!("class_map_override_safety: passed");
    Ok(())
}

fn run_recognizer_composition_validator_gate() -> Result<()> {
    println!(
        "recognizer_composition_validator: checking {} behavioral tests",
        RECOGNIZER_COMPOSITION_VALIDATOR_TESTS.len()
    );
    for test in RECOGNIZER_COMPOSITION_VALIDATOR_TESTS {
        ensure_test_exists(*test)?;
    }
    for test in RECOGNIZER_COMPOSITION_VALIDATOR_TESTS {
        run_behavioral_test("recognizer_composition_validator", *test)?;
    }
    println!("recognizer_composition_validator: passed");
    Ok(())
}

const RESTORE_AUDIT_FORBIDDEN_SYMBOLS: &[&str] = &[
    "redaction_log",
    "ConflictTier",
    "DocumentKind",
    "RedactionEntry",
    "RedactionLogger",
    "SqliteLogger",
    // Extend denylist on each new audit export.
    "AuditFilter",
    "AuditLogRow",
    "AUDIT_RESTRICTED_COLUMNS",
    "build_audit_query_sql",
    "current_epoch_ms",
];

fn run_audit_metadata_only_gate() -> Result<()> {
    let root = std::env::current_dir().context("failed to resolve current directory")?;
    let restore_files = restore_files(&root)?;
    println!(
        "audit_metadata_only: scanning {} restore files",
        restore_files.len()
    );
    for path in restore_files {
        let source = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let parsed = syn::parse_file(&source)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        reject_forbidden_use_imports(&path, &parsed)?;
    }
    println!("audit_metadata_only: passed");
    Ok(())
}

fn reject_forbidden_use_imports(path: &Path, file: &syn::File) -> Result<()> {
    walk_items(&file.items, path)
}

fn walk_items(items: &[syn::Item], path: &Path) -> Result<()> {
    for item in items {
        if let syn::Item::Use(item_use) = item {
            let mut imported = Vec::new();
            collect_use_tree_names(&item_use.tree, &mut imported);
            if let Some(symbol) = RESTORE_AUDIT_FORBIDDEN_SYMBOLS
                .iter()
                .find(|symbol| imported.iter().any(|name| name == **symbol))
            {
                bail!(
                    "audit metadata import in restore path: {} imports {}",
                    path.display(),
                    symbol
                );
            }
        }
        if let syn::Item::Mod(item_mod) = item {
            if let Some((_, items)) = &item_mod.content {
                walk_items(items, path)?;
            }
        }
    }
    Ok(())
}

fn collect_use_tree_names(tree: &syn::UseTree, names: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(path) => {
            names.push(path.ident.to_string());
            collect_use_tree_names(&path.tree, names);
        }
        syn::UseTree::Name(name) => names.push(name.ident.to_string()),
        syn::UseTree::Rename(rename) => {
            names.push(rename.ident.to_string());
            names.push(rename.rename.to_string());
        }
        syn::UseTree::Glob(_) => names.push("*".to_string()),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree_names(item, names);
            }
        }
    }
}

fn restore_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let cli_restore = root.join("crates/gaze-cli/src/restore");
    if cli_restore.exists() {
        collect_rs_files(&cli_restore, &mut files)?;
    }
    let core_restore = root.join("crates/gaze/src/restore.rs");
    if core_restore.exists() {
        files.push(core_restore);
    }
    if files.is_empty() {
        bail!("audit_metadata_only found no restore files to scan");
    }
    files.sort();
    Ok(files)
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn ensure_test_exists(test: BehavioralTest) -> Result<()> {
    let output = cargo_test_command(test, None)
        .arg("--")
        .arg("--list")
        .output()
        .with_context(|| format!("failed to list tests for {}", test.package))?;
    if !output.status.success() {
        bail!(
            "failed to list tests for {}: {}",
            describe(test),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected_line = format!("{}: test", test.name);
    if !stdout.lines().any(|line| line == expected_line) {
        bail!("missing behavioral test: {}", describe(test));
    }
    Ok(())
}

fn run_behavioral_test(gate: &str, test: BehavioralTest) -> Result<()> {
    println!("{gate}: running {}", describe(test));
    let status = cargo_test_command(test, Some(test.name))
        .arg("--")
        .arg("--exact")
        .status()
        .with_context(|| format!("failed to run {}", describe(test)))?;
    if !status.success() {
        bail!("behavioral test failed: {}", describe(test));
    }
    Ok(())
}

fn cargo_test_command(test: BehavioralTest, filter: Option<&str>) -> ProcessCommand {
    let mut command = ProcessCommand::new("cargo");
    command.arg("test").arg("-p").arg(test.package);
    if let Some(test_target) = test.test_target {
        command.arg("--test").arg(test_target);
    }
    if let Some(filter) = filter {
        command.arg(filter);
    }
    command
}

fn describe(test: BehavioralTest) -> String {
    match test.test_target {
        Some(target) => format!("{} --test {} {}", test.package, target, test.name),
        None => format!("{} {}", test.package, test.name),
    }
}
