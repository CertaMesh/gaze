use std::fs;
use std::path::PathBuf;

fn xtask_main() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")
}

#[test]
fn class_map_override_safety_gate_keeps_manual_attestation_contract() {
    let source = fs::read_to_string(xtask_main()).expect("read xtask main");

    assert!(
        source.contains("Command::ClassMapOverrideSafety => run_class_map_override_safety_gate()"),
        "class-map override safety command must dispatch to the behavioral gate"
    );
    assert!(
        !source.contains("class_map_override_safety: scaffolded"),
        "class-map override safety command must not regress to a scaffold"
    );
    assert!(
        source.contains("Adversarial self-test: reviewer manually renames one of the listed"),
        "manual reviewer-attestation guard must stay documented in source"
    );
    assert!(
        source.contains("gaze_architecture_12b32d53"),
        "source comment must retain the meta-Potemkin drawer reference"
    );
}

#[test]
fn class_map_override_safety_gate_lists_both_behavioral_tests() {
    let source = fs::read_to_string(xtask_main()).expect("read xtask main");

    for expected in [
        "tests::t20_context_class_map_overrides_policy_dict_class",
        "tests::t20a_class_map_override_fails_closed_when_action_rule_uncovered",
    ] {
        assert!(
            source.contains(expected),
            "class-map override safety gate must list behavioral test {expected}"
        );
    }
}

#[test]
fn class_map_override_safety_cross_cutting_rationale_is_build_time_only() {
    // Round-trip: N/A because this is a build-time gate and emits no tokens.
    // Three-surfaces: N/A because there is no runtime policy, CLI, or API knob.
    // cooperates_with: N/A because this gate does not define a recognizer.
    // No-tenant-knowledge: this file uses synthetic test names only.
    // CLI shipping smoke: N/A for recognizer paths; the smoke is `cargo run -p xtask`.
    let source = fs::read_to_string(xtask_main()).expect("read xtask main");

    assert!(
        source.contains("fn run_class_map_override_safety_gate()"),
        "build-time xtask gate must remain the only runtime surface"
    );
}
