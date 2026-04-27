# Contributing

## Workspace shape

As of v0.5 dev complete (`[Unreleased]`):

| Crate | Role |
|---|---|
| `crates/gaze` | Core: pipeline, session, policy loader, recognizer registry, locale chain, rulepack schema, token grammar, `RedactionLogger` trait. **No `rusqlite` dep in default or `--no-default-features` builds.** |
| `crates/gaze-types` | Shared value contracts (`Recognizer`, `Detection`, `PiiClass`, `Action`, `RedactionEntry`, `LocaleTag` / `LocaleChain` / `LocaleError`, `RawDocument`, `CleanDocument`, `DictionaryBundle`, token-related types). Serde-only — no ML or sql deps. New in v0.5 Phase B (PR #74). |
| `crates/gaze-recognizers` | Regex/dictionary/NER detection backends + embedded `core` and `core-extended` rulepacks + locale bundles. |
| `crates/gaze-audit` | Passive audit sink: `SqliteLogger`, `AuditFilter`, `AuditLogRow`, `build_audit_query_sql`, `AUDIT_RESTRICTED_COLUMNS`. `rusqlite` is isolated here. New in v0.5 Phase C (PR #75). |
| `crates/gaze-assembly` | Policy-to-pipeline assembly shared by CLI-style adopters. |
| `crates/gaze-cli` | Standalone `gaze` binary; the only allowlisted `gaze-audit` consumer outside compatibility tests. |
| `crates/xtask` | Internal repository gate runner: `bundle-tokenization-drift`, `fixture-citation-lint`, `ci-feature-matrix`, `class-map-override-safety`, `symmetric-potemkin`, `no-tenant-knowledge`, `cargo-metadata-audit-isolation` (Phase C), `dylint-gate` (Phase D). |
| `xtask/dylint/` | Dylint lint crate hosting `gaze_module_isolation`. Detached workspace pinned to `nightly-2025-09-18`. New in v0.5 Phase D. |

## Tenant class names in tests

Test fixtures and benchmark labels MUST use neutral class names (e.g. `class_alpha`, `tenant_class_a`, `dict_alpha`), never tenant-specific patterns like `order_id`, `Order_42`, `Song_42`, `User_7`. Rationale: drawer `eac549ae` — gaze core has no built-in tenant knowledge.

The `cargo run -p xtask -- no-tenant-knowledge` gate scans production Rust code in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/**/*.rs` and fails on those tenant-specific patterns. It intentionally does not scan `tests/`, `benches/`, docs, `CONTRIBUTING.md`, or `crates/xtask/`.

Use `// allow(tenant-fixture)` only in tests, benches, or docs when a tenant-like fixture is necessary to exercise behavior. That marker is a production-bypass attempt in `crates/*/src/` and hard-fails the gate with `AllowMarkerInProductionScope`.

The `order_id` denylist is intentionally broad — it catches `Order_42`, `order_ids`, etc. If a legitimate production identifier (e.g. `order_history_index_id` for an unrelated subsystem) collides with the denylist post-v0.4.3, coordinate with maintainers to add to allowlist with rationale comment. Do NOT silently bypass via `// allow(tenant-fixture)` in production code — that marker hard-fails the gate (drawer `eac549ae`).

Round-trip, three-surfaces, and recognizer-composition cross-cutting rows are N/A for this structural gate: it emits no tokens, adds no runtime knobs, and does not compose recognizers. The no-tenant-knowledge row is enforced by CI so production code must pass post-merge.

## Fixture citations in production code

Production Rust code in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/`
MUST NOT introduce hardcoded fixture-shaped PII literals unless the line or
immediately preceding line cites the behavioral test that owns the fixture:

```rust
// fixture-cited(crates/gaze/tests/email.rs:gaze::tests::email_round_trip)
const FIXTURE_EMAIL: &str = "alice@example.invalid";
```

The `cargo run -p xtask -- fixture-citation-lint` gate verifies two things:
the production literal has a `fixture-cited(...)` marker, and
`cargo test --workspace -- --list` contains the cited fully qualified test name
exactly. Suffix-only matches and path-only markers do not pass.

Known limitation: this gate proves the cited test exists, not that the test body
still asserts that exact fixture literal. Reviewers must still check that the
citation points at a meaningful behavioral assertion.

## Phone-number fixtures

Test and benchmark fixtures that contain phone numbers MUST use synthetic, non-reachable values from documented reservation ranges:

- US/NA fixtures: NANPA "555" exchanges (`+1-555-01xx` etc.), reserved for fictional use under [NANPA reservation 555-01xx](https://nationalnanpa.com/).
- UK fixtures: Ofcom drama-reserved ranges (`+44-7700-900xxx`), per [Ofcom drama numbers guidance](https://www.ofcom.org.uk/phones-and-broadband/phone-numbers/numbers-for-drama).
- DE fixtures: synthetic non-reachable mobile shapes the `phonenumber` parser still accepts as valid E.164 (e.g. `+49 1555 0112233`-style values used in v0.4.4 S3a / v0.4.5 S2 phone-recognizer tests). The `1555` mobile prefix mirrors the NANPA 555 carve-out — non-reachable but parseable. Cite the v0.4.5 S2 phonenumber-region tests rather than introducing real-looking BNetzA-assigned ranges.
- Other locales: synthesize a non-reachable shape (e.g. exchange code `0` or out-of-band country code) and add a fixture comment noting the synthetic origin.

Rationale: drawer `gaze_decisions_e1ab6dc0`. Real reachable numbers in test
fixtures risk inadvertent leakage into adopter telemetry, public CI logs, and
crate metadata. The `phonenumber` parser-backed `E164Phone` validator
(v0.4.4 S3a) accepts the NANPA 555 reservation and Ofcom drama ranges as valid
E.164, so positive-path tests continue to exercise the validator without using
real numbers. v0.4.5 S2 (PR #58) adds parser-backed national phone recognizers
for DE and US that follow the same synthetic-only fixture posture.

## PR-checks ritual

Before opening or pushing to a PR, run the workspace test suite plus all
behavioral xtask gates:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- symmetric-potemkin
cargo run -p xtask -- class-map-override-safety
cargo run -p xtask -- recognizer-composition-validator
cargo run -p xtask -- no-tenant-knowledge
cargo run -p xtask -- bundle-tokenization-drift
cargo run -p xtask -- fixture-citation-lint
cargo run -p xtask -- ci-feature-matrix
cargo run -p xtask -- cargo-metadata-audit-isolation
cargo run -p xtask -- dylint-gate
```

The `--all-features` flag on `cargo test` enables `gaze`'s `audit` feature shim
(v0.5 Phase C compatibility window) so dev-dep tests that reference the
re-exported `gaze::SqliteLogger` continue to compile.

The `cargo-metadata-audit-isolation` gate (v0.5 Phase C) parses
`cargo metadata` and fails closed if any non-audit-responsible workspace
member has a normal-dependency path to `gaze-audit`.

The `dylint-gate` (v0.5 Phase D) is the canonical audit-sink protected-path
enforcer. It supersedes the legacy `audit-metadata-only` syn walker, which was
decommissioned in v0.5 Phase E (PR #77, commit `f4fde12`). Toolchain pins,
fixture matrix, and timings live in
[`docs/research/v0.5-dylint-audit-gate.md`](docs/research/v0.5-dylint-audit-gate.md).
`cargo-dylint` is a CI-only requirement; the local wrapper skips with a clear
message when `cargo-dylint` is absent.

CI runs the equivalents of these gates on every PR. Running them locally
before pushing prevents the "I forgot to run xtask" round-trip.
