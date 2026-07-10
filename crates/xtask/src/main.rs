use std::collections::{hash_map::Entry, HashMap};
use std::process::Command as ProcessCommand;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

mod bundle_tokenization_drift;
mod cargo_metadata_audit_isolation;
mod ci_feature_matrix;
mod class_map_override_safety;
mod coverage_corpus;
mod dylint_gate;
mod family_policy_coherence;
mod fixture_citation;
mod locale_cue_bundle_coherence;
mod mcp_tier_isolation;
mod negative_corpus;
mod no_tenant_knowledge;
mod publish_plan;
mod readme_version_check;
mod safety_net_sanity;
mod scrub_public_text;
mod tokenbridge_encrypted_index;
mod tokenbridge_no_raw_index;

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
    FixtureCitationLint,
    FamilyPolicyTableCoherence,
    CargoMetadataAuditIsolation,
    DylintGate,
    BundleTokenizationDrift(bundle_tokenization_drift::Args),
    LocaleCueBundleCoherence,
    CoverageCorpus(coverage_corpus::Args),
    CiFeatureMatrix,
    SafetyNetSanity,
    TokenbridgeEncryptedIndex,
    TokenbridgeNoRawIndex(tokenbridge_no_raw_index::Args),
    McpTierIsolation,
    GenerateNegativeCorpus(negative_corpus::Args),
    PublishPlan,
    ReadmeVersionCheck,
    ScrubPublicText(scrub_public_text::Args),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::SymmetricPotemkin => run_symmetric_potemkin_gate(),
        Command::ClassMapOverrideSafety => run_class_map_override_safety_gate(),
        Command::RecognizerCompositionValidator => run_recognizer_composition_validator_gate(),
        Command::NoTenantKnowledge => no_tenant_knowledge::run(),
        Command::FixtureCitationLint => fixture_citation::run(),
        Command::FamilyPolicyTableCoherence => family_policy_coherence::run(),
        Command::CargoMetadataAuditIsolation => cargo_metadata_audit_isolation::run(),
        Command::DylintGate => dylint_gate::run(),
        Command::BundleTokenizationDrift(args) => bundle_tokenization_drift::run(args),
        Command::LocaleCueBundleCoherence => locale_cue_bundle_coherence::run(),
        Command::CoverageCorpus(args) => coverage_corpus::run(args),
        Command::CiFeatureMatrix => ci_feature_matrix::run(),
        Command::SafetyNetSanity => safety_net_sanity::run(),
        Command::TokenbridgeEncryptedIndex => tokenbridge_encrypted_index::run(),
        Command::TokenbridgeNoRawIndex(args) => tokenbridge_no_raw_index::run(args),
        Command::McpTierIsolation => mcp_tier_isolation::run(),
        Command::GenerateNegativeCorpus(args) => negative_corpus::run(args),
        Command::PublishPlan => publish_plan::run(),
        Command::ReadmeVersionCheck => readme_version_check::run(),
        Command::ScrubPublicText(args) => scrub_public_text::run(args),
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
        package: "gaze-pii",
        test_target: None,
        name: "pipeline::tests::t21d_token_family_threads_from_recognizer_to_session",
    },
    BehavioralTest {
        package: "gaze-pii",
        test_target: None,
        name: "session::tests::snapshot_round_trip_two_families_same_class_raw_preserved_under_shared_counter",
    },
    BehavioralTest {
        package: "gaze-pii",
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
        package: "gaze-pii",
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
        package: "gaze-pii",
        test_target: None,
        name: "rulepack::tests::rulepack_load_fails_when_two_name_recognizers_omit_cooperates_with",
    },
    BehavioralTest {
        package: "gaze-pii",
        test_target: None,
        name: "rulepack::tests::rulepack_load_accepts_same_class_pair_with_cooperates_with",
    },
];

fn run_symmetric_potemkin_gate() -> Result<()> {
    println!(
        "symmetric_potemkin_gate: checking {} behavioral tests",
        SYMMETRIC_POTEMKIN_TESTS.len()
    );
    ensure_tests_exist(SYMMETRIC_POTEMKIN_TESTS)?;
    for test in SYMMETRIC_POTEMKIN_TESTS {
        run_behavioral_test("symmetric_potemkin_gate", *test)?;
    }
    println!("symmetric_potemkin_gate: passed");
    Ok(())
}

fn run_class_map_override_safety_gate() -> Result<()> {
    class_map_override_safety::run()
}

fn run_recognizer_composition_validator_gate() -> Result<()> {
    println!(
        "recognizer_composition_validator: checking {} behavioral tests",
        RECOGNIZER_COMPOSITION_VALIDATOR_TESTS.len()
    );
    ensure_tests_exist(RECOGNIZER_COMPOSITION_VALIDATOR_TESTS)?;
    for test in RECOGNIZER_COMPOSITION_VALIDATOR_TESTS {
        run_behavioral_test("recognizer_composition_validator", *test)?;
    }
    println!("recognizer_composition_validator: passed");
    Ok(())
}

fn ensure_tests_exist(tests: &[BehavioralTest]) -> Result<()> {
    let mut listed = HashMap::<(&'static str, Option<&'static str>), String>::new();

    for test in tests {
        let key = (test.package, test.test_target);
        let stdout = match listed.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(list_tests(*test)?),
        };
        let expected_line = format!("{}: test", test.name);
        if !stdout.lines().any(|line| line == expected_line) {
            bail!("missing behavioral test: {}", describe(*test));
        }
    }

    Ok(())
}

fn ensure_test_exists(test: BehavioralTest) -> Result<()> {
    ensure_tests_exist(&[test])
}

fn list_tests(test: BehavioralTest) -> Result<String> {
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
    String::from_utf8(output.stdout)
        .with_context(|| format!("failed to decode test list for {}", describe(test)))
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
