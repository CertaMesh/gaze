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
$ cargo run -p xtask -- no-tenant-knowledge
$ cargo run -p xtask -- bundle-tokenization-drift
$ cargo run -p xtask -- family-policy-table-coherence
$ cargo run -p xtask -- locale-cue-bundle-coherence
$ cargo run -p xtask -- fixture-citation-lint
$ cargo run -p xtask -- ci-feature-matrix
$ cargo run -p xtask -- cargo-metadata-audit-isolation
$ cargo run -p xtask -- dylint-gate
$ cargo run -p xtask -- safety-net-sanity
```

Clap converts enum variants to kebab-case command names.

The canonical active-gate roster is the "Active xtask gates" line in
[`CLAUDE.md`](../../CLAUDE.md); keep this README and
[`docs/architecture/xtask.md`](../../docs/architecture/xtask.md) in sync with
that list.

## Gates

| Gate | Command | Behavior |
|------|---------|----------|
| `SymmetricPotemkin` | `symmetric-potemkin` | Checks that every named behavioral test in `SYMMETRIC_POTEMKIN_TESTS` exists, then runs each exact test. |
| `ClassMapOverrideSafety` | `class-map-override-safety` | Checks that every named behavioral test in `CLASS_MAP_OVERRIDE_SAFETY_TESTS` exists, then runs each exact test. Activated in v0.4.4. |
| `RecognizerCompositionValidator` | `recognizer-composition-validator` | Checks that every named behavioral test in `RECOGNIZER_COMPOSITION_VALIDATOR_TESTS` exists, then runs each exact test. |
| `NoTenantKnowledge` | `no-tenant-knowledge` | Production-code lint scanner that rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. `// allow(tenant-fixture)` markers hard-fail in production scope. Added in v0.4.3. |
| `BundleTokenizationDrift` | `bundle-tokenization-drift` | Runs bundled rulepacks through the real CLI clean/audit path and compares metadata-only snapshots. `--verify-ack` requires source/test and changelog acknowledgement for drift. |
| `FamilyPolicyTableCoherence` | `family-policy-table-coherence` | Parses embedded rulepacks and checks collision-family declarations compile into the expected family precedence table, including IBAN-over-PAN and same-variant phone non-arbitration. |
| `LocaleCueBundleCoherence` | `locale-cue-bundle-coherence` | Checks every mandatory-anchor declaration in core bundles has a matching cue key in embedded locale bundles. |
| `FixtureCitationLint` | `fixture-citation-lint` | Production-code lint scanner for fixture-shaped PII literals. Each production fixture literal must cite a test that exists in `cargo test --workspace -- --list`. |
| `CiFeatureMatrix` | `ci-feature-matrix` | Runs the local feature matrix, including document-ingestion, MCP, safety-net, no-phone-parser, and gate wrapper checks. |
| `CargoMetadataAuditIsolation` | `cargo-metadata-audit-isolation` | Parses `cargo metadata` and rejects normal dependency paths from non-audit-responsible packages to `gaze-audit` across default, no-default-features, and safety-net graphs. |
| `DylintGate` | `dylint-gate` | Canonical audit-sink protected-path isolation gate. Verifies the Dylint UI fixture corpus and runs `cargo dylint --workspace --all` when `cargo-dylint` is installed. |
| `SafetyNetSanity` | `safety-net-sanity` | Runs mock-driven safety-net behavioral suites across core, recognizers, CLI, and audit. Nightly/live OPF corpus hardening is deferred to v0.6.2+ follow-up todo #328. |

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
