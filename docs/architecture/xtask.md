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
```

The gate list lives in [`crates/xtask/src/main.rs`](../../crates/xtask/src/main.rs).

## Current gates

| Gate | Command | Current behavior |
|------|---------|------------------|
| `SymmetricPotemkin` | `cargo run -p xtask -- symmetric-potemkin` | Lists and runs the behavioral tests in `SYMMETRIC_POTEMKIN_TESTS`. The gate fails if any named test is missing or fails. |
| `ClassMapOverrideSafety` | `cargo run -p xtask -- class-map-override-safety` | Activated in v0.4.4. Lists and runs `t20_context_class_map_overrides_policy_dict_class` and `t20a_class_map_override_fails_closed_when_action_rule_uncovered`. An adversarial in-PR self-test verifies the gate fails non-zero when a listed test is missing or renamed, following the meta-Potemkin guard captured in drawer `gaze_architecture_12b32d53`. |
| `RecognizerCompositionValidator` | `cargo run -p xtask -- recognizer-composition-validator` | Lists and runs the behavioral tests in `RECOGNIZER_COMPOSITION_VALIDATOR_TESTS`. The gate fails if the rulepack composition validator tests are missing or failing. |
| `NoTenantKnowledge` | `cargo run -p xtask -- no-tenant-knowledge` | Added in v0.4.3. Production-code lint scanner that rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Allow markers (`// allow(tenant-fixture)`) hard-fail in production scope and remain valid only in tests, benches, docs, and `CONTRIBUTING.md`. |
| `BundleTokenizationDrift` | `cargo run -p xtask -- bundle-tokenization-drift` | Added in v0.4.6. Discovers recognizer-bearing bundled rulepacks from `crates/gaze-recognizers/embedded/*.toml`, runs the real `gaze clean --rulepack-bundled <bundle> --audit-db <tmp>` path against `crates/xtask/fixtures/drift-corpus.txt`, restores emitted tokens to infer byte spans, and compares canonical no-policy tokenization metadata to `crates/xtask/snapshots/<bundle>-no-policy.json`. Snapshots exclude raw values, `session_blob`, and audit `created_at`. `--verify-ack` fails closed when snapshot changes lack both a `// drift-ack:` source/test comment and a `[bundle-tokenization-drift]` line in the `[Unreleased]` `### Changed` section of `CHANGELOG.md`. |
| `FixtureCitationLint` | `cargo run -p xtask -- fixture-citation-lint` | Added in v0.4.6 S2. Production-code lint scanner for fixture-shaped PII literals in `crates/{gaze,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Each production fixture literal must carry `// fixture-cited(<test-path>:<fully-qualified-test-name>)`, and the fully qualified test name must appear exactly in `cargo test --workspace -- --list`. |
| `CiFeatureMatrix` | `cargo run -p xtask -- ci-feature-matrix` | Added in v0.4.6 S5. Runs the CI feature matrix, including the no-phone-parser fail-closed configuration. |

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

## audit_metadata_only — known limitations (v0.4.5)

The `audit_metadata_only` xtask gate enforces "no audit metadata symbols
imported in restore paths" via syn-based AST scanning. As of v0.4.5 round-3,
the gate covers:

- File-scope `Item::Use` (top-level + nested inline `mod { ... }`)
- Function body / impl method body `use` statements
- Trait default-method body `use` statements (covered by walker; behavioral test added in this round: `audit_metadata_only_fails_on_trait_default_method_body_use`)
- Const/static initializer block-expression `use` statements (covered by walker; behavioral test added in this round: `audit_metadata_only_fails_on_const_block_initializer_use`)
- `Item::ExternCrate { gaze | gaze_cli }` (with or without alias)
- Glob imports `use gaze::*;` / `use gaze_cli::*;` / `use crate::*;` (expand to all denylist symbols)
- Aliased crate `use gaze as <ident>;` (synthetic `__renamed_gaze_root__` marker)
- External modules declared via `#[path = "..."]` (rustc-style file resolution)
- All denylist symbols including forward-guards (`AuditFilter`, `AuditLogRow`, `AUDIT_RESTRICTED_COLUMNS`, `build_audit_query_sql`, `current_epoch_ms`)

Known limitations (v0.4.5 — accepted-risk; covered by code review):

1. **Macro-emitted use statements.** `macro_rules! pull_audit { () => { use gaze::RedactionEntry; }; } pull_audit!();` — the gate does not expand macros. Code review must catch macros that emit forbidden imports. Future: see `docs/research/v0.5-dylint-audit-gate.md` (todo #181).
2. **`use super::*;` re-export chains.** A submodule glob-importing from a `super` that re-exports audit symbols would bypass the gate. Currently no such re-exports exist, but no defense in depth.
3. **Indirect references via name-resolution edge cases** (for example, `extern crate alloc as gaze;`). Rare and would be obvious in code review.
4. **Fully-qualified path references without `use` statement.** A restore module can reference an audit symbol via fully-qualified path WITHOUT importing it: `let _ = std::marker::PhantomData::<gaze::RedactionEntry>;` or `let _ = gaze::current_epoch_ms();` or `fn x(_: gaze::AuditFilter)`. The walker scans `Item::Use` and `Item::ExternCrate` and recursively walks block statements for nested `use`, but does NOT inspect `syn::Path` references in type positions, expression positions, function signatures, return types, struct fields, or generic args. Closing this requires walking `syn::Path` references via `syn::visit::Visit` (which is essentially recreating rustc's name resolver — see todo #181 v0.5 dylint pivot for the architectural answer).
5. **`include!("...")` macro inlining sibling files.** `include!("../external_inc.rs")` at module scope inlines a sibling file's source into the current module. The walker hits `Item::Macro` and falls through; the included file may live outside the scanned restore tree. Distinct from `macro_rules!` emission (limitation #1) — `include!` doesn't emit, it inlines. The reviewer skimming for `macro_rules!` definitions would miss this class.
6. **`let-else` diverge block use statements.** `let Some(_x) = predicate else { use gaze::RedactionEntry; ... };` — the `else { ... }` block is `Local::init.diverge` in the syn AST. The walker walks `local.init.expr` but not the diverge block. Stable Rust since 1.65.

Architectural roadmap: todo #181 schedules a v0.5 rewrite using
rustc-resolver-based lint (likely `dylint`). That replaces the syn-walker
entirely and eliminates this class of recursive-Potemkin risk.

**Naming caveat:** the gate is named `audit_metadata_only` to match its goal ("only non-audit metadata may live in restore"), but its actual enforcement is narrower: "no audit symbols imported via `use` or `extern crate` in restore". Adopters should NOT trust the name alone — read the limitations above. The v0.5 dylint pivot (#181) closes this gap by switching to rustc-resolver-based enforcement.

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
