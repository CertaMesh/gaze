# Xtask

`crates/xtask` is Gaze's internal gate runner. It is not published; it exists
to make repository checks explicit and repeatable.

Run gates from the workspace root:

```console
$ cargo run -p xtask -- symmetric-potemkin
$ cargo run -p xtask -- class-map-override-safety
$ cargo run -p xtask -- recognizer-composition-validator
$ cargo run -p xtask -- no-tenant-knowledge
$ cargo run -p xtask -- bundle-tokenization-drift
$ cargo run -p xtask -- fixture-citation-lint
$ cargo run -p xtask -- ci-feature-matrix
$ cargo run -p xtask -- cargo-metadata-audit-isolation
$ cargo run -p xtask -- dylint-gate
```

The gate list lives in [`crates/xtask/src/main.rs`](../../crates/xtask/src/main.rs).

## Current gates

| Gate | Command | Current behavior |
|------|---------|------------------|
| `SymmetricPotemkin` | `cargo run -p xtask -- symmetric-potemkin` | Lists and runs the behavioral tests in `SYMMETRIC_POTEMKIN_TESTS`. The gate fails if any named test is missing or fails. |
| `ClassMapOverrideSafety` | `cargo run -p xtask -- class-map-override-safety` | Activated in v0.4.4. Lists and runs `t20_context_class_map_overrides_policy_dict_class` and `t20a_class_map_override_fails_closed_when_action_rule_uncovered`. An adversarial in-PR self-test verifies the gate fails non-zero when a listed test is missing or renamed, following the meta-Potemkin guard captured in drawer `gaze_architecture_12b32d53`. |
| `RecognizerCompositionValidator` | `cargo run -p xtask -- recognizer-composition-validator` | Lists and runs the behavioral tests in `RECOGNIZER_COMPOSITION_VALIDATOR_TESTS`. The gate fails if the rulepack composition validator tests are missing or failing. |
| `NoTenantKnowledge` | `cargo run -p xtask -- no-tenant-knowledge` | Added in v0.4.3. Production-code lint scanner that rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Allow markers (`// allow(tenant-fixture)`) hard-fail in production scope and remain valid only in tests, benches, docs, and `CONTRIBUTING.md`. |
| `BundleTokenizationDrift` | `cargo run -p xtask -- bundle-tokenization-drift` | Added in v0.4.6. Discovers recognizer-bearing bundled rulepacks from `crates/gaze-recognizers/embedded/*.toml`, runs the real `gaze clean --rulepack-bundled <bundle> --audit-db <tmp>` path against `crates/xtask/fixtures/drift-corpus.txt`, restores emitted tokens to infer byte spans, and compares canonical no-policy tokenization metadata to `crates/xtask/snapshots/<bundle>-no-policy.json`. Snapshots exclude raw values, `session_blob`, and audit `created_at`. `--verify-ack` fails closed when snapshot changes lack both a `// drift-ack:` source/test comment and a `[bundle-tokenization-drift]` line in the `[Unreleased]` `### Changed` section of `CHANGELOG.md`. |
| `FixtureCitationLint` | `cargo run -p xtask -- fixture-citation-lint` | Added in v0.4.6 S2. Production-code lint scanner for fixture-shaped PII literals in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Each production fixture literal must carry `// fixture-cited(<test-path>:<fully-qualified-test-name>)`, and the fully qualified test name must appear exactly in `cargo test --workspace -- --list`. |
| `CiFeatureMatrix` | `cargo run -p xtask -- ci-feature-matrix` | Added in v0.4.6 S5. Runs the CI feature matrix, including the no-phone-parser fail-closed configuration. |
| `CargoMetadataAuditIsolation` | `cargo run -p xtask -- cargo-metadata-audit-isolation` | Added in v0.5 Phase C and updated in v0.6 after the `gaze` audit feature shim was removed. Parses `cargo metadata --format-version=1` and fails if any non-audit-responsible workspace package has a normal dependency path to `gaze-audit` in default, `--no-default-features`, or safety-net graphs. The explicit audit-responsible allowlist is documented in source; currently `gaze-cli` is allowed because its audit command consumes the passive sink directly. |
| `DylintGate` | `cargo run -p xtask -- dylint-gate` | Added in v0.5 Phase D. Verifies the `xtask/dylint/ui` fixture corpus has exactly 18 enabled fixtures, rejects `*_disabled.rs`, and runs `cargo dylint --workspace --all` when `cargo-dylint` is installed. The lint is `GAZE_MODULE_ISOLATION`, the canonical rustc-resolver-based gate for audit-sink protected-path isolation. |

## dylint_gate

The `dylint_gate` command enforces the Phase D resolver-based audit isolation
lint. It is CI-only in practice because local developer machines may not have
`cargo-dylint`; the wrapper skips locally with a clear message when
`cargo-dylint` is absent.

CI runs:

```console
$ cd xtask/dylint && cargo test --test ui
$ cd ../..
$ cargo run -p xtask -- dylint-gate
```

The UI suite covers 18 bypass classes, including macro call-site hygiene,
`#[path]` modules, `include!()`, type positions, trait bounds, and clean
controls. This is the source of truth for audit-sink protected-path isolation;
the legacy `audit-metadata-only` syn walker was decommissioned in v0.5 Phase E.
The architecture, toolchain pins, timings, and Phase E migration plan are documented in
[`docs/research/v0.5-dylint-audit-gate.md`](../research/v0.5-dylint-audit-gate.md).

## cargo_metadata_audit_isolation self-test

The `cargo_metadata_audit_isolation` gate protects the crate boundary:
`gaze-audit` owns the `rusqlite` sink, while `gaze` stays free of audit-sink
normal dependencies in every shipped feature graph.

Adversarial self-test for reviewers:

1. On a throwaway branch, add `gaze-audit = { workspace = true }` to
   `crates/gaze/Cargo.toml` under `[dependencies]`.
2. Run `cargo run -p xtask -- cargo-metadata-audit-isolation`.
3. Confirm the gate exits non-zero and names `gaze` with a path to
   `gaze-audit`.
4. Revert the throwaway `Cargo.toml` edit.

The gate walks normal dependency edges from `cargo metadata`; development
dependencies are ignored so `gaze` contract tests can depend on `gaze-audit`
without weakening the shipped default and no-default graphs.

## bundle-tokenization-drift adversarial self-test

Run the clean gate first:

```console
$ cargo run -p xtask -- bundle-tokenization-drift
```

Then rehearse failure on a throwaway branch or detached worktree:

1. Temporarily rename an enabled recognizer id in `crates/gaze-recognizers/embedded/core-extended.toml`, for example `id = "ip.v4"` to `id = "ip.v4.drift"`.
2. Run `cargo run -p xtask -- bundle-tokenization-drift`.
3. Confirm the gate exits non-zero and names `core-extended`, the old/new recognizer id, `custom:ip_address`, and the changed count.
4. Revert the TOML edit.

To intentionally update snapshots, run `cargo run -p xtask -- bundle-tokenization-drift --regenerate-baseline`, add or update a nearby `// drift-ack:` source/test comment, and add a `[bundle-tokenization-drift]` line naming each changed bundle under `[Unreleased]` `### Changed` in `CHANGELOG.md`. Then run `cargo run -p xtask -- bundle-tokenization-drift --verify-ack`.

## fixture_citation_lint self-test + limitation

The fixture citation gate prevents production code from accumulating
uncited fixture-shaped PII literals. It intentionally scans the same crate
source roots as `no_tenant_knowledge`, while excluding Rust regions compiled
only under `#[cfg(test)]`.

Adversarial self-test for reviewers:

1. On a throwaway branch, add a production-scope fixture literal such as
   `"alice@example.invalid"` with a valid-looking marker:
   `// fixture-cited(crates/gaze/tests/email.rs:gaze::tests::email_round_trip)`.
2. Run `cargo run -p xtask -- fixture-citation-lint`; it must fail unless
   `cargo test --workspace -- --list` contains the cited test name exactly.
3. If the cited test exists, temporarily rename the test, rerun the gate, and
   confirm it exits non-zero with `FixtureCitationMissingTest`.
4. Revert the throwaway changes before merging.

Known limitation: the gate proves that the cited test exists. It does not prove
that the test body still asserts the specific fixture literal. That deeper
semantic tie is out of scope for the v0.4.6 S2 two-point lint gate and remains
a code-review responsibility.

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
