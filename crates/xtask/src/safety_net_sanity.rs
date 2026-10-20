use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};

use crate::test_commands::{
    cargo_test_command as shared_cargo_test_command, list_command_output, BehavioralTarget,
};

// Live Nym checks belong to test-support in test.yml, which installs the pinned bundle
// and requires every ignored live test to report success by name. This gate is model-free.
const MAX_CARGO_TEST_INVOCATIONS: usize = 5;

pub fn run() -> Result<()> {
    let suites = suites();
    if suites.len() > MAX_CARGO_TEST_INVOCATIONS {
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
                "captured_salutation_protects_surname_but_not_unsupported_secret",
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

fn ensure_required_tests_exist(suite: &Suite) -> Result<()> {
    let mut command = cargo_test_command(suite);
    command.args(["--", "--list"]);
    let stdout = list_command_output(command, suite.label)?;
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
    let mut command = shared_cargo_test_command(
        suite.package,
        BehavioralTarget::Integration(suite.test_target),
    );
    if !suite.features.is_empty() {
        command.arg("--features").arg(suite.features.join(","));
    }
    command
}
