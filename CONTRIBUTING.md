# Contributing

Follow the licence, DCO and Code of Conduct below, then run the PR checks.

## Before you open a PR

### Licence

Contributions use [Apache-2.0](LICENSE-APACHE) OR [MIT](LICENSE-MIT), at the
user's option. No CLA is required. Contributors keep copyright; later
maintainers need contributor agreement to relicense.
[Governance](docs/explanation/governance.md).

### Developer Certificate of Origin (DCO)

Every commit needs a `Signed-off-by:` trailer matching its author and certifying
the [DCO](https://developercertificate.org/), plus a cryptographic signature:

```bash
git commit -S -s
```

`.github/workflows/dco.yml` checks author-matching trailers on every non-merge
PR commit. It does not apply retroactively to pre-gate history. Agents follow
[commit discipline](AGENTS.md#universal-rules-all-agents), including no amend
or force-push.

### Code of Conduct

Community interactions are governed by the [Contributor Covenant](CODE_OF_CONDUCT.md).
Reporting channels live in that file.

## Setup

Clone the repo and run the toolchain check:

```bash
cargo build --workspace --all-features
```

PR-triggered CI runs `cargo doc -D warnings`, `cargo test --doc`, workspace
tests, MSRV checks, cargo-deny, and the active xtask gate roster on every
relevant PR. Keep running the local [PR checks](#run-the-pr-checks) before opening or
pushing to a PR.

## Workspace shape

The workspace has 15 published crates plus the internal `xtask` crate and the detached `lint/dylint/` workspace. [docs/reference/crates.md](docs/reference/crates.md) has the full dependency map.

| Crate | Role |
|---|---|
| `crates/gaze` | Core: pipeline, session, policy loader, recognizer registry, locale chain, rulepack schema, token grammar. Re-exports `gaze_types::RedactionLogger` for source-compat. No `rusqlite` dep in any feature graph. |
| `crates/gaze-types` | Shared value contracts (`Recognizer`, `Detection`, `PiiClass`, `Action`, `RedactionEntry`, `LocaleTag` / `LocaleChain` / `LocaleError`, `RawDocument`, `CleanDocument`, `DictionaryBundle`, token-related types). Serde-only — no ML or sql deps. |
| `crates/gaze-recognizers` | Regex/dictionary/NER detection backends + embedded `core` and `core-extended` rulepacks + locale bundles. |
| `crates/gaze-audit` | Passive audit sink: `SqliteLogger`, `AuditFilter`, `AuditLogRow`, `build_audit_query_sql`, `AUDIT_RESTRICTED_COLUMNS`. `rusqlite` is isolated here. |
| `crates/gaze-assembly` | Policy-to-pipeline assembly shared by CLI-style adopters. |
| `crates/gaze-cli` | Standalone `gaze` binary; the only allowlisted `gaze-audit` consumer outside compatibility tests. |
| `crates/gaze-mcp-core` | Transport-free MCP-shaped chokepoint runtime: `Tool` trait, sealed `ToolCtx`, `ToolRegistry`, `PiiEnvelope::dispatch`, `Frontend`/`DispatchHost`, `ManifestStore`, `AuthHook`, `SessionIdPolicy`. |
| `crates/gaze-mcp-rmcp` | rmcp transport sink: `RmcpFrontend`, stdio default transport, opt-in streamable HTTP transport, adopter-supplied `PrincipalResolver`. |
| `crates/gaze-document` | OSS document ingestion: PNG/JPG/PDF → Tesseract OCR → gaze redact → `SafeBundle` (`clean.md`, `manifest.json`, `report.json`). Ships a `gaze document clean` CLI verb under the `gaze-cli` `document` feature. `BundleReport` has a versioned `bundle_version` schema. |
| `crates/gaze-proxy` | Feature-gated HTTP proxy runtime for OpenAI, Anthropic, and Gemini SDK base-URL swaps; backs `gaze proxy`. |
| `crates/gaze-inspection` | Provider-neutral, bounded inspection delivery: zeroizing payload wrappers and the matched producer/consumer runtime. |
| `crates/gaze-model-setup` | Installs and verifies pinned model bundles for `gaze setup`. |
| `crates/gaze-mcp-bridge` | Optional policy-gated MCP bridge in front of downstream MCP tools. |
| `crates/gaze-token-bridge` | Experimental owner-side authorization and translation bridge for searching redact-before-index corpora. |
| `crates/gaze-proxy-dashboard` | Opt-in, memory-only inspection dashboard runtime for `gaze proxy`: a killable child process owns listener/auth/store/rendering while the parent owns bounded ingress and the registration-bound activation. Among Gaze crates it depends on exactly `gaze-types` + `gaze-inspection`; shipped behind the default-off `gaze-cli` `dashboard` feature and enforced by the `dashboard-isolation` xtask gate. |
| `crates/xtask` | Internal repository gate runner: `bundle-tokenization-drift`, `fixture-citation-lint`, `trybuild-fixture-hygiene`, `ci-feature-matrix`, `class-map-override-safety`, `symmetric-potemkin`, `no-tenant-knowledge`, `cargo-metadata-audit-isolation` (Phase C), `dylint-gate` (Phase D), `dashboard-isolation`. |
| `lint/dylint/` | Dylint lint crate hosting `gaze_module_isolation`. Detached workspace pinned to `nightly-2025-09-18`. |

## Run the PR checks

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

The live GLiNER tests are explicitly ignored in ordinary CI because CI does
not install the 333 MB model. After `gaze setup --dob-judge`, run them with the
pinned local bundle:

```bash
export GAZE_GLINER_DOB_TEST_BUNDLE="${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/gliner-multi-pii-dob-int8"
cargo test -p gaze-recognizers --lib dob_judge::tests::live_model_separates_birth_dates_from_business_dates_without_keyword_filter -- --ignored --exact
cargo test --release -p gaze-recognizers --lib dob_judge::tests::live_forty_row_table_reuses_tiled_windows -- --ignored --exact --nocapture
cargo test -p gaze-recognizers --test dob_judge_live local_bundle_classifies_synthetic_dates -- --ignored --exact
```

`--all-features` exercises every workspace feature. Import concrete audit sinks
from `gaze-audit`; the old core `audit` shim is removed.
`cargo-metadata-audit-isolation` parses Cargo metadata and fails closed on a
normal-dependency path to `gaze-audit` from a non-audit-responsible member.

There is no tracked pre-push hook. Run these gates manually before opening or
pushing a PR; CI also runs workspace, MSRV, cargo-deny and active xtask gates.

The required `test` check aggregates three owners: `test-support` (the existing
feature-specific suites, all-feature doctests, pinned live models, and fresh
producer binding), `workspace-shards` (the all-feature lib/bin/integration suite),
and `test-coverage` (once-only execution proof). A failed, skipped, cancelled, or
missing owner fails the required check. The existing `xtask gates` aggregator
still requires `test`, the default-feature workspace suite, and feature gates.

Workspace shards use pinned cargo-nextest with no retries. Every test binary,
including empty platform/feature-gated binaries, has one explicit assignment in
`scripts/ci/test-shards.json`. Adding a binary requires updating that roster;
`python3 -m unittest discover -s scripts/ci` checks it against Cargo metadata.
Each CI shard independently compares the original Cargo/libtest binary and test
lists with nextest, including ignored flags. The proof job checks each selected
test against JUnit results and publishes all timings plus the top 20. Ignored
child helpers remain ignored; parent tests can still execute them directly.

`.config/nextest.toml` carries process-level scheduling locks for nested builds
and loopback probes whose in-process mutexes cannot coordinate nextest processes.
Shards use separate runners and fresh target trees, including mutated checkouts;
only registry/git/native dependencies are shared in one cache with one writer.
The fresh producer proof continues to build offline in its own scratch checkout.
To reproduce one shard after installing cargo-nextest 0.9.146:

```bash
export RUSTC="$(rustup which --toolchain 1.96.0 rustc)"
export RUSTDOC="$(rustup which --toolchain 1.96.0 rustdoc)"
python3 scripts/ci/test_shards.py collect --output target/shard-evidence/cli
cargo nextest run --workspace --all-features --profile ci --no-fail-fast \
  -E "$(python3 scripts/ci/test_shards.py filter cli)"
cargo test --workspace --all-features --doc --no-fail-fast
```

The unqualified local `cargo test` and `ci-feature-matrix` commands remain the
complete local verification path. CI timing artifacts expire after 14 days.

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

### Dylint audit-sink gate

`dylint-gate` is the canonical audit-sink enforcer; the old
`audit-metadata-only` syn walker is removed. [lint/dylint](lint/dylint) is the
public source. CI runs the compiled lint and UI fixtures on every PR to `main`,
Mondays at 08:00 UTC and manual dispatch. `GAZE_DYLINT_REQUIRED=1` makes
missing tooling fail CI. Locally, report a deferral if `cargo-dylint` is unavailable.

To run the compiled gate locally when touching audit-sink boundaries, install
the same pinned tools as CI:

```bash
rustup toolchain install nightly-2025-09-18 --component rustc-dev,rust-src,llvm-tools-preview
cargo install --locked --version 6.0.4 cargo-dylint dylint-link
GAZE_DYLINT_REQUIRED=1 cargo run -p xtask --locked -- dylint-gate
```

The workflow can also be triggered manually:

```bash
gh workflow run dylint.yml
```

## Fixture rules

### Tenant class names in tests

Use neutral test/benchmark classes (`class_alpha`, `tenant_class_a`,
`dict_alpha`), never tenant-specific `order_id`, `Order_42`, `Song_42` or
`User_7`. Core has no tenant knowledge.

`cargo run -p xtask -- no-tenant-knowledge` scans production
`crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/**/*.rs`.
It excludes tests, benches, docs, this file and `crates/xtask/`.

`// allow(tenant-fixture)` is allowed only in tests, benches and docs when
needed for behavior. In production `crates/*/src/` it fails with
`AllowMarkerInProductionScope`. The broad `order_id` denylist also catches
`order_ids`; ask maintainers for a documented allowlist entry for a legitimate
collision such as `order_history_index_id`.

This CI structural gate emits no tokens, adds no runtime knobs and composes no
recognizers, so round-trip, three-surface and composition rows are N/A.

### Fixture citations in production code

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

### Phone-number fixtures

Test and benchmark fixtures that contain phone numbers MUST use synthetic, non-reachable values from documented reservation ranges:

- US/NA fixtures: only line numbers `555-0100` to `555-0199` behind any area code (for example `+1 212 555 0142`). NANPA reserves exactly these as fictitious numbers for entertainment and advertising; other `555` line numbers can be assigned. Source: [NANPA 555 line numbers](https://www.nanpa.com/numbering/555-line-numbers).
- UK fixtures: Ofcom drama-reserved ranges (`+44-7700-900xxx`), per [Ofcom drama numbers guidance](https://www.ofcom.org.uk/phones-and-broadband/phone-numbers/numbers-for-drama).
- DE fixtures: the Bundesnetzagentur drama numbers ("Rufnummern für Medienproduktionen", Mitteilung 148/2021), which are never assigned to a subscriber. Mobile: `(0)171 39200 00` to `99` and `(0)176 040690 00` to `99` (100 numbers each), plus ten single numbers. Landline: 1,000 numbers each in Berlin `(0)30 23125 xxx`, Frankfurt `(0)69 90009 xxx`, Hamburg `(0)40 66969 xxx`, Köln `(0)221 4710 xxx` and München `(0)89 99998 xxx`. The `phonenumber` parser accepts them as valid E.164. Source: [BNetzA Mitteilung 148/2021](https://www.bundesnetzagentur.de/DE/Fachthemen/Telekommunikation/Nummerierung/_DL/mittlg148_2021.pdf?__blob=publicationFile&v=1). Do not use `+49 1555`: `(0)15-550` is an allocated mobile block, not a fictional one. Older fixtures that still use it are tracked for replacement.
- Other locales: synthesize a non-reachable shape (e.g. exchange code `0` or out-of-band country code) and add a fixture comment noting the synthetic origin.

Reserved fixtures avoid leaking reachable numbers into telemetry, CI logs or
crate metadata. `E164Phone` accepts NANPA and Ofcom reservations, so positive
tests still exercise validation. DE/US national-phone tests use fictional
numbers too.
