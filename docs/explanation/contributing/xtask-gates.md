# Xtask repository gates

`crates/xtask` is the unpublished repository gate runner. Run
`cargo run -p xtask -- <gate>` from the workspace root. Commands live in
[`main.rs`](../../../crates/xtask/src/main.rs); the canonical active roster is
in [`CLAUDE.md`](../../../CLAUDE.md).

## Run the gates

### CI coverage ownership

Unqualified `ci-feature-matrix` runs the full local matrix. CI splits ownership:

| Owner | Work |
| --- | --- |
| `test` | Formatting, all-feature Clippy, document/CLI MCP tests, all-feature workspace suite |
| `xtask feature gates` | `ci-feature-matrix --partition ci-gates`: remaining features and gates |
| `workspace default features` | `ci-feature-matrix --partition ci-default`: default-feature suite with sanitized build environment |

Required `xtask gates` waits for all three. Failure, skip, or cancellation fails
it. Required `test` and `Check DCO sign-off` names are unchanged.

## Current gates

| Gate | Command | Current behavior |
|------|---------|------------------|
| `SymmetricPotemkin` | `cargo run -p xtask -- symmetric-potemkin` | Runs tests in `SYMMETRIC_POTEMKIN_TESTS`. Missing/failing tests fail the gate. |
| `ClassMapOverrideSafety` | `cargo run -p xtask -- class-map-override-safety` | Lists and runs `t20_context_class_map_overrides_policy_dict_class` and `t20a_class_map_override_fails_closed_when_action_rule_uncovered`. |
| `RecognizerCompositionValidator` | `cargo run -p xtask -- recognizer-composition-validator` | Runs tests in `RECOGNIZER_COMPOSITION_VALIDATOR_TESTS`. The gate fails if the rulepack composition validator tests are missing or failing. |
| `NoTenantKnowledge` | `cargo run -p xtask -- no-tenant-knowledge` | Rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Allow markers (`// allow(tenant-fixture)`) hard-fail in production scope and remain valid only in tests, benches, docs, and `CONTRIBUTING.md`. |
| `BundleTokenizationDrift` | `cargo run -p xtask -- bundle-tokenization-drift` | Discovers recognizer-bearing bundled rulepacks from `crates/gaze-recognizers/embedded/*.toml`, runs the real `gaze clean --rulepack-bundled <bundle> --audit-db <tmp>` path against `crates/xtask/fixtures/drift-corpus.txt`, restores emitted tokens to infer byte spans, and compares canonical no-policy tokenization metadata to `crates/xtask/snapshots/<bundle>-no-policy.json`. Snapshots exclude raw values, `session_blob`, and audit `created_at`. `--verify-ack` fails closed when snapshot changes lack both a `// drift-ack:` source/test comment and a `[bundle-tokenization-drift]` line in the `[Unreleased]` `### Changed` section of `CHANGELOG.md`. |
| `FamilyPolicyTableCoherence` | `cargo run -p xtask -- family-policy-table-coherence` | Parses embedded rulepacks and checks collision-family declarations compile into the expected family precedence table, including IBAN-over-PAN and same-variant phone non-arbitration. |
| `LocaleCueBundleCoherence` | `cargo run -p xtask -- locale-cue-bundle-coherence` | Checks every mandatory-anchor declaration in core bundles has a matching cue key in embedded locale bundles. |
| `FixtureCitationLint` | `cargo run -p xtask -- fixture-citation-lint` | Production-code lint scanner for fixture-shaped PII literals in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Each production fixture literal must carry `// fixture-cited(<test-path>:<fully-qualified-test-name>)`, and the fully qualified test name must appear exactly in `cargo test --workspace -- --list`. |
| `CiFeatureMatrix` | `cargo run -p xtask -- ci-feature-matrix` | Runs the CI feature matrix, including the no-phone-parser fail-closed configuration. |
| `CargoMetadataAuditIsolation` | `cargo run -p xtask -- cargo-metadata-audit-isolation` | Parses `cargo metadata --format-version=1` and fails if any non-audit-responsible workspace package has a normal dependency path to `gaze-audit` in default, `--no-default-features`, or safety-net graphs. The explicit audit-responsible allowlist is documented in source; currently `gaze-cli` is allowed because its audit command consumes the passive sink directly. |
| `ReadmeVersionCheck` | `cargo run -p xtask -- readme-version-check` | Parses effective workspace package versions via `cargo metadata`, verifies fixture pass/fail tests, then rejects stale crate README install pins before publish. |
| `DylintGate` | `cargo run -p xtask -- dylint-gate` | Verifies the `lint/dylint/ui` fixture corpus has exactly 18 enabled fixtures, rejects `*_disabled.rs`, and runs `cargo dylint --workspace --all` when `cargo-dylint` is installed. The lint is `GAZE_MODULE_ISOLATION`, the canonical rustc-resolver-based gate for audit-sink protected-path isolation. |
| `SafetyNetSanity` | `cargo run -p xtask -- safety-net-sanity` | Behavioral gate over the OpenAI-filter SafetyNet path: lists and runs the `mock_safety_net`, `openai_filter_subprocess`, and `context_sensitivity_v0_6` recognizer-suite tests. |
| `TokenbridgeNoRawIndex` | `cargo run -p xtask -- tokenbridge-no-raw-index` | Pins the TokenBridge library invariant for emitted spans. It builds its own email detector and checks that synthetic fixture PII cannot appear raw or as current-session tokens in stored or searched snippets. CLI detector coverage belongs to `index_ingest_tokenizes_core_identifiers_so_search_never_shows_them_raw` in `crates/gaze-cli/tests/index_cli.rs`. |
| `TokenbridgeEncryptedIndex` | `cargo run -p xtask -- tokenbridge-encrypted-index` | Runs the TokenBridge persistent-index behavioral test that saves a synthetic owner-side index, asserts the file is AEAD-sealed, and fails if raw PII or projection key material appears on disk. |

## Recursive-Potemkin discipline

A gate must execute a behavioral test that fails when its contract breaks.
Symbol/file/string presence alone is insufficient. Add `BehavioralTest` entries
(package, optional integration target, exact name), call `ensure_test_exists`,
then `run_behavioral_test`. Missing/renamed tests fail listing; broken behavior
fails execution.

## Dylint gate

`GAZE_MODULE_ISOLATION` is the rustc-resolver audit-sink isolation lint.
The wrapper skips locally with a message when `cargo-dylint` is absent. CI runs:

```console
$ cd lint/dylint && cargo test --test ui
$ cd ../..
$ cargo run -p xtask -- dylint-gate
```

All 18 UI fixtures must stay enabled; `*_disabled.rs` fails. Coverage includes
macro call-site hygiene, `#[path]`, `include!()`, type positions, trait bounds,
and clean controls. The retired `audit-metadata-only` syn walker is not the
source of truth. See [`lint/dylint`](../../../lint/dylint) for public source/pins.

## Rehearse a gate failure

Use a throwaway branch/worktree. Confirm failure, then revert before commit.

### cargo-metadata-audit-isolation

Add `gaze-audit = { workspace = true }` to `crates/gaze/Cargo.toml` normal
`[dependencies]`. Run `cargo run -p xtask -- cargo-metadata-audit-isolation`;
it must fail and name `gaze`'s path to `gaze-audit`. Revert. Development edges
are ignored so compatibility tests can use the sink.

### bundle-tokenization-drift

Run `cargo run -p xtask -- bundle-tokenization-drift` clean, then rename
`ip.v4` to `ip.v4.drift` in `core-extended.toml`. Rerun: it must fail and name
the bundle, old/new ID, `custom:ip_address`, and changed count. Revert.

For intentional snapshot changes, run `--regenerate-baseline`, add a nearby
`// drift-ack:` source/test comment and a `[bundle-tokenization-drift]` line
naming affected bundles under `[Unreleased]` → `### Changed` in `CHANGELOG.md`.
Then run `cargo run -p xtask -- bundle-tokenization-drift --verify-ack`.

### tokenbridge-no-raw-index

```console
$ cargo run -p xtask -- tokenbridge-no-raw-index
$ cargo run -p xtask -- tokenbridge-no-raw-index --inject-adversarial-raw-snippet
```

The second must fail, naming the synthetic value and stored/search leak surface.
The first exercises real ingest, store, and search.

### tokenbridge-encrypted-index

`cargo run -p xtask -- tokenbridge-encrypted-index` lists/runs
`persistent::tests::saved_index_file_is_aead_sealed_and_omits_plaintext`.
It writes an owner index with `GAZE_INDEX_KEY`, reads `index.json`, and requires
AEAD magic without raw fixture values or projection key material.

### fixture-citation-lint and its limitation

This gate scans the same production roots as `no_tenant_knowledge`, excluding
`#[cfg(test)]` regions. Add a synthetic email assembled from `alice`, `@`, and
`example.invalid` with a marker such as
`// fixture-cited(crates/gaze/tests/email.rs:gaze::tests::email_round_trip)`.
Run `cargo run -p xtask -- fixture-citation-lint`: the exact cited test must be
in `cargo test --workspace -- --list`. Rename an existing cited test and rerun;
expect `FixtureCitationMissingTest`. Revert all edits.

The gate proves test existence, not that the test still asserts that literal.
Reviewers must check the latter.

## Adding a gate

1. Add a `Command` enum variant and dispatch arm in `main.rs`.
2. Define a `BehavioralTest` list with package, optional target, and exact names.
3. In the runner, call `ensure_test_exists` for all entries, then
   `run_behavioral_test` for all entries.
4. Run `cargo run -p xtask -- new-behavior-gate`.
5. Temporarily rename a protected test; confirm nonzero failure during listing,
   then revert before opening the PR.
