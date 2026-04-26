use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

mod ci_feature_matrix;
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
    CiFeatureMatrix,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::SymmetricPotemkin => run_symmetric_potemkin_gate(),
        Command::ClassMapOverrideSafety => run_class_map_override_safety_gate(),
        Command::RecognizerCompositionValidator => run_recognizer_composition_validator_gate(),
        Command::NoTenantKnowledge => no_tenant_knowledge::run(),
        Command::AuditMetadataOnly => run_audit_metadata_only_gate(),
        Command::CiFeatureMatrix => ci_feature_matrix::run(),
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

const RESTORE_AUDIT_RENAMED_ROOT_MARKER: &str = "__renamed_gaze_root__";

const RESTORE_AUDIT_FORBIDDEN_SYMBOLS: &[&str] = &[
    RESTORE_AUDIT_RENAMED_ROOT_MARKER,
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

#[derive(Clone, Copy)]
enum UseTreeRoot {
    Crate,
    Gaze,
    GazeCli,
    Other,
}

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
        match item {
            syn::Item::Use(item_use) => check_use(item_use, path)?,
            syn::Item::ExternCrate(extern_crate) => {
                if matches!(extern_crate.ident.to_string().as_str(), "gaze" | "gaze_cli") {
                    bail!(
                        "audit metadata import in restore path: {} imports {}",
                        path.display(),
                        RESTORE_AUDIT_RENAMED_ROOT_MARKER
                    );
                }
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, items)) = &item_mod.content {
                    walk_items(items, path)?;
                }
            }
            syn::Item::Fn(item_fn) => walk_block_stmts(&item_fn.block, path)?,
            syn::Item::Impl(item_impl) => {
                for item in &item_impl.items {
                    match item {
                        syn::ImplItem::Const(item_const) => walk_expr(&item_const.expr, path)?,
                        syn::ImplItem::Fn(item_fn) => walk_block_stmts(&item_fn.block, path)?,
                        _ => {}
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for item in &item_trait.items {
                    match item {
                        syn::TraitItem::Const(item_const) => {
                            if let Some((_, expr)) = &item_const.default {
                                walk_expr(expr, path)?;
                            }
                        }
                        syn::TraitItem::Fn(item_fn) => {
                            if let Some(block) = &item_fn.default {
                                walk_block_stmts(block, path)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            syn::Item::Const(item_const) => walk_expr(&item_const.expr, path)?,
            syn::Item::Static(item_static) => walk_expr(&item_static.expr, path)?,
            _ => {}
        }
    }
    Ok(())
}

fn walk_block_stmts(block: &syn::Block, path: &Path) -> Result<()> {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Item(item) => walk_items(std::slice::from_ref(item), path)?,
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    walk_expr(&init.expr, path)?;
                }
            }
            syn::Stmt::Expr(expr, _) => walk_expr(expr, path)?,
            syn::Stmt::Macro(_) => {}
        }
    }
    Ok(())
}

fn walk_expr(expr: &syn::Expr, path: &Path) -> Result<()> {
    match expr {
        syn::Expr::Array(expr) => {
            for item in &expr.elems {
                walk_expr(item, path)?;
            }
        }
        syn::Expr::Assign(expr) => {
            walk_expr(&expr.left, path)?;
            walk_expr(&expr.right, path)?;
        }
        syn::Expr::Async(expr) => walk_block_stmts(&expr.block, path)?,
        syn::Expr::Await(expr) => walk_expr(&expr.base, path)?,
        syn::Expr::Binary(expr) => {
            walk_expr(&expr.left, path)?;
            walk_expr(&expr.right, path)?;
        }
        syn::Expr::Block(expr) => walk_block_stmts(&expr.block, path)?,
        syn::Expr::Break(expr) => {
            if let Some(value) = &expr.expr {
                walk_expr(value, path)?;
            }
        }
        syn::Expr::Call(expr) => {
            walk_expr(&expr.func, path)?;
            for arg in &expr.args {
                walk_expr(arg, path)?;
            }
        }
        syn::Expr::Cast(expr) => walk_expr(&expr.expr, path)?,
        syn::Expr::Closure(expr) => walk_expr(&expr.body, path)?,
        syn::Expr::Const(expr) => walk_block_stmts(&expr.block, path)?,
        syn::Expr::Field(expr) => walk_expr(&expr.base, path)?,
        syn::Expr::ForLoop(expr) => {
            walk_expr(&expr.expr, path)?;
            walk_block_stmts(&expr.body, path)?;
        }
        syn::Expr::Group(expr) => walk_expr(&expr.expr, path)?,
        syn::Expr::If(expr) => {
            walk_expr(&expr.cond, path)?;
            walk_block_stmts(&expr.then_branch, path)?;
            if let Some((_, else_branch)) = &expr.else_branch {
                walk_expr(else_branch, path)?;
            }
        }
        syn::Expr::Index(expr) => {
            walk_expr(&expr.expr, path)?;
            walk_expr(&expr.index, path)?;
        }
        syn::Expr::Let(expr) => walk_expr(&expr.expr, path)?,
        syn::Expr::Loop(expr) => walk_block_stmts(&expr.body, path)?,
        syn::Expr::Macro(_) => {}
        syn::Expr::Match(expr) => {
            walk_expr(&expr.expr, path)?;
            for arm in &expr.arms {
                if let Some((_, guard)) = &arm.guard {
                    walk_expr(guard, path)?;
                }
                walk_expr(&arm.body, path)?;
            }
        }
        syn::Expr::MethodCall(expr) => {
            walk_expr(&expr.receiver, path)?;
            for arg in &expr.args {
                walk_expr(arg, path)?;
            }
        }
        syn::Expr::Paren(expr) => walk_expr(&expr.expr, path)?,
        syn::Expr::Range(expr) => {
            if let Some(start) = &expr.start {
                walk_expr(start, path)?;
            }
            if let Some(end) = &expr.end {
                walk_expr(end, path)?;
            }
        }
        syn::Expr::Reference(expr) => walk_expr(&expr.expr, path)?,
        syn::Expr::Repeat(expr) => {
            walk_expr(&expr.expr, path)?;
            walk_expr(&expr.len, path)?;
        }
        syn::Expr::Return(expr) => {
            if let Some(value) = &expr.expr {
                walk_expr(value, path)?;
            }
        }
        syn::Expr::Struct(expr) => {
            for field in &expr.fields {
                walk_expr(&field.expr, path)?;
            }
            if let Some(rest) = &expr.rest {
                walk_expr(rest, path)?;
            }
        }
        syn::Expr::Try(expr) => walk_expr(&expr.expr, path)?,
        syn::Expr::TryBlock(expr) => walk_block_stmts(&expr.block, path)?,
        syn::Expr::Tuple(expr) => {
            for item in &expr.elems {
                walk_expr(item, path)?;
            }
        }
        syn::Expr::Unary(expr) => walk_expr(&expr.expr, path)?,
        syn::Expr::Unsafe(expr) => walk_block_stmts(&expr.block, path)?,
        syn::Expr::While(expr) => {
            walk_expr(&expr.cond, path)?;
            walk_block_stmts(&expr.body, path)?;
        }
        syn::Expr::Yield(expr) => {
            if let Some(value) = &expr.expr {
                walk_expr(value, path)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_use(item_use: &syn::ItemUse, path: &Path) -> Result<()> {
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
    Ok(())
}

fn collect_external_mod_files(path: &Path, items: &[syn::Item]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for item in items {
        match item {
            syn::Item::Mod(item_mod) => {
                if let Some((_, items)) = &item_mod.content {
                    files.extend(collect_external_mod_files(path, items)?);
                } else {
                    files.extend(resolve_external_mod_files(path, item_mod));
                }
            }
            syn::Item::Fn(item_fn) => {
                files.extend(collect_external_mod_files_in_block(path, &item_fn.block)?);
            }
            syn::Item::Impl(item_impl) => {
                for item in &item_impl.items {
                    match item {
                        syn::ImplItem::Const(item_const) => {
                            files.extend(collect_external_mod_files_in_expr(
                                path,
                                &item_const.expr,
                            )?);
                        }
                        syn::ImplItem::Fn(item_fn) => {
                            files
                                .extend(collect_external_mod_files_in_block(path, &item_fn.block)?);
                        }
                        _ => {}
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for item in &item_trait.items {
                    match item {
                        syn::TraitItem::Const(item_const) => {
                            if let Some((_, expr)) = &item_const.default {
                                files.extend(collect_external_mod_files_in_expr(path, expr)?);
                            }
                        }
                        syn::TraitItem::Fn(item_fn) => {
                            if let Some(block) = &item_fn.default {
                                files.extend(collect_external_mod_files_in_block(path, block)?);
                            }
                        }
                        _ => {}
                    }
                }
            }
            syn::Item::Const(item_const) => {
                files.extend(collect_external_mod_files_in_expr(path, &item_const.expr)?);
            }
            syn::Item::Static(item_static) => {
                files.extend(collect_external_mod_files_in_expr(path, &item_static.expr)?);
            }
            _ => {}
        }
    }
    Ok(files)
}

fn collect_external_mod_files_in_block(path: &Path, block: &syn::Block) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Item(item) => {
                files.extend(collect_external_mod_files(
                    path,
                    std::slice::from_ref(item),
                )?);
            }
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    files.extend(collect_external_mod_files_in_expr(path, &init.expr)?);
                }
            }
            syn::Stmt::Expr(expr, _) => {
                files.extend(collect_external_mod_files_in_expr(path, expr)?);
            }
            syn::Stmt::Macro(_) => {}
        }
    }
    Ok(files)
}

fn collect_external_mod_files_in_expr(path: &Path, expr: &syn::Expr) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    match expr {
        syn::Expr::Array(expr) => {
            for item in &expr.elems {
                files.extend(collect_external_mod_files_in_expr(path, item)?);
            }
        }
        syn::Expr::Assign(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.left)?);
            files.extend(collect_external_mod_files_in_expr(path, &expr.right)?);
        }
        syn::Expr::Async(expr) => {
            files.extend(collect_external_mod_files_in_block(path, &expr.block)?)
        }
        syn::Expr::Await(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.base)?)
        }
        syn::Expr::Binary(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.left)?);
            files.extend(collect_external_mod_files_in_expr(path, &expr.right)?);
        }
        syn::Expr::Block(expr) => {
            files.extend(collect_external_mod_files_in_block(path, &expr.block)?)
        }
        syn::Expr::Break(expr) => {
            if let Some(value) = &expr.expr {
                files.extend(collect_external_mod_files_in_expr(path, value)?);
            }
        }
        syn::Expr::Call(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.func)?);
            for arg in &expr.args {
                files.extend(collect_external_mod_files_in_expr(path, arg)?);
            }
        }
        syn::Expr::Cast(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?)
        }
        syn::Expr::Closure(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.body)?)
        }
        syn::Expr::Const(expr) => {
            files.extend(collect_external_mod_files_in_block(path, &expr.block)?)
        }
        syn::Expr::Field(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.base)?)
        }
        syn::Expr::ForLoop(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?);
            files.extend(collect_external_mod_files_in_block(path, &expr.body)?);
        }
        syn::Expr::Group(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?)
        }
        syn::Expr::If(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.cond)?);
            files.extend(collect_external_mod_files_in_block(
                path,
                &expr.then_branch,
            )?);
            if let Some((_, else_branch)) = &expr.else_branch {
                files.extend(collect_external_mod_files_in_expr(path, else_branch)?);
            }
        }
        syn::Expr::Index(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?);
            files.extend(collect_external_mod_files_in_expr(path, &expr.index)?);
        }
        syn::Expr::Let(expr) => files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?),
        syn::Expr::Loop(expr) => {
            files.extend(collect_external_mod_files_in_block(path, &expr.body)?)
        }
        syn::Expr::Macro(_) => {}
        syn::Expr::Match(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?);
            for arm in &expr.arms {
                if let Some((_, guard)) = &arm.guard {
                    files.extend(collect_external_mod_files_in_expr(path, guard)?);
                }
                files.extend(collect_external_mod_files_in_expr(path, &arm.body)?);
            }
        }
        syn::Expr::MethodCall(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.receiver)?);
            for arg in &expr.args {
                files.extend(collect_external_mod_files_in_expr(path, arg)?);
            }
        }
        syn::Expr::Paren(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?)
        }
        syn::Expr::Range(expr) => {
            if let Some(start) = &expr.start {
                files.extend(collect_external_mod_files_in_expr(path, start)?);
            }
            if let Some(end) = &expr.end {
                files.extend(collect_external_mod_files_in_expr(path, end)?);
            }
        }
        syn::Expr::Reference(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?)
        }
        syn::Expr::Repeat(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?);
            files.extend(collect_external_mod_files_in_expr(path, &expr.len)?);
        }
        syn::Expr::Return(expr) => {
            if let Some(value) = &expr.expr {
                files.extend(collect_external_mod_files_in_expr(path, value)?);
            }
        }
        syn::Expr::Struct(expr) => {
            for field in &expr.fields {
                files.extend(collect_external_mod_files_in_expr(path, &field.expr)?);
            }
            if let Some(rest) = &expr.rest {
                files.extend(collect_external_mod_files_in_expr(path, rest)?);
            }
        }
        syn::Expr::Try(expr) => files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?),
        syn::Expr::TryBlock(expr) => {
            files.extend(collect_external_mod_files_in_block(path, &expr.block)?)
        }
        syn::Expr::Tuple(expr) => {
            for item in &expr.elems {
                files.extend(collect_external_mod_files_in_expr(path, item)?);
            }
        }
        syn::Expr::Unary(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.expr)?)
        }
        syn::Expr::Unsafe(expr) => {
            files.extend(collect_external_mod_files_in_block(path, &expr.block)?)
        }
        syn::Expr::While(expr) => {
            files.extend(collect_external_mod_files_in_expr(path, &expr.cond)?);
            files.extend(collect_external_mod_files_in_block(path, &expr.body)?);
        }
        syn::Expr::Yield(expr) => {
            if let Some(value) = &expr.expr {
                files.extend(collect_external_mod_files_in_expr(path, value)?);
            }
        }
        _ => {}
    }
    Ok(files)
}

fn resolve_external_mod_files(path: &Path, item_mod: &syn::ItemMod) -> Vec<PathBuf> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    if let Some(path_attr) = item_mod.attrs.iter().find_map(path_attribute_value) {
        return vec![dir.join(path_attr)];
    }

    let name = item_mod.ident.to_string();
    vec![
        dir.join(format!("{name}.rs")),
        dir.join(name).join("mod.rs"),
    ]
}

fn path_attribute_value(attr: &syn::Attribute) -> Option<String> {
    if !attr.path().is_ident("path") {
        return None;
    }
    let syn::Meta::NameValue(value) = &attr.meta else {
        return None;
    };
    let syn::Expr::Lit(expr_lit) = &value.value else {
        return None;
    };
    let syn::Lit::Str(path) = &expr_lit.lit else {
        return None;
    };
    Some(path.value())
}

fn collect_use_tree_names(tree: &syn::UseTree, names: &mut Vec<String>) {
    collect_use_tree_names_with_root(tree, names, None);
}

fn collect_use_tree_names_with_root(
    tree: &syn::UseTree,
    names: &mut Vec<String>,
    current_root: Option<UseTreeRoot>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            names.push(path.ident.to_string());
            let root = current_root.unwrap_or_else(|| use_tree_root(&path.ident));
            collect_use_tree_names_with_root(&path.tree, names, Some(root));
        }
        syn::UseTree::Name(name) => names.push(name.ident.to_string()),
        syn::UseTree::Rename(rename) => {
            names.push(rename.ident.to_string());
            names.push(rename.rename.to_string());
            let root = current_root.unwrap_or_else(|| use_tree_root(&rename.ident));
            if matches!(
                root,
                UseTreeRoot::Crate | UseTreeRoot::Gaze | UseTreeRoot::GazeCli
            ) {
                names.push(RESTORE_AUDIT_RENAMED_ROOT_MARKER.to_string());
            }
        }
        syn::UseTree::Glob(_) => {
            if matches!(
                current_root,
                Some(UseTreeRoot::Crate | UseTreeRoot::Gaze | UseTreeRoot::GazeCli)
            ) {
                names.extend(
                    RESTORE_AUDIT_FORBIDDEN_SYMBOLS
                        .iter()
                        .filter(|symbol| **symbol != RESTORE_AUDIT_RENAMED_ROOT_MARKER)
                        .map(|symbol| (*symbol).to_string()),
                );
            } else {
                names.push("*".to_string());
            }
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree_names_with_root(item, names, current_root);
            }
        }
    }
}

fn use_tree_root(ident: &syn::Ident) -> UseTreeRoot {
    if ident == "crate" {
        UseTreeRoot::Crate
    } else if ident == "gaze" {
        UseTreeRoot::Gaze
    } else if ident == "gaze_cli" {
        UseTreeRoot::GazeCli
    } else {
        UseTreeRoot::Other
    }
}

fn restore_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    let cli_restore = root.join("crates/gaze-cli/src/restore");
    if cli_restore.exists() {
        collect_rs_files(&cli_restore, &mut queue)?;
    }
    let core_restore = root.join("crates/gaze/src/restore.rs");
    if core_restore.exists() {
        queue.push_back(core_restore);
    }

    while let Some(path) = queue.pop_front() {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", path.display()))?;
        if !seen.insert(canonical.clone()) {
            continue;
        }

        let source = fs::read_to_string(&canonical)
            .with_context(|| format!("failed to read {}", canonical.display()))?;
        let parsed = syn::parse_file(&source)
            .with_context(|| format!("failed to parse {}", canonical.display()))?;
        for external_mod in collect_external_mod_files(&canonical, &parsed.items)? {
            if external_mod.exists() {
                queue.push_back(external_mod);
            }
        }
        files.push(canonical);
    }

    if files.is_empty() {
        bail!("audit_metadata_only found no restore files to scan");
    }
    files.sort();
    Ok(files)
}

fn collect_rs_files(dir: &Path, files: &mut VecDeque<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push_back(path);
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
