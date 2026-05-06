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

"Pseudonymization" is the GDPR Art. 4(5) term for reversible substitution with tokens — that reversibility is the Gaze moat, not one-way redaction. See [AGENTS.md](AGENTS.md#project-north-star) for the locked north star and rationale.

## Five Axes

Every design, implementation, and review decision is evaluated against these five axes. Full rationale: [AGENTS.md](AGENTS.md#project-north-star).

1. **Reliability (never leak).** Fail-closed always; defense in depth across regex, NER, dictionary, and optional neural safety net.
2. **Reversibility.** Manifest-first restore; no one-way primitives in the core contract.
3. **Agentic-first.** Prioritizes agent-workflow needs (tool-call JSON, streaming, multi-turn, tenant PII) over generic text handling.
4. **Trust (auditable + deterministic).** Rule-based detectors preferred; every token emission traceable to a rule or recognizer.
5. **Adopter ergonomics.** Low-friction framework adapters; adopter picks Gaze up in under a day without deep PII expertise.

## Install

Apple Silicon macOS via release asset:

```bash
curl -L -o gaze https://github.com/PIInuts/gaze/releases/latest/download/gaze-aarch64-apple-darwin
chmod +x gaze
mv gaze /usr/local/bin/gaze
```

Homebrew tap installation is not yet available. The formula source exists at `dist/homebrew/gaze.rb`, but no `piinuts/tap` or `piinuts/homebrew-tap` formula is published yet. Maintainers can smoke the formula by staging it into a scratch local tap; direct `brew install piinuts/tap/gaze` is not supported yet.

`brew install piinuts/tap/gaze` will be documented once a public tap exists and the release process publishes to it.

Linux x86_64 binary download from the release assets:

```bash
curl -L -o gaze https://github.com/PIInuts/gaze/releases/latest/download/gaze-x86_64-linux-gnu
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
- Network access NOT used at runtime
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
- `RecognizerRegistry` trait + `DetectContext` envelope (the recognizer-native detection path; the legacy standalone `Detector` path was removed)
- class/rule/score/length/id conflict resolver
- locale chain (CLI > policy > rulepack default > system default) with strict opaque-tag matching
- TOML rulepack schema loader (`[policy.rulepacks]`, `[[policy.custom_recognizers]]`)
- redaction logger + audit symmetry (`decided_by` + merge-loser entries)
- pluggable sandbox trait shape for future action-side work

### `gaze-audit`

Passive audit sink crate. Owns `SqliteLogger`, `AuditFilter`, `AuditLogRow`,
`build_audit_query_sql`, and `AUDIT_RESTRICTED_COLUMNS`. `gaze` does not depend
on this crate in default or `--no-default-features` builds; adopters that need
SQLite audit logging depend on `gaze-audit` directly.

### `gaze-recognizers`

Detection backends registered through `gaze::RecognizerRegistry`:

- `RegexDetector` — named regex recognizer with optional validator (Luhn, IBAN MOD-97, IPv4/IPv6, VIN) and normalizer kinds.
- `DictionaryRecognizer` — Aho-Corasick multi-term recognizer for tenant-specific PII (order IDs, song titles, artist names). Terms flow in through `[[policy.custom_recognizers]]`, `terms_file`, or `--context-json`.
- `NerDetector` — optional transformer NER backend (ONNX + Davlan mBERT). Off unless `[ner] model_dir` resolves a valid bundle.

The crate also ships the embedded `core` rulepack (`gaze-recognizers/embedded/core.toml`) plus DACH/EN locale bundles.

### `gaze-cli`

Standalone `gaze` binary for LLM pipe-mode integration. Language-specific adapters (e.g. `gaze-laravel`) shell out to it rather than linking the library. The CLI contract centers on stdin/stdout JSON for `gaze clean` and `gaze restore`, with runtime overrides such as `--locale` (comma-separated priority chain) and `--context-json` (typed `DetectContext` envelope with tenant fields, dictionaries, and class map).

#### CLI Example

Pseudonymize on the way out, restore on the way back:

```bash
echo "Email alice@example.invalid now" | gaze clean --policy=policy.toml
# {"clean_text":"Email <{session_hex}:Email_1> now","session_blob":"<base64>","stats":{"detections":1}}

echo '{"text":"Email <{session_hex}:Email_1> now","session_blob":"<base64>"}' | gaze restore
# {"text":"Email alice@example.invalid now"}
```

Counter-family tokens (`<{session_hex}:Email_N>`, `<{session_hex}:Name_N>`, `<{session_hex}:Location_N>`, `<{session_hex}:Organization_N>`, `<{session_hex}:Custom:name_N>`) are wrapped in angle brackets so the LLM cannot silently dissolve them into adjacent words. Format-preserving email tokens (`email1.{session_hex}@gaze-fake.invalid`) intentionally stay bare — the whole point is to look like a real email.

Default bundled-rulepack tokenization is a contract surface. The no-policy baselines for bundled outputs live in `crates/xtask/snapshots/`, and intentional drift requires a `[bundle-tokenization-drift]` `CHANGELOG.md` `[Unreleased]` Changed entry alongside a source ACK.

#### Audit Query and Export

When `gaze clean --audit-db <path>` is enabled, the metadata-only redaction log is queryable from the CLI:

```bash
gaze audit query --audit-db audit.sqlite --class email --action tokenize
gaze audit query --audit-db audit.sqlite --from 2026-04-25T00:00:00Z --to 2026-04-26T00:00:00Z
gaze audit export --audit-db audit.sqlite --format jsonl --output redactions.jsonl
```

Filters: `--class`, `--source`, `--action`, `--document-kind`, plus `--from <iso8601>` and `--to <iso8601>` time bounds, and `--session <opaque>` for opaque session-scope filtering (NOT raw `session_hex`). The audit DB opens read-only; export rows ship a restricted column set so raw PII payloads stay outside the export surface.

#### Audit Purge

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

## Detection coverage

Gaze runs a layered detection pipeline built for fail-closed agent workflows:
deterministic regex recognizers, dictionary recognizers, optional transformer
NER, and an observer-only Pass-3 SafetyNet that checks already-cleaned text
without mutating the manifest or restore path. See
[`docs/policy.md`](docs/policy.md) for policy shape and
[`docs/architecture/safety-nets.md`](docs/architecture/safety-nets.md) for the
SafetyNet contract.

Bundled rulepacks:

- `core` — always-on email detection plus locale-aware email header and
  cue-anchored `Name` coverage for forward headers, agent reply preambles, and
  auto-footer sender lines.
- `core-extended` — opt-in phone, IP address, postal code, IBAN, and credit-card
  recognizers, with default rules so `--rulepack-bundled core,core-extended`
  tokenizes those classes out of the box.

Locale bundles and cue buckets let adopters compose `core` with DACH and EN
language cues without custom recognizers. The locale chain is strict and
ordered: CLI override, policy, bundled rulepack default, then system default.

Validators keep structural matches from becoming broad regex guesses: Luhn for
payment-card shapes, IBAN MOD-97 plus canonicalization for IBANs, IPv4/IPv6 and
VIN validators, and parser-backed E.164 / national phone validation when the
`phone-parser` feature is enabled.

## Audit & restore

Gaze's restore contract is manifest-first: emitted tokens are session-scoped,
countered by class, and restored only through the signed sensitive snapshot.
The optional SQLite audit sink is metadata-only and lives in
`gaze-audit::SqliteLogger`; `gaze` core does not carry SQLite in default builds.

`gaze audit query`, `gaze audit export`, `gaze audit purge`, and
`gaze audit safety-net query` provide read-side audit and retention operations
without exposing raw PII payloads. See
[`docs/architecture/crates.md`](docs/architecture/crates.md) for crate
boundaries and [`docs/policy.md`](docs/policy.md) for adopter-facing policy
examples.

## Repository gates

The `xtask` gate matrix protects the contracts above: recognizer composition,
tenant-fixture hygiene, class-map override safety, bundle-tokenization drift,
SafetyNet sanity, cargo metadata audit isolation, and the resolver-based
`gaze_module_isolation` Dylint gate. The full gate inventory and authoring
contract live in [`docs/architecture/xtask.md`](docs/architecture/xtask.md).

## Adopter notes

- **Linux distros** — the published Linux x86_64 binary requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer). On older distros, build from source with `cargo build --release -p gaze-cli`.
- **Phone validation** — `phone-parser` is enabled by default for `gaze-cli`. Library consumers that want parser-backed E.164 validation must opt in: `gaze-recognizers = { features = ["phone-parser"] }`. Without that feature, the rulepack loader rejects `e164_phone` at load time, preserving fail-closed behavior rather than silently degrading to shape-only matching.
- **`core-extended` no-policy activation** — invocations of `--rulepack-bundled core-extended` *without a policy* activate `phone.national.de`, `phone.national.us`, `postal.us`, and `postal.de` recognizers. Adopters relying on no national phone tokenization or no bare 5-digit numeric tokenization must pass `--locale=global` or supply a policy with narrower locale gating.
- **Audit time filters** — `gaze audit query` / `gaze audit export` accept ISO 8601 timestamps via `--from` and `--to`. Legacy audit DBs without `created_at` are still queryable, but time-filtered queries exclude their NULL timestamp rows by SQL semantics.
- **Audit retention** — there is no policy-level retention default and no background auto-purge. Adopters who need retention drive it via `gaze audit purge --before <iso8601>` (preview with `--dry-run` or `--count`).
- **Tenant-class fixture discipline** — production code in `crates/{gaze,gaze-types,gaze-recognizers,gaze-assembly,gaze-cli}/src/` may not contain tenant-specific patterns such as `order_id`, `Order_42`, `Song_42`, `User_7`. The `cargo run -p xtask -- no-tenant-knowledge` gate enforces this in CI. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

Apache-2.0.
