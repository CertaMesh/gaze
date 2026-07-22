# Contributing

Thanks for considering a contribution to Gaze. The rest of this document is
the technical workflow (gates, fixtures, test rituals). Before you open a PR,
two project-wide rules apply:

## Licence

By submitting a contribution to this project you agree to licence it under
**Apache-2.0 OR MIT** at the user's option — the same dual permissive licence
the project ships under. Both licence files live at the repo root:
[`LICENSE-APACHE`](LICENSE-APACHE) and [`LICENSE-MIT`](LICENSE-MIT).

We do **not** require a Contributor Licence Agreement (CLA). Contributors
retain their copyright; the project cannot be silently re-licensed by any
later maintainer without contributor agreement. See [`docs/explanation/governance.md`](docs/explanation/governance.md)
for the full governance model and the structural commitments that keep the
project a commons.

## Developer Certificate of Origin (DCO)

Every commit MUST carry a `Signed-off-by:` trailer matching the commit author,
certifying the [Developer Certificate of Origin](https://developercertificate.org/).
Add it automatically:

```bash
git commit -s
```

The `.github/workflows/dco.yml` check enforces this on every pull request: each
non-merge commit must contain a `Signed-off-by: Name <email>` line matching its
author identity. Fix existing commits with `git rebase --signoff <base>` (or
`git commit --amend -s` for the latest). Enforcement is forward-looking on PRs;
commits predating this gate are not retroactively signed.

## Code of Conduct

Community interactions are governed by the [Contributor Covenant](CODE_OF_CONDUCT.md).
Reporting channels live in that file.

---

## Setup

Clone the repo and run the toolchain check:

```bash
cargo build --workspace --all-features
```

PR-triggered CI runs `cargo doc -D warnings`, `cargo test --doc`, workspace
tests, MSRV checks, cargo-deny, and the active xtask gate roster on every
relevant PR. Keep running the local "PR-checks ritual" below before opening or
pushing to a PR.

## Workspace shape

As of v0.7.2, the workspace has **nine** published-shape crates plus `xtask`. The ninth crate is `gaze-document` (added in v0.7.1).

| Crate | Role |
|---|---|
| `crates/gaze` | Core: pipeline, session, policy loader, recognizer registry, locale chain, rulepack schema, token grammar. Re-exports `gaze_types::RedactionLogger` for source-compat. **No `rusqlite` dep in any feature graph.** |
| `crates/gaze-types` | Shared value contracts (`Recognizer`, `Detection`, `PiiClass`, `Action`, `RedactionEntry`, `LocaleTag` / `LocaleChain` / `LocaleError`, `RawDocument`, `CleanDocument`, `DictionaryBundle`, token-related types). Serde-only — no ML or sql deps. New in v0.5 Phase B (PR #74). |
| `crates/gaze-recognizers` | Regex/dictionary/NER detection backends + embedded `core` and `core-extended` rulepacks + locale bundles. |
| `crates/gaze-audit` | Passive audit sink: `SqliteLogger`, `AuditFilter`, `AuditLogRow`, `build_audit_query_sql`, `AUDIT_RESTRICTED_COLUMNS`. `rusqlite` is isolated here. New in v0.5 Phase C (PR #75). |
| `crates/gaze-assembly` | Policy-to-pipeline assembly shared by CLI-style adopters. |
| `crates/gaze-cli` | Standalone `gaze` binary; the only allowlisted `gaze-audit` consumer outside compatibility tests. |
| `crates/gaze-mcp-core` | Transport-free MCP-shaped chokepoint runtime: `Tool` trait, sealed `ToolCtx`, `ToolRegistry`, `PiiEnvelope::dispatch`, `Frontend`/`DispatchHost`, `ManifestStore`, `AuthHook`, `SessionIdPolicy`. New in v0.7.0. |
| `crates/gaze-mcp-rmcp` | rmcp transport sink: `RmcpFrontend`, stdio default transport, opt-in streamable HTTP transport, adopter-supplied `PrincipalResolver`. New in v0.7.0. |
| `crates/gaze-document` | OSS document ingestion: PNG/JPG/PDF → Tesseract OCR → gaze redact → `SafeBundle` (`clean.md`, `manifest.json`, `report.json`). Ships a `gaze document clean` CLI verb under the `gaze-cli` `document` feature. `BundleReport` schema versioned via `bundle_version = 1`. New in v0.7.1. |
| `crates/gaze-proxy-dashboard` | Opt-in, memory-only inspection dashboard runtime for `gaze proxy`: a killable child process owns listener/auth/store/rendering while the parent owns bounded ingress and the registration-bound activation. Among Gaze crates it depends on exactly `gaze-types` + `gaze-inspection`; shipped behind the default-off `gaze-cli` `dashboard` feature and enforced by the `dashboard-isolation` xtask gate. |
| `crates/xtask` | Internal repository gate runner: `bundle-tokenization-drift`, `fixture-citation-lint`, `trybuild-fixture-hygiene`, `ci-feature-matrix`, `class-map-override-safety`, `symmetric-potemkin`, `no-tenant-knowledge`, `cargo-metadata-audit-isolation` (Phase C), `dylint-gate` (Phase D), `dashboard-isolation`. |
| `lint/dylint/` | Dylint lint crate hosting `gaze_module_isolation`. Detached workspace pinned to `nightly-2025-09-18`. New in v0.5 Phase D. |

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

## Local gate matrix

Gaze does not ship a tracked pre-push hook — gates still run manually before
opening or pushing to a PR. The "PR-checks ritual" below lists the local set.
Relevant PRs also run the workspace, MSRV, cargo-deny, and active xtask gates in
GitHub Actions.

`dylint` requires the pinned `nightly-2025-09-18` toolchain and cargo-dylint
setup. It runs weekly on Monday at 08:00 UTC via the scheduled workflow and
can be triggered manually:

```bash
gh workflow run dylint.yml
```

## PR-checks ritual

Before opening or pushing to a PR, run the workspace test suite plus all
behavioral xtask gates:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo deny check
cargo run -p xtask -- symmetric-potemkin
cargo run -p xtask -- class-map-override-safety
cargo run -p xtask -- recognizer-composition-validator
cargo run -p xtask -- no-tenant-knowledge
cargo run -p xtask -- bundle-tokenization-drift
cargo run -p xtask -- family-policy-table-coherence
cargo run -p xtask -- locale-cue-bundle-coherence
cargo run -p xtask -- fixture-citation-lint
cargo run -p xtask -- trybuild-fixture-hygiene
cargo run -p xtask -- cargo-metadata-audit-isolation
cargo run -p xtask -- readme-version-check
cargo run -p xtask -- safety-net-sanity
cargo run -p xtask -- ci-feature-matrix
```

The `--all-features` flag on `cargo test` exercises every current workspace
feature. The v0.5 `gaze` audit feature shim was removed in v0.6; compatibility
tests import concrete audit sinks from `gaze-audit` directly.

The `cargo-metadata-audit-isolation` gate (v0.5 Phase C) parses
`cargo metadata` and fails closed if any non-audit-responsible workspace
member has a normal-dependency path to `gaze-audit`.

### Trybuild compiler and blessing ritual

The three root trybuild drivers verify the compiler Cargo will actually invoke,
not only the Cargo or shell toolchain identity. When the workspace
`rust-toolchain.toml` is present, each driver honors `RUSTC` when set (otherwise
PATH `rustc`), reads `rustc --version --verbose`, and requires its `release:` to
match the pinned channel before any fixture runs. A mismatch is an execution
error, not a snapshot change; bind Cargo and both child compiler tools
explicitly:

```bash
GAZE_TOOLCHAIN=1.96.0
GAZE_CARGO="$(rustup which --toolchain "$GAZE_TOOLCHAIN" cargo)"
GAZE_RUSTC="$(rustup which --toolchain "$GAZE_TOOLCHAIN" rustc)"
GAZE_RUSTDOC="$(rustup which --toolchain "$GAZE_TOOLCHAIN" rustdoc)"
RUSTC="$GAZE_RUSTC" RUSTDOC="$GAZE_RUSTDOC" "$GAZE_CARGO" test --workspace --all-features --locked
```

Bless trybuild output only in a clean disposable checkout using those same
explicit bindings. Set `TRYBUILD=overwrite` for the smallest affected test
target, inspect every changed `.stderr`, then run the target normally and run:

```bash
RUSTC="$GAZE_RUSTC" RUSTDOC="$GAZE_RUSTDOC" TRYBUILD=overwrite "$GAZE_CARGO" test -p <package> --test <driver> --locked
RUSTC="$GAZE_RUSTC" RUSTDOC="$GAZE_RUSTDOC" "$GAZE_CARGO" test -p <package> --test <driver> --locked
RUSTC="$GAZE_RUSTC" RUSTDOC="$GAZE_RUSTDOC" "$GAZE_CARGO" run -p xtask --locked -- trybuild-fixture-hygiene
```

The hygiene gate fixes the root inventory at 19 expectations (13 inspection,
3 core, 3 MCP core), separately inventories the detached Dylint UI surface at
18 fixtures (16 fail, 2 pass), and rejects sysroot placeholders or raw
compiler/Homebrew/user paths. Do not add root `rust-src` or bless under a
sources-bundled compiler to work around the guard.

The `dylint-gate` (v0.5 Phase D) is the canonical audit-sink protected-path
enforcer. It supersedes the legacy `audit-metadata-only` syn walker, which was
decommissioned in v0.5 Phase E (PR #77, commit `f4fde12`). Toolchain pins,
fixture matrix, and timings live in
[`v0.5-dylint-audit-gate.md`](https://github.com/PIInuts/business/blob/main/research/v0.5-dylint-audit-gate.md)
(hosted in `PIInuts/business:research/`).
`cargo-dylint` is a scheduled-workflow requirement; the local gate ritual does
not include it.

Run `dylint` manually when touching audit-sink boundaries or wait for the
weekly scheduled workflow.
