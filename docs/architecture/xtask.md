# Xtask

`crates/xtask` is Gaze's internal gate runner. It is not published; it exists
to make repository checks explicit and repeatable.

Run gates from the workspace root:

```console
$ cargo run -p xtask -- symmetric-potemkin
$ cargo run -p xtask -- class-map-override-safety
$ cargo run -p xtask -- recognizer-composition-validator
$ cargo run -p xtask -- no-tenant-knowledge
```

The gate list lives in [`crates/xtask/src/main.rs`](../../crates/xtask/src/main.rs).

## Current gates

| Gate | Command | Current behavior |
|------|---------|------------------|
| `SymmetricPotemkin` | `cargo run -p xtask -- symmetric-potemkin` | Lists and runs the behavioral tests in `SYMMETRIC_POTEMKIN_TESTS`. The gate fails if any named test is missing or fails. |
| `ClassMapOverrideSafety` | `cargo run -p xtask -- class-map-override-safety` | Activated in v0.4.4. Lists and runs `t20_context_class_map_overrides_policy_dict_class` and `t20a_class_map_override_fails_closed_when_action_rule_uncovered`. An adversarial in-PR self-test verifies the gate fails non-zero when a listed test is missing or renamed, following the meta-Potemkin guard captured in drawer `gaze_architecture_12b32d53`. |
| `RecognizerCompositionValidator` | `cargo run -p xtask -- recognizer-composition-validator` | Lists and runs the behavioral tests in `RECOGNIZER_COMPOSITION_VALIDATOR_TESTS`. The gate fails if the rulepack composition validator tests are missing or failing. |
| `NoTenantKnowledge` | `cargo run -p xtask -- no-tenant-knowledge` | Added in v0.4.3. Production-code lint scanner that rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Allow markers (`// allow(tenant-fixture)`) hard-fail in production scope and remain valid only in tests, benches, docs, and `CONTRIBUTING.md`. |

## Recursive-Potemkin discipline

Every gate must invoke a behavioral test. A gate may check that a test exists,
but the final proof must be a real `cargo test` invocation of behavior that
would fail if the protected contract regressed.

Do not write gates that only assert symbol presence, file presence, or string
presence. Those checks can pass while the behavior is broken.

The current helper shape is:

- add one or more `BehavioralTest` entries with package, optional integration
  test target, and exact test name
- call `ensure_test_exists(test)` before running it
- call `run_behavioral_test(test)` to execute the exact test

This makes the gate recursive-Potemkin resistant: deleting or renaming the
test fails during the list phase, and breaking the contract fails during the
execution phase.

## Adding a gate

Add the gate in four places:

```rust
#[derive(Debug, Subcommand)]
enum Command {
    SymmetricPotemkin,
    ClassMapOverrideSafety,
    RecognizerCompositionValidator,
    NewBehaviorGate,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::SymmetricPotemkin => run_symmetric_potemkin_gate(),
        Command::ClassMapOverrideSafety => run_class_map_override_safety_gate(),
        Command::RecognizerCompositionValidator => run_recognizer_composition_validator_gate(),
        Command::NewBehaviorGate => run_new_behavior_gate(),
    }
}

const NEW_BEHAVIOR_TESTS: &[BehavioralTest] = &[
    BehavioralTest {
        package: "gaze",
        test_target: None,
        name: "module::tests::specific_contract_test",
    },
];

fn run_new_behavior_gate() -> Result<()> {
    println!(
        "new_behavior_gate: checking {} behavioral tests",
        NEW_BEHAVIOR_TESTS.len()
    );
    for test in NEW_BEHAVIOR_TESTS {
        ensure_test_exists(*test)?;
    }
    for test in NEW_BEHAVIOR_TESTS {
        run_behavioral_test(*test)?;
    }
    println!("new_behavior_gate: passed");
    Ok(())
}
```

Then run the gate and at least one failure rehearsal before opening a PR:

```console
$ cargo run -p xtask -- new-behavior-gate
```

Failure rehearsal means temporarily renaming the protected test, running the
gate, confirming it exits non-zero during the list phase, and reverting the
rename before commit.
