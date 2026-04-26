# Gaze

**Reversible PII pseudonymization for agentic workflows.**

Gaze lets AI tools inspect logs, database samples, support messages, and production-adjacent data without exposing real personal data to the agent.

It does not merely redact. It replaces PII with stable, reversible tokens, so the data owner can safely restore the original values later — while the agent only ever sees pseudonyms.

**Clean in. Safe out. Restore when needed. No silent leaks.**

## What Gaze is

Gaze is a reversible PII pseudonymization runtime that lets AI agents work with production data without ever seeing the real personal data.

The workspace has seven crates:

- `crates/gaze-types` — shared value contracts for adopters and internal crates without pulling runtime or recognizer dependencies.
- `crates/gaze` — core library: pipeline, sessions, policy loader, recognizer registry, locale chain, rulepack schema, token grammar.
- `crates/gaze-audit` — passive SQLite audit sink and read-side audit query API, isolated from `gaze` core.
- `crates/gaze-recognizers` — detection backends plugged into the registry (regex, dictionary, NER) and bundled rulepacks.
- `crates/gaze-assembly` — policy-to-pipeline assembly shared by CLI-style adopters.
- `crates/gaze-cli` — the `gaze clean` / `gaze restore` binary adopters invoke from language adapters.
- `crates/xtask` — internal repository gate runner.

External consumer: [piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens), formerly the in-tree `debug-proxy` crate, provides the MCP debug server built on top of Gaze.

## Project north star

> **Gaze is the most reliable, reversible PII pseudonymization runtime for agentic workflows. Zero PII leaks between the agent and the data owner — ever. Any byte of PII that reaches an LLM outside the manifest contract is a critical defect.**

"Pseudonymization" is the GDPR Art. 4(5) term for reversible substitution with tokens — that reversibility is the Gaze moat, not one-way redaction. See [docs/research/gaze-first-principles-vision.md](docs/research/gaze-first-principles-vision.md#north-star-locked-2026-04-24) for the locked north star and rationale.

## Five Axes

Every design, implementation, and review decision is evaluated against these five axes. Full rationale: [docs/research/gaze-first-principles-vision.md](docs/research/gaze-first-principles-vision.md#north-star-locked-2026-04-24).

1. **Reliability (never leak).** Fail-closed always; defense in depth across regex, NER, dictionary, and optional neural safety net.
2. **Reversibility.** Manifest-first restore; no one-way primitives in the core contract.
3. **Agentic-first.** Prioritizes agent-workflow needs (tool-call JSON, streaming, multi-turn, tenant PII) over generic text handling.
4. **Trust (auditable + deterministic).** Rule-based detectors preferred; every token emission traceable to a rule or recognizer.
5. **Adopter ergonomics.** Low-friction framework adapters; adopter picks Gaze up in under a day without deep PII expertise.

## Install (v0.4.6)

v0.4.6 is the current stable release.

Apple Silicon macOS via release asset:

```bash
curl -L -o gaze https://github.com/piinuts/gaze/releases/download/v0.4.6/gaze-aarch64-apple-darwin
chmod +x gaze
mv gaze /usr/local/bin/gaze
```

Homebrew is repo-local for now. The formula source exists at `dist/homebrew/gaze.rb`, but no public `piinuts/tap` or `piinuts/homebrew-tap` formula is published yet, and this repository is private. Maintainers can smoke the formula by staging it into a scratch local tap; direct `brew install piinuts/tap/gaze` is not supported yet.

Public `brew install piinuts/tap/gaze` documentation should wait until a public tap exists and the release process publishes to it.

Linux x86_64 binary download from the release assets:

```bash
curl -L -o gaze https://github.com/piinuts/gaze/releases/download/v0.4.6/gaze-x86_64-unknown-linux-gnu
chmod +x gaze
mv gaze /usr/local/bin/gaze
```

The Linux x86_64 binary requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer). On older distros, build from source with `cargo build --release -p gaze-cli`.

Intel macOS binaries are not published; build from source with `cargo build --release -p gaze-cli`.

## Requirements

### Pre-built binaries

| Platform | Status | Notes |
|---|---|---|
| **macOS aarch64 (Apple Silicon)** | ✅ Supported | Download from [releases](https://github.com/piinuts/gaze/releases). Homebrew is repo-local at `dist/homebrew/gaze.rb`; no public tap is published yet. |
| **Linux x86_64 (glibc)** | ✅ Supported | **Requires glibc 2.39+** (Ubuntu 24.04, Debian 13, RHEL 10, or newer). The bundled ONNX Runtime needs C23 symbols (`__isoc23_strtoll` etc.) introduced in glibc 2.39. Older distributions: build from source. |
| **Linux aarch64 / musl** | ❌ Not shipped | Adopter-driven; [open an issue](https://github.com/piinuts/gaze/issues/new) if needed. |
| **macOS x86_64 (Intel)** | ❌ Not shipped | Apple Silicon focus. Build from source if needed. |
| **Windows** | ❌ Not shipped | Linux/WSL2 recommended. |

### Build from source

All platforms supported via `cargo`:

- **Rust:** 1.89+ (workspace MSRV pinned in crate `Cargo.toml` files)
- **C toolchain:** required for native dependencies (ort/ONNX Runtime, tokenizers)
- **Optional features:**
  - `phone-parser` (default-on for `gaze-recognizers` and therefore `gaze-cli`): pulls `phonenumber` crate for parser-backed E.164 phone validation. Disable with `--no-default-features` for raw recognizer-library use without phone deps.

```bash
git clone https://github.com/piinuts/gaze
cd gaze
cargo build --release -p gaze-cli
./target/release/gaze --version
```

### Library integration

```bash
cargo add gaze
cargo add gaze-recognizers --features phone-parser
```

### Runtime

- No external services required (all detection runs locally)
- Network access NOT used at runtime (audited per [docs/research/v0.4.4-phonenumber-audit.md](docs/research/v0.4.4-phonenumber-audit.md))
- SQLite for audit logs (optional, opt-in via `--audit-db <path>`)

## Workspace Layout

```text
crates/
  gaze-types/         shared value contracts with serde only
  gaze/               core library (pipeline, sessions, policy, registry, locale, rulepack)
  gaze-audit/         passive SQLite audit sink + read-side query API
  gaze-recognizers/   detection backends (regex, dictionary, NER) + bundled rulepacks
  gaze-assembly/      policy-to-pipeline assembly shared by CLI-style adopters
  gaze-cli/           standalone `gaze` binary for LLM pipe-mode integrations
  xtask/              internal repository gate runner
```

## Crate Guide

### `gaze`

Pure Rust library. Owns:

- pipeline + rule evaluation
- session-scoped tokenization (`<{session_hex}:Class_N>` grammar)
- signed sensitive snapshots with TTL enforcement
- `RecognizerRegistry` trait + `DetectContext` envelope (the recognizer-native detection path; the legacy standalone `Detector` path was removed in v0.4.0-rc.1)
- class/rule/score/length/id conflict resolver
- locale chain (CLI > policy > rulepack default > system default) with strict opaque-tag matching
- TOML rulepack schema loader (`[policy.rulepacks]`, `[[policy.custom_recognizers]]`)
- redaction logger + audit symmetry (`decided_by` + merge-loser entries)
- pluggable sandbox trait shape for future action-side work

### `gaze-audit`

Passive audit sink crate. Owns `SqliteLogger`, `AuditFilter`, `AuditLogRow`,
`build_audit_query_sql`, and `AUDIT_RESTRICTED_COLUMNS`. `gaze` does not depend
on this crate in default or `--no-default-features` builds; the temporary
`audit` feature re-exports these symbols for one minor migration window.

### `gaze-recognizers`

Detection backends registered through `gaze::RecognizerRegistry`:

- `RegexDetector` — named regex recognizer with optional validator (Luhn, IBAN MOD-97, IPv4/IPv6, VIN) and normalizer kinds.
- `DictionaryRecognizer` — Aho-Corasick multi-term recognizer for tenant-specific PII (order IDs, song titles, artist names). Terms flow in through `[[policy.custom_recognizers]]`, `terms_file`, or `--context-json`.
- `NerDetector` — optional transformer NER backend (ONNX + Davlan mBERT). Off unless `[ner] model_dir` resolves a valid bundle.

The crate also ships the embedded `core` rulepack (`gaze-recognizers/embedded/core.toml`) plus DACH/EN locale bundles.

### `gaze-cli`

Standalone `gaze` binary for LLM pipe-mode integration. Language-specific adapters (e.g. `gaze-laravel`) shell out to it rather than linking the library. See `docs/roadmap/v0.3/cli.md` for the full CLI contract and `docs/roadmap/v0.3/laravel.md` for the host-side integration. v0.4 flag additions: `--locale` (comma-separated priority chain) and `--context-json` (typed `DetectContext` envelope with tenant fields + dictionaries + class map).

#### CLI Example

Pseudonymize on the way out, restore on the way back:

```bash
echo "Email alice@example.invalid now" | gaze clean --policy=policy.toml
# {"clean_text":"Email <{session_hex}:Email_1> now","session_blob":"<base64>","stats":{"detections":1}}

echo '{"text":"Email <{session_hex}:Email_1> now","session_blob":"<base64>"}' | gaze restore
# {"text":"Email alice@example.invalid now"}
```

Counter-family tokens (`<{session_hex}:Email_N>`, `<{session_hex}:Name_N>`, `<{session_hex}:Location_N>`, `<{session_hex}:Organization_N>`, `<{session_hex}:Custom:name_N>`) are wrapped in angle brackets so the LLM cannot silently dissolve them into adjacent words. Format-preserving email tokens (`email1.{session_hex}@gaze-fake.invalid`) intentionally stay bare — the whole point is to look like a real email.

Default bundled-rulepack tokenization is a contract surface. The no-policy baselines for bundled outputs live in `crates/xtask/snapshots/`, and intentional drift requires a `[bundle-tokenization-drift]` `CHANGELOG.md` `[Unreleased]` Changed entry alongside a source ACK. See `ROADMAP.md` Now/Next/Later for the live stability context behind these gates.

#### Audit Query and Export (v0.4.3+)

When `gaze clean --audit-db <path>` is enabled, the metadata-only redaction log is queryable from the CLI:

```bash
gaze audit query --audit-db audit.sqlite --class email --action tokenize
gaze audit query --audit-db audit.sqlite --from 2026-04-25T00:00:00Z --to 2026-04-26T00:00:00Z
gaze audit export --audit-db audit.sqlite --format jsonl --output redactions.jsonl
```

Filters: `--class`, `--source`, `--action`, `--document-kind`, plus `--from <iso8601>` and `--to <iso8601>` time bounds (v0.4.4), and `--session <opaque>` for opaque session-scope filtering (v0.4.5 S1, NOT raw `session_hex`). The audit DB opens read-only; export rows ship a restricted column set so raw PII payloads stay outside the export surface.

#### Audit Purge (v0.4.5+)

Manual retention via `gaze audit purge`:

```bash
gaze audit purge --audit-db audit.sqlite --before 2026-01-01T00:00:00Z --dry-run
gaze audit purge --audit-db audit.sqlite --before 2026-01-01T00:00:00Z --count
gaze audit purge --audit-db audit.sqlite --before 2026-01-01T00:00:00Z
```

Calendar-aware ISO 8601 validation rejects malformed cutoffs fail-closed with the typed `AuditPurgeIso8601` error. Restricted DELETE clause; no policy-level retention default; no background auto-purge — adopters drive retention explicitly.

#### Policy Configuration

`gaze clean --policy=<path>` loads a TOML policy that declares recognizer bundles (`[policy.rulepacks]`), custom recognizers (`[[policy.custom_recognizers]]`), per-class rules, the locale chain, and optional NER bootstrap. See [`docs/policy.md`](docs/policy.md) for the full schema reference and worked examples, including the `[[detector]]` → `[[policy.custom_recognizers]]` migration.

Use `--audit-db=<path>` to persist the metadata-only SQLite redaction log for a
clean invocation. Dictionary sources include the matched term index as
`dictionary:{name}[#term_index]`.

#### Library Example

```rust
use gaze::{Action, ClassRule, Pipeline, PiiClass, RawDocument, Scope, Session};
use gaze_recognizers::RegexDetector;

let pipeline = Pipeline::builder()
    .recognizer(RegexDetector::emails()?)
    .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
    .build()?;

let session = Session::new(Scope::Conversation("msg-42".into()))?;
let clean = pipeline.redact(
    &session,
    RawDocument::Text("alice@example.invalid".to_string()),
)?;
```

#### NER Model Runtime

Transformer NER is optional and only enabled when a pinned local model directory is provided.

Expected runtime model directory:

```text
${XDG_DATA_HOME:-~/.local/share}/gaze/models/davlan-mbert-ner-hrl/
```

Required files:

- `model.onnx`
- `tokenizer.json`
- `config.json`
- `labels.json`
- `SHA256SUMS`

See [crates/gaze/testdata/ner/README.md](crates/gaze/testdata/ner/README.md) and [docs/research/ner-library-evaluation.md](docs/research/ner-library-evaluation.md).

### External MCP Consumer

The former in-tree `debug-proxy` MCP consumer now lives in [piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens). Keep Gaze changes focused on the pseudonymization runtime, recognizers, assembly layer, CLI, and repository gates.

## Build

```bash
cargo build --workspace
```

Release:

```bash
cargo build --release --workspace
```

## Verification

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## What's new since v0.4.0-rc.1

Cumulative highlights from v0.4.1 through v0.4.6 — see [CHANGELOG.md](CHANGELOG.md) for the per-release detail.

### v0.4.5 highlights

- **Audit retention manual purge** (v0.4.5 S3) — `gaze audit purge --before <iso8601> [--dry-run | --count]` deletes redaction-log rows older than the cutoff. Calendar-aware ISO 8601 validation rejects malformed dates fail-closed via the typed `AuditPurgeIso8601` error. No policy-level retention default; no background auto-purge.
- **`audit_metadata_only` xtask gate** (v0.4.5 S3) — compile-time enforcement that restore-path code does not import audit metadata symbols. Walker covers file scope, nested `mod`, function/impl/trait-default/const/static block-statement `use`, glob imports, aliased crates, `extern crate`, and `#[path]`-resolved external modules. Known limitations (fully-qualified path references, `include!`, let-else diverge, macro-emit) documented in [`docs/architecture/xtask.md`](docs/architecture/xtask.md); v0.5 architectural pivot to dylint-based name-resolution lint scheduled (todo #181, see [`docs/research/v0.5-dylint-audit-gate.md`](docs/research/v0.5-dylint-audit-gate.md)).
- **`--session` audit filter** (v0.4.5 S1) — opaque session-scope filter for `gaze audit query` / `gaze audit export`. Filters by opaque audit metadata, NOT raw `session_hex`.
- **DE + US national phone recognizers** (v0.4.5 S2) — parser-backed E.164 region-aware validators (`phonenumber` crate) for German and US national phone numbers. Cooperate with the structural phone recognizer; gated behind the `phone-parser` Cargo feature.
- **`core-extended` no-policy locale activation** (v0.4.5 S2) — the bundled `core-extended` rulepack now activates `phone.national.de`, `phone.national.us`, `postal.us`, and `postal.de` recognizers when invoked without a policy via `--rulepack-bundled core-extended`. Previously these required an explicit `--locale` or policy-supplied locale. **Adopter impact:** invocations without a policy now tokenize German/US national phone numbers AND bare 5-digit numeric strings (matching the postal recognizers). To restore prior behavior, supply `--locale=global` or pass a policy with narrower locale gating. (todo #171)
- **`gaze-assembly` crate restructure** (v0.4.5 S6) — `lib.rs` split into focused modules by responsibility. No public API change.
- **ClassMapOverrideSafety extension** (v0.4.5 S4) — further hardening of the v0.4.4 class-map override safety gate.
- **Rulepack version bump validation** (v0.4.5 S5) — rulepack version bump audit + drift-prevention rule.

### Detection: validators and the `core-extended` rulepack

- **`ValidatorKind` substrate** (v0.4.3) — `Luhn` (Mod 10), `IbanMod97` (ISO 7064), `IbanCanonical` (uppercase plus whitespace strip) join `EmailRfc` as closed validator/normalizer enums in `gaze-recognizers`.
- **`E164Phone` parser-backed validator** (v0.4.4) — built on the `phonenumber` crate, gated behind the optional `phone-parser` feature. `gaze-cli` enables it by default; library users opt in via `gaze-recognizers = { features = ["phone-parser"] }`. Without the feature, `e164_phone` is rejected at rulepack load time with `RulepackError::UnsupportedValidator` rather than silently dropping detection.
- **`core-extended` rulepack** (v0.4.2 Phase 1, v0.4.3 Phase 2) — opt-in shipped rulepack. Phase 1 covers shape-only E.164 phone numbers, IPv4/IPv6 addresses, and `de-DE`/`en-US` postal codes. Phase 2 adds validator-backed IBAN (`iban.structural`, `iban_mod97` + `iban_canonical`) and credit card (`card.structural`, `luhn`). Default `[[rule]]` entries ship in the rulepack so `--rulepack-bundled core,core-extended` tokenizes the new classes out of the box.
- **`email.header.name` recognizer** (v0.4.2) — locale-aware regex for RFC822 display names, including German `Von:` / `An:` headers. Closes the prompt-preamble NER gap from issue #24.
- **`[ner] threshold` knob** (v0.4.2) — `--ner-threshold` overrides the per-span confidence floor for tuning prompt-preamble PII without retraining the model.

### CLI surface

- **Three-surfaces backfill** (v0.4.2 S1) — `gaze clean` exposes `--session-scope`, `--ner-model-dir`, `--ner-locale`, `--rulepack-bundled`, and `--rulepack-path` overrides for existing policy knobs. Modular split moves `commands` / `pipeline` / `restore` / `io` / `error` / `logger` into their own files.
- **`gaze clean --audit-db`** (v0.4.2) — persists the metadata-only SQLite redaction log for pipe-mode invocations. Dictionary sources include per-term traceability as `dictionary:{name}[#term_index]`.
- **`gaze audit query` / `gaze audit export`** (v0.4.3 S4) — read-only audit metadata export from the SQLite log. Filters: `--class`, `--source`, `--action`, `--document-kind`. JSONL is the default output format. The audit DB opens read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY`.
- **Audit schema v2** (v0.4.4 S2) — `RedactionEntry` carries `created_at` epoch milliseconds; on-open `ALTER TABLE` migration keeps legacy DBs queryable through a NULL default. `gaze audit query` and `gaze audit export` accept `--from <iso8601>` and `--to <iso8601>` filters.

### Linux releases

- **Linux x86_64 binary** (v0.4.2 S4) — release CI publishes `gaze-x86_64-unknown-linux-gnu` from a native `ubuntu-24.04` runner alongside `gaze-aarch64-apple-darwin`, with `.sha256` files for both artifacts. Requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer); older distros should build from source.

### Repository gates (xtask)

- **`SymmetricPotemkin`** (v0.4.1) — runs the named behavioral tests for symmetric audit-merge entries.
- **`RecognizerCompositionValidator`** — guards same-class rulepack composition; missing `cooperates_with` declarations fail closed with `RulepackError::SameClassWithoutCooperation`.
- **`NoTenantKnowledge`** (v0.4.3 S3) — production-code lint scanner rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. `// allow(tenant-fixture)` markers hard-fail in production scope.
- **`ClassMapOverrideSafety`** (v0.4.4 S1) — previously scaffolded gate is now active. `cargo run -p xtask -- class-map-override-safety` runs `t20_context_class_map_overrides_policy_dict_class` and `t20a_class_map_override_fails_closed_when_action_rule_uncovered`. Adversarial in-PR self-test verifies the gate fails non-zero when a listed test is missing or renamed.
- **`audit_metadata_only`** (v0.4.5 S3) — compile-time enforcement that restore-path code does not import audit metadata symbols. Syn-based AST walker covers file scope, nested `mod`, function/impl/trait-default/const/static block-statement `use`, glob imports, aliased crates, `extern crate`, and `#[path]`-resolved external modules. Known limitations (fully-qualified path references, `include!`, let-else diverge, macro-emit) and the v0.5 dylint pivot are documented in [docs/architecture/xtask.md](docs/architecture/xtask.md).

See [docs/architecture/xtask.md](docs/architecture/xtask.md) for the gate authoring contract.

### Date posture

`docs/research/v0.4.4-date-posture.md` (v0.4.4 S4) locks Gaze's Date-as-PII stance: dates are not PII by default, never ship in default `core` or `core-extended` bundles. General-prose dates require context classification research for v0.5+.

## Roadmap teaser — v0.5

- **Open-key `PiiClass`** — sketched in [`docs/design/v0.5-open-piiclass.md`](docs/design/v0.5-open-piiclass.md). Replaces the closed enum with an open-key string interner so adopters and rulepacks can introduce new classes without core changes.
- **Crate-shape Option B** — `gaze-types` extraction is underway for v0.5; `gaze-assembly` remains the policy-to-pipeline joining layer.

Deferred beyond v0.5:

- real sandbox backend implementations
- k-anonymity / query-budget controls
- full format-preserving fake generation

See [docs/ROADMAP.md](docs/ROADMAP.md) for v0.5 directions.

## Adopter notes

- **Linux distros** — the published Linux x86_64 binary requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer). On older distros, build from source with `cargo build --release -p gaze-cli`.
- **Phone validation** — `phone-parser` is enabled by default for `gaze-cli`. Library consumers that want parser-backed E.164 validation must opt in: `gaze-recognizers = { features = ["phone-parser"] }`. Without that feature, the rulepack loader rejects `e164_phone` at load time, preserving fail-closed behavior rather than silently degrading to shape-only matching.
- **`core-extended` no-policy activation (v0.4.5)** — invocations of `--rulepack-bundled core-extended` *without a policy* now activate `phone.national.de`, `phone.national.us`, `postal.us`, and `postal.de` recognizers. Adopters relying on the prior behavior (no national phone tokenization, no bare 5-digit numeric tokenization) must pass `--locale=global` or supply a policy with narrower locale gating. (todo #171)
- **Audit time filters** — `gaze audit query` / `gaze audit export` accept ISO 8601 timestamps via `--from` and `--to`. Legacy v0.4.3 audit DBs without `created_at` are still queryable, but time-filtered queries exclude their NULL timestamp rows by SQL semantics.
- **Audit retention (v0.4.5)** — there is no policy-level retention default and no background auto-purge. Adopters who need retention drive it via `gaze audit purge --before <iso8601>` (preview with `--dry-run` or `--count`).
- **Tenant-class fixture discipline** — production code in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/` may not contain tenant-specific patterns such as `order_id`, `Order_42`, `Song_42`, `User_7`. The `cargo run -p xtask -- no-tenant-knowledge` gate enforces this in CI. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

Apache-2.0.
