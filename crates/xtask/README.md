# xtask

Internal gate runner for the Gaze repository.

This crate is not published (`publish = false`). It gives maintainers and CI a
stable place to run repository-specific checks that do not belong in the
library or CLI binaries.

## Run locally

From the workspace root:

```console
$ cargo run -p xtask -- symmetric-potemkin
$ cargo run -p xtask -- class-map-override-safety
$ cargo run -p xtask -- recognizer-composition-validator
```

Clap converts enum variants to kebab-case command names.

## Gates

| Gate | Command | Behavior |
|------|---------|----------|
| `SymmetricPotemkin` | `symmetric-potemkin` | Checks that every named behavioral test in `SYMMETRIC_POTEMKIN_TESTS` exists, then runs each exact test. |
| `ClassMapOverrideSafety` | `class-map-override-safety` | Checks that every named behavioral test in `CLASS_MAP_OVERRIDE_SAFETY_TESTS` exists, then runs each exact test. |
| `RecognizerCompositionValidator` | `recognizer-composition-validator` | Checks that every named behavioral test in `RECOGNIZER_COMPOSITION_VALIDATOR_TESTS` exists, then runs each exact test. |

The implementation lives in [`src/main.rs`](src/main.rs). The broader gate
catalog and gate-authoring rules are in
[docs/architecture/xtask.md](../../docs/architecture/xtask.md).

## CI integration

CI can call gates directly:

```console
$ cargo run -p xtask -- symmetric-potemkin
```

Each gate exits non-zero when:

- a protected test cannot be listed
- a protected test has been renamed or removed
- a protected test fails
- the underlying `cargo test` command cannot be started

## Adding a gate

Every gate must invoke behavioral tests. Do not add a gate that only checks for
symbols, files, or strings.

The current helper type is:

```rust
#[derive(Debug, Clone, Copy)]
struct BehavioralTest {
    package: &'static str,
    test_target: Option<&'static str>,
    name: &'static str,
}
```

A new gate should:

1. add a `Command` enum variant
2. add a `const` slice of `BehavioralTest`
3. call `ensure_test_exists` for every entry
4. call `run_behavioral_test` for every entry
5. print a clear passed line only after all tests pass

For integration tests, set `test_target: Some("target_name")`; for unit tests,
set `test_target: None`.

## Failure rehearsal

Before merging a new gate, temporarily rename one protected test and run the
gate. It should fail during the list phase before any passing subset can hide
the missing behavioral contract. Revert the temporary rename before commit.
