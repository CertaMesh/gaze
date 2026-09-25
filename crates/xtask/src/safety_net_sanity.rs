use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};

// One more than the suite count: the Nym suite runs a second, live pass when a bundle is set.
const MAX_CARGO_TEST_INVOCATIONS: usize = 6;

/// Pinned Nym bundle for the live pass. Unset: the Nym suite still runs its captured
/// real-model fixtures, and the live pass is reported as not run.
const NYM_MODEL_DIR_ENV: &str = "GAZE_NYM_MODEL_DIR";

pub fn run() -> Result<()> {
    let suites = suites();
    if suites.len() + 1 > MAX_CARGO_TEST_INVOCATIONS {
        bail!(
            "safety_net_sanity: expected at most {MAX_CARGO_TEST_INVOCATIONS} batched cargo test invocations, got {}",
            suites.len()
        );
    }
    println!(
        "safety_net_sanity: checking {} behavioral tests across {} batched cargo test invocations",
        suites
            .iter()
            .map(|suite| suite.required_tests.len())
            .sum::<usize>(),
        suites.len()
    );
    for suite in &suites {
        ensure_required_tests_exist(suite)?;
    }
    for suite in &suites {
        run_suite(suite)?;
    }
    run_live_nym_pass(&suites)?;
    println!("safety_net_sanity: passed");
    Ok(())
}

#[derive(Debug)]
struct Suite {
    label: &'static str,
    package: &'static str,
    test_target: &'static str,
    features: &'static [&'static str],
    required_tests: &'static [&'static str],
}

fn suites() -> Vec<Suite> {
    vec![
        Suite {
            label: "core safety-net manifest and structured behavior",
            package: "gaze-pii",
            test_target: "safety_net",
            features: &["safety-net"],
            required_tests: &[
                "byte_equal_invariance_for_leak_kinds_and_locale_skip",
                "structured_safety_net_traverses_nested_fields_and_preserves_shape",
                // Reversible follow-up must use current residual coordinates and retain tokens.
                // Separate fallback-trace and first-refusal tests still prove actual deletion.
                "resolve_followup_uses_the_residual_report_not_the_stale_primary_report",
                "resolve_fallback_redacts_the_residual_without_deleting_protected_live_tokens",
                "resolve_followup_does_not_act_on_stale_pre_resolve_spans",
                "first_pass_refusal_redacts_only_the_exposed_suspect",
                "fallback_redaction_is_traced_as_fallback_redact",
            ],
        },
        Suite {
            label: "CLI safety-net strict/tolerant behavior",
            package: "gaze-cli",
            test_target: "safety_net_cli",
            features: &["safety-net-openai"],
            required_tests: &[
                "missing_checkpoint_fails_closed_with_sanitized_error",
                "uncovered_suspect_exits_three_in_strict_mode_without_stdout",
                "class_mismatch_inside_owned_token_is_dropped",
                "tolerant_uncovered_outputs_report_and_logs_audit_row",
                "safety_net_audit_query_filters_structured_field_path",
            ],
        },
        Suite {
            label: "OpenAI subprocess boundary safety",
            package: "gaze-recognizers",
            test_target: "openai_filter_subprocess",
            features: &["safety-net-openai"],
            required_tests: &[
                "all_official_labels_map_exactly_to_gaze_classes",
                "verbose_stderr_is_stripped_and_capped",
                "missing_checkpoint_fails_closed_without_spawn_or_download",
                "safety_net_correlates_raw_spans_with_manifest_without_source_text",
            ],
        },
        Suite {
            label: "Nym-small backend on captured real-model output",
            package: "gaze-recognizers",
            test_target: "nym_safety_net",
            features: &["safety-net-nym", "test-support"],
            required_tests: &[
                "captured_plate_in_prose_is_one_suspect",
                "captured_salutation_sentence_has_no_suspects",
                "captured_multibyte_offsets_land_on_whole_words",
                "captured_long_document_flags_the_tail",
            ],
        },
        Suite {
            label: "safety_net_log metadata-only schema",
            package: "gaze-audit",
            test_target: "safety_net_log",
            features: &[],
            required_tests: &[
                "restricted_columns_have_no_raw_payload_fields",
                "safety_net_log_does_not_persist_suspect_or_placeholder_bytes",
            ],
        },
    ]
}

/// Live pass: the ignored `live_*` tests of the Nym suite load the real pinned bundle, check a
/// positive and a negative sentence end to end, and prove the committed fixture is still what the
/// model outputs. They need the 150 MB bundle, so they run only when `GAZE_NYM_MODEL_DIR` is set.
fn run_live_nym_pass(suites: &[Suite]) -> Result<()> {
    if std::env::var_os(NYM_MODEL_DIR_ENV).is_none() {
        println!(
            "safety_net_sanity: nym live pass NOT RUN ({NYM_MODEL_DIR_ENV} unset); captured fixtures ran above"
        );
        return Ok(());
    }
    let suite = suites
        .iter()
        .find(|suite| suite.test_target == "nym_safety_net")
        .context("nym suite missing")?;
    println!("safety_net_sanity: running nym live pass against the pinned bundle");
    let output = cargo_test_command(suite)
        .arg("--")
        .arg("--ignored")
        .arg("live_")
        .output()
        .context("failed to run the nym live pass")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{stdout}");
    if !output.status.success() {
        bail!(
            "safety_net_sanity: nym live pass failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // A filter that matched nothing exits 0; require every live test by name.
    for required in [
        "live_plate_in_prose_is_a_suspect",
        "live_salutation_sentence_has_no_suspects",
        "live_long_document_is_scanned_to_the_end",
        "live_capture_matches_the_committed_fixture",
    ] {
        if !stdout
            .lines()
            .any(|line| line == format!("test {required} ... ok"))
        {
            bail!("safety_net_sanity: nym live test `{required}` did not run and pass");
        }
    }
    Ok(())
}

fn ensure_required_tests_exist(suite: &Suite) -> Result<()> {
    let output = cargo_test_command(suite)
        .arg("--")
        .arg("--list")
        .output()
        .with_context(|| format!("failed to list tests for {}", suite.label))?;
    if !output.status.success() {
        bail!(
            "safety_net_sanity: failed to list {}: {}",
            suite.label,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for required in suite.required_tests {
        let expected = format!("{required}: test");
        if !stdout.lines().any(|line| line == expected) {
            bail!(
                "safety_net_sanity: missing required test `{}` in {}",
                required,
                suite.label
            );
        }
    }
    Ok(())
}

fn run_suite(suite: &Suite) -> Result<()> {
    println!("safety_net_sanity: running {}", suite.label);
    let status = cargo_test_command(suite)
        .status()
        .with_context(|| format!("failed to run {}", suite.label))?;
    if !status.success() {
        bail!("safety_net_sanity: suite failed: {}", suite.label);
    }
    Ok(())
}

fn cargo_test_command(suite: &Suite) -> ProcessCommand {
    let mut command = ProcessCommand::new("cargo");
    command
        .arg("test")
        .arg("-p")
        .arg(suite.package)
        .arg("--test")
        .arg(suite.test_target);
    if !suite.features.is_empty() {
        command.arg("--features").arg(suite.features.join(","));
    }
    command
}
