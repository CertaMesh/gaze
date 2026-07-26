use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const ROOT_UI_DIRS: &[&str] = &[
    "crates/gaze-inspection/tests/ui",
    "crates/gaze/tests/ui",
    "crates/gaze-mcp-core/tests/ui",
];

const ROOT_EXPECTATIONS: &[&str] = &[
    "crates/gaze-inspection/tests/ui/event_no_debug_or_serialize.stderr",
    "crates/gaze-inspection/tests/ui/handles_no_clone_or_debug.stderr",
    "crates/gaze-inspection/tests/ui/metadata_fields_are_private.stderr",
    "crates/gaze-inspection/tests/ui/pending_halves_are_one_shot.stderr",
    "crates/gaze-inspection/tests/ui/private_pending_fields.stderr",
    "crates/gaze-inspection/tests/ui/purge_guard_cannot_target_another_consumer.stderr",
    "crates/gaze-inspection/tests/ui/safe_codes_reject_strings.stderr",
    "crates/gaze-inspection/tests/ui/sensitive_callback_escape.stderr",
    "crates/gaze-inspection/tests/ui/sensitive_no_clone.stderr",
    "crates/gaze-inspection/tests/ui/sensitive_no_debug.stderr",
    "crates/gaze-inspection/tests/ui/sensitive_no_display.stderr",
    "crates/gaze-inspection/tests/ui/sensitive_no_serialize.stderr",
    "crates/gaze-inspection/tests/ui/typed_ordinals_do_not_convert.stderr",
    "crates/gaze/tests/ui/pipeline_transaction_rejects_committed_snapshot.stderr",
    "crates/gaze/tests/ui/pipeline_transaction_rejects_fake_target.stderr",
    "crates/gaze/tests/ui/raw_document_serialize.stderr",
    "crates/gaze-mcp-core/tests/ui/tool_ctx_no_external_constructor.stderr",
    "crates/gaze-mcp-core/tests/ui/tool_ctx_no_struct_literal.stderr",
    "crates/gaze-mcp-core/tests/ui/tool_resources_no_external_constructor.stderr",
];

const DYLINT_UI_ROOT: &str = "lint/dylint/ui";
const DYLINT_FAILURE_EXPECTATIONS: &[&str] = &[
    "lint/dylint/ui/crates/gaze-cli/src/restore/include_expands_forbidden_path.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/trait_bound_where_clause.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/type_fn_param_return.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/type_generic_arg.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/type_phantom_data.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/type_struct_field.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_in_fn_body.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_in_impl_body.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_in_trait_default.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_top_level.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_via_alias.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_via_const_initializer.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_via_extern_crate.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_via_glob.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_via_macro_emit_callsite.stderr",
    "lint/dylint/ui/crates/gaze-cli/src/restore/use_via_path_attr.stderr",
];
const DYLINT_PASS_SOURCES: &[&str] = &[
    "lint/dylint/ui/crates/gaze-cli/src/main/use_via_macro_emit_defsite_negative.rs",
    "lint/dylint/ui/crates/gaze-cli/src/restore/clean_no_violation.rs",
];

#[derive(Debug, PartialEq, Eq)]
struct Inventory {
    inspection_stderr: usize,
    gaze_stderr: usize,
    mcp_core_stderr: usize,
    dylint_sources: usize,
    dylint_stderr: usize,
    dylint_pass: usize,
}

pub fn run() -> Result<()> {
    let root = std::env::current_dir().context("failed to resolve workspace root")?;
    let inventory = verify(&root)?;
    println!(
        "trybuild_fixture_hygiene: root expectations verified: gaze-inspection={} gaze={} gaze-mcp-core={} total={}",
        inventory.inspection_stderr,
        inventory.gaze_stderr,
        inventory.mcp_core_stderr,
        inventory.inspection_stderr + inventory.gaze_stderr + inventory.mcp_core_stderr
    );
    println!(
        "trybuild_fixture_hygiene: detached Dylint verified: fixtures={} stderr={} pass={}",
        inventory.dylint_sources, inventory.dylint_stderr, inventory.dylint_pass
    );
    println!("trybuild_fixture_hygiene: passed");
    Ok(())
}

fn verify(root: &Path) -> Result<Inventory> {
    let expected_root_stderr = string_set(ROOT_EXPECTATIONS.iter().copied());
    let expected_root_sources = source_set(ROOT_EXPECTATIONS);
    let actual_root_stderr = collect_from_roots(root, ROOT_UI_DIRS, "stderr")?;
    let actual_root_sources = collect_from_roots(root, ROOT_UI_DIRS, "rs")?;
    require_exact_inventory(
        &actual_root_stderr,
        &expected_root_stderr,
        "missing root trybuild expectation",
        "unexpected root trybuild expectation",
    )?;
    require_exact_inventory(
        &actual_root_sources,
        &expected_root_sources,
        "missing root trybuild source",
        "unexpected root trybuild source",
    )?;

    let expected_dylint_stderr = string_set(DYLINT_FAILURE_EXPECTATIONS.iter().copied());
    let mut expected_dylint_sources = source_set(DYLINT_FAILURE_EXPECTATIONS);
    expected_dylint_sources.extend(string_set(DYLINT_PASS_SOURCES.iter().copied()));
    let actual_dylint_stderr = collect_from_roots(root, &[DYLINT_UI_ROOT], "stderr")?;
    let actual_dylint_sources = collect_from_roots(root, &[DYLINT_UI_ROOT], "rs")?
        .into_iter()
        .filter(|path| !path.split('/').any(|component| component == "auxiliary"))
        .collect::<BTreeSet<_>>();
    require_exact_inventory(
        &actual_dylint_stderr,
        &expected_dylint_stderr,
        "missing detached Dylint expectation",
        "unexpected detached Dylint expectation",
    )?;
    require_exact_inventory(
        &actual_dylint_sources,
        &expected_dylint_sources,
        "missing detached Dylint fixture",
        "unexpected detached Dylint fixture",
    )?;

    for relative in expected_root_stderr
        .iter()
        .chain(expected_dylint_stderr.iter())
    {
        reject_forbidden_paths(root, relative)?;
    }

    Ok(Inventory {
        inspection_stderr: expected_root_stderr
            .iter()
            .filter(|path| path.starts_with("crates/gaze-inspection/"))
            .count(),
        gaze_stderr: expected_root_stderr
            .iter()
            .filter(|path| path.starts_with("crates/gaze/tests/"))
            .count(),
        mcp_core_stderr: expected_root_stderr
            .iter()
            .filter(|path| path.starts_with("crates/gaze-mcp-core/"))
            .count(),
        dylint_sources: expected_dylint_sources.len(),
        dylint_stderr: expected_dylint_stderr.len(),
        dylint_pass: DYLINT_PASS_SOURCES.len(),
    })
}

fn collect_from_roots(
    root: &Path,
    relative_roots: &[&str],
    extension: &str,
) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    for relative_root in relative_roots {
        collect_files(root, &root.join(relative_root), extension, &mut files)?;
    }
    Ok(files)
}

fn collect_files(
    root: &Path,
    directory: &Path,
    extension: &str,
    files: &mut BTreeSet<String>,
) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", relative_path(root, directory)))?
    {
        let entry = entry.with_context(|| {
            format!("failed to read entry in {}", relative_path(root, directory))
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, extension, files)?;
        } else if path
            .extension()
            .is_some_and(|candidate| candidate == extension)
        {
            files.insert(relative_path(root, &path));
        }
    }
    Ok(())
}

fn require_exact_inventory(
    actual: &BTreeSet<String>,
    expected: &BTreeSet<String>,
    missing_reason: &str,
    extra_reason: &str,
) -> Result<()> {
    if let Some(path) = expected.difference(actual).next() {
        bail!("trybuild_fixture_hygiene: {path}: {missing_reason}");
    }
    if let Some(path) = actual.difference(expected).next() {
        bail!("trybuild_fixture_hygiene: {path}: {extra_reason}");
    }
    Ok(())
}

fn reject_forbidden_paths(root: &Path, relative: &str) -> Result<()> {
    let content = fs::read_to_string(root.join(relative))
        .with_context(|| format!("failed to read {relative}"))?;
    let lowered = content.to_ascii_lowercase();
    let reason = if content.contains("$RUST/") {
        Some("forbidden sysroot placeholder `$RUST/`")
    } else if content.contains("/rustc/") {
        Some("forbidden raw rustc sysroot path `/rustc/<hash>`")
    } else if lowered.contains("/opt/homebrew/") || lowered.contains("/cellar/") {
        Some("forbidden Homebrew/Cellar path")
    } else if lowered.contains("/users/")
        || lowered.contains("/home/")
        || lowered.contains("/var/home/")
        || lowered.contains(":\\users\\")
        || lowered.contains(":/users/")
    {
        Some("forbidden absolute user/home path")
    } else {
        None
    };
    if let Some(reason) = reason {
        bail!("trybuild_fixture_hygiene: {relative}: {reason}");
    }
    Ok(())
}

fn string_set<'a>(paths: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    paths.into_iter().map(str::to_owned).collect()
}

fn source_set(expectations: &[&str]) -> BTreeSet<String> {
    expectations
        .iter()
        .map(|path| with_extension(path, "rs"))
        .collect()
}

fn with_extension(path: &str, extension: &str) -> String {
    let mut path = PathBuf::from(path);
    path.set_extension(extension);
    normalized_path(&path)
}

fn relative_path(root: &Path, path: &Path) -> String {
    normalized_path(path.strip_prefix(root).unwrap_or(path))
}

fn normalized_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture path has a parent"))
            .expect("create fixture parent");
        fs::write(path, content).expect("write fixture");
    }

    fn valid_tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("tempdir");
        for expectation in ROOT_EXPECTATIONS {
            write(root.path(), expectation, "error[E0000]: synthetic\n");
            write(
                root.path(),
                &with_extension(expectation, "rs"),
                "fn main() {}\n",
            );
        }
        for expectation in DYLINT_FAILURE_EXPECTATIONS {
            write(root.path(), expectation, "error: synthetic lint\n");
            write(
                root.path(),
                &with_extension(expectation, "rs"),
                "fn main() {}\n",
            );
        }
        for source in DYLINT_PASS_SOURCES {
            write(root.path(), source, "fn main() {}\n");
        }
        root
    }

    #[test]
    fn accepts_exact_root_and_detached_dylint_inventories() {
        let root = valid_tree();
        assert_eq!(
            verify(root.path()).expect("valid inventory"),
            Inventory {
                inspection_stderr: 13,
                gaze_stderr: 3,
                mcp_core_stderr: 3,
                dylint_sources: 18,
                dylint_stderr: 16,
                dylint_pass: 2,
            }
        );
    }

    #[test]
    fn rejects_forbidden_sysroot_token_with_exact_path_and_reason() {
        let root = valid_tree();
        let path = ROOT_EXPECTATIONS[0];
        write(root.path(), path, "--> $RUST/core/src/marker.rs\n");
        let error = verify(root.path()).expect_err("forbidden token must fail");
        let message = error.to_string();
        assert!(message.contains(path));
        assert!(message.contains("`$RUST/`"));
    }

    #[test]
    fn rejects_missing_root_expectation_with_exact_path() {
        let root = valid_tree();
        let path = ROOT_EXPECTATIONS[0];
        fs::remove_file(root.path().join(path)).expect("remove expectation");
        let message = verify(root.path())
            .expect_err("missing expectation must fail")
            .to_string();
        assert!(message.contains(path));
        assert!(message.contains("missing root trybuild expectation"));
    }

    #[test]
    fn rejects_extra_root_expectation_with_exact_path() {
        let root = valid_tree();
        let expectation = "crates/gaze/tests/ui/unexpected.stderr";
        write(root.path(), expectation, "error: extra\n");
        write(
            root.path(),
            &with_extension(expectation, "rs"),
            "fn main() {}\n",
        );
        let message = verify(root.path())
            .expect_err("extra expectation must fail")
            .to_string();
        assert!(message.contains(expectation));
        assert!(message.contains("unexpected root trybuild expectation"));
    }

    #[test]
    fn rejects_detached_dylint_homebrew_path() {
        let root = valid_tree();
        let path = DYLINT_FAILURE_EXPECTATIONS[0];
        write(
            root.path(),
            path,
            "--> /opt/homebrew/Cellar/rust/library.rs\n",
        );
        let message = verify(root.path())
            .expect_err("host path must fail")
            .to_string();
        assert!(message.contains(path));
        assert!(message.contains("Homebrew/Cellar"));
    }
}
