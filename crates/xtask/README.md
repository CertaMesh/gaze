# xtask

Internal gate runner for the Gaze repository.

Not published (`publish = false`). Maintainers and CI use it for repository checks.

## Run locally

Run `cargo run -p xtask -- <command>` from the workspace root. Commands are listed below.

Clap converts enum variants to kebab-case command names.

The canonical active-gate roster is the "Active xtask gates" line in
[`CLAUDE.md`](../../CLAUDE.md); keep this README and
[`docs/explanation/contributing/xtask-gates.md`](../../docs/explanation/contributing/xtask-gates.md) in sync with
that list.

## Gates

| Gate | Command | Behavior |
|------|---------|----------|
| `SymmetricPotemkin` | `symmetric-potemkin` | Checks that every named behavioral test in `SYMMETRIC_POTEMKIN_TESTS` exists, then runs each exact test. |
| `ClassMapOverrideSafety` | `class-map-override-safety` | Checks that every named behavioral test in `CLASS_MAP_OVERRIDE_SAFETY_TESTS` exists, then runs each exact test. |
| `RecognizerCompositionValidator` | `recognizer-composition-validator` | Checks that every named behavioral test in `RECOGNIZER_COMPOSITION_VALIDATOR_TESTS` exists, then runs each exact test. |
| `NoTenantKnowledge` | `no-tenant-knowledge` | Production-code lint scanner that rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. `// allow(tenant-fixture)` markers hard-fail in production scope. |
| `BundleTokenizationDrift` | `bundle-tokenization-drift` | Runs bundled rulepacks through the real CLI clean/audit path and compares metadata-only snapshots. `--verify-ack` requires source/test and changelog acknowledgement for drift. |
| `FamilyPolicyTableCoherence` | `family-policy-table-coherence` | Parses embedded rulepacks and checks collision-family declarations compile into the expected family precedence table, including IBAN-over-PAN and same-variant phone non-arbitration. |
| `LocaleCueBundleCoherence` | `locale-cue-bundle-coherence` | Checks every mandatory-anchor declaration in core bundles has a matching cue key in embedded locale bundles. |
| `FixtureCitationLint` | `fixture-citation-lint` | Production-code lint scanner for fixture-shaped PII literals. Each production fixture literal must cite a test that exists in `cargo test --workspace -- --list`. |
| `CiFeatureMatrix` | `ci-feature-matrix` | Runs the local feature matrix, including document-ingestion, MCP, safety-net, no-phone-parser, and gate wrapper checks. |
| `CargoMetadataAuditIsolation` | `cargo-metadata-audit-isolation` | Parses `cargo metadata` and rejects normal dependency paths from non-audit-responsible packages to `gaze-audit` across default, no-default-features, and safety-net graphs. |
| `ReadmeVersionCheck` | `readme-version-check` | Parses workspace package versions via `cargo metadata`, scans published crate READMEs for pinned Cargo/container versions, and fails when README pins drift from `Cargo.toml`. |
| `DylintGate` | `dylint-gate` | Canonical audit-sink protected-path isolation gate. Verifies the Dylint UI fixture corpus and runs `cargo dylint --workspace --all` when `cargo-dylint` is installed. |
| `SafetyNetSanity` | `safety-net-sanity` | Runs mock-driven safety-net behavioral suites across core, recognizers, CLI, and audit. |
| `TokenbridgeNoRawIndex` | `tokenbridge-no-raw-index` | Pins the TokenBridge library invariant for emitted spans: synthetic fixture PII cannot appear raw or as current-session tokens in stored or searched snippets. Its fixture supplies an email detector; CLI core detector coverage belongs to `index_ingest_tokenizes_core_identifiers_so_search_never_shows_them_raw` in `crates/gaze-cli/tests/index_cli.rs`. |
| `TokenbridgeEncryptedIndex` | `tokenbridge-encrypted-index` | Runs the TokenBridge persistent-index behavioral test that saves a synthetic owner-side index, asserts the file is AEAD-sealed, and fails if raw PII or projection key material appears on disk. |

The implementation lives in [`src/main.rs`](src/main.rs). The broader gate
catalog and gate-authoring rules are in
[docs/explanation/contributing/xtask-gates.md](../../docs/explanation/contributing/xtask-gates.md).

## English/German negative corpus

`generate-negative-corpus` produces the committed negative-only JSONL benchmark
fixture at
[`fixtures/negative_corpus/en_de_negative.jsonl`](fixtures/negative_corpus/en_de_negative.jsonl).
The canonical corpus uses seed `0`. Regenerate it with:

```console
$ cargo run -p xtask -- generate-negative-corpus --seed 0
```

Verification regenerates the full corpus in memory, compares it byte for byte,
and never writes:

```console
$ cargo run -p xtask -- generate-negative-corpus --verify --seed 0
```

Each UTF-8 JSONL record has the fixed fields `id`, `generator_id`, `seed`,
`language`, `category`, `license_origin`, `text`, and `oracle_spans`. This is a
negative-only corpus, so every `oracle_spans` value is `[]`; any protection
emitted by a false-positive scorer for a document is therefore a false
positive. The full annotation and stability contract is documented in
[`negative-corpus-annotation-contract.md`](../../docs/reference/benchmarks/negative-corpus-annotation-contract.md).

## CI integration

CI calls the same commands. Behavioral gates fail when a protected test is missing, renamed, fails, or cannot start.

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
