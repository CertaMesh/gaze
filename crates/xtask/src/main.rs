use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::SymmetricPotemkin => run_symmetric_potemkin_gate(),
        Command::ClassMapOverrideSafety => run_class_map_override_safety_gate(),
        Command::RecognizerCompositionValidator => run_recognizer_composition_validator_gate(),
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
        run_behavioral_test(*test)?;
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
        run_behavioral_test(*test)?;
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
        run_behavioral_test(*test)?;
    }
    println!("recognizer_composition_validator: passed");
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

fn run_behavioral_test(test: BehavioralTest) -> Result<()> {
    println!("behavioral_test: running {}", describe(test));
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
