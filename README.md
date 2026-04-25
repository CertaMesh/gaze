# Gaze

**Reversible PII pseudonymization for agentic workflows.**

Gaze lets AI tools inspect logs, database samples, support messages, and production-adjacent data without exposing real personal data to the agent.

It does not merely redact. It replaces PII with stable, reversible tokens, so the data owner can safely restore the original values later — while the agent only ever sees pseudonyms.

**Clean in. Safe out. Restore when needed. No silent leaks.**

## What Gaze is

Gaze is a reversible PII pseudonymization runtime that lets AI agents work with production data without ever seeing the real personal data.

The workspace has four crates:

- `crates/gaze` — core library: pipeline, sessions, policy loader, recognizer registry, locale chain, rulepack schema, token grammar.
- `crates/gaze-recognizers` — detection backends plugged into the registry (regex, dictionary, NER) and bundled rulepacks.
- `crates/gaze-cli` — the `gaze clean` / `gaze restore` binary adopters invoke from language adapters.
- `crates/debug-proxy` — MCP debug server for MySQL + Laravel logs, built on top of `gaze`.

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

## Install (v0.4.0-rc.1)

v0.4.0-rc.1 is a release candidate — pin it explicitly while dogfooding.

Apple Silicon macOS via Homebrew (tap):

```bash
brew install Naoray/gaze/gaze
```

Direct binary download from the release assets:

```bash
curl -LO https://github.com/Naoray/gaze/releases/download/v0.4.0-rc.1/gaze-v0.4.0-rc.1-aarch64-apple-darwin.tar.gz
tar -xzf gaze-v0.4.0-rc.1-aarch64-apple-darwin.tar.gz
mv gaze /usr/local/bin/gaze
```

Linux and Intel macOS binaries are not published in v0.4.0-rc.1; they return in a later release once the runner and runtime story is pinned. Build from source with `cargo build --release -p gaze-cli` in the meantime.

## Workspace Layout

```text
crates/
  gaze/               core library (pipeline, sessions, policy, registry, locale, rulepack)
  gaze-recognizers/   detection backends (regex, dictionary, NER) + bundled rulepacks
  gaze-cli/           standalone `gaze` binary for LLM pipe-mode integrations
  debug-proxy/        MCP debug server consumer for MySQL + Laravel logs
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

### `gaze-recognizers`

Detection backends registered through `gaze::RecognizerRegistry`:

- `RegexDetector` — named regex recognizer with optional validator (Luhn, IBAN MOD-97, IPv4/IPv6, VIN) and normalizer kinds.
- `DictionaryRecognizer` — Aho-Corasick multi-term recognizer for tenant-specific PII (order IDs, song titles, artist names). Terms flow in through `[[policy.custom_recognizers]]`, `terms_file`, or `--context-json`.
- `NerDetector` — optional transformer NER backend (ONNX + Davlan mBERT). Off unless `[ner] model_dir` resolves a valid bundle.

The crate also ships the embedded `core` rulepack (`gaze-recognizers/embedded/core.toml`) plus DACH/EN locale bundles.

### `gaze-cli`

Standalone `gaze` binary for LLM pipe-mode integration. Language-specific adapters (e.g. `gaze-laravel`) shell out to it rather than linking the library. See `docs/roadmap/v0.3/cli.md` for the full CLI contract and `docs/roadmap/v0.3/laravel.md` for the host-side integration. v0.4 flag additions: `--locale` (comma-separated priority chain) and `--context-json` (typed `DetectContext` envelope with tenant fields + dictionaries + class map).

#### CLI Example

```bash
echo "Email alice@example.invalid now" | gaze clean --policy=policy.toml
# {"clean_text":"Email <{session_hex}:Email_1> now","session_blob":"<base64>","stats":{"detections":1}}
```

Counter-family tokens (`<{session_hex}:Email_N>`, `<{session_hex}:Name_N>`, `<{session_hex}:Location_N>`, `<{session_hex}:Organization_N>`, `<{session_hex}:Custom:name_N>`) are wrapped in angle brackets so the LLM cannot silently dissolve them into adjacent words. Format-preserving email tokens (`email1.{session_hex}@gaze-fake.invalid`) intentionally stay bare — the whole point is to look like a real email.

#### Policy Configuration

`gaze clean --policy=<path>` loads a TOML policy that declares recognizer bundles (`[policy.rulepacks]`), custom recognizers (`[[policy.custom_recognizers]]`), per-class rules, the locale chain, and optional NER bootstrap. See [`docs/policy.md`](docs/policy.md) for the full schema reference and worked examples, including the `[[detector]]` → `[[policy.custom_recognizers]]` migration.

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

### `debug-proxy`

MCP server consumer built on top of `gaze`.

Commands:

```text
debug-proxy init
debug-proxy check [policy.toml]
debug-proxy serve [policy.toml]
```

#### What It Exposes

- `db.schema`
- `db.sample`
- `db.count`
- `db.distinct`
- `db.explain`
- `logs.search`
- `logs.context`
- `logs.tail`

#### Typical Flow

1. Scaffold a policy:

```bash
cargo run -p debug-proxy -- init
```

2. Validate it:

```bash
cargo run -p debug-proxy -- check policy.toml
```

3. Serve MCP over stdio:

```bash
cargo run -p debug-proxy -- serve policy.toml
```

#### Policy Notes

- one production connection is required
- table/column scope is allowlisted
- NER locale is configured in policy
- shared session state lets DB rows and logs reuse the same pseudonyms

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

## What's new in v0.4.0-rc.1

Shipped 2026-04-24 (see [CHANGELOG.md](CHANGELOG.md) for the full entry):

- **Crate split** — `gaze` (core) + `gaze-recognizers` (detection backends) + `gaze-cli` (binary) + `debug-proxy` (MCP consumer). Debug-proxy canary test tightened to assert the 8-hex `{session_hex}` shape.
- **`RecognizerRegistry` is the detection path.** The legacy standalone `Detector` trait path was removed; every detection routes through the registry with a typed `DetectContext` envelope.
- **F3 rulepack schema** — TOML-defined recognizer bundles (`[policy.rulepacks]`) with a closed validator/normalizer kind registry. Unknown matchers fail closed.
- **F4 locale chain** — 4-tier: CLI > policy > rulepack default > system default. Per-recognizer locale gating via `locales = [...]`; `LocaleTag::Other(_)` matches strict-equal only.
- **F2-full resolver** — class-priority > rule-priority > score > span-length > recognizer-id, with a multi-overlap fixed-point pass. Audit entries carry `decided_by: ConflictTier` + merge-loser rows.
- **F5 `.invalid` domain** — format-preserving email fakes now use `email{N}.{session_hex}@gaze-fake.invalid`. The legacy `example.test` Pass 2 trap arm is retained for v0.3 manifest restore compatibility.
- **F6 Dictionary recognizer** — Aho-Corasick multi-term detector for tenant PII, registered through the `Recognizer` trait. Adopter-tunable via `[[policy.custom_recognizers]]`, `terms_file`, or `--context-json`.
- **Typed `Context` envelope** — `--context-json` carries tenant `fields` / `dictionaries` / `class_map` through `DetectContext` instead of being parsed-and-dropped.
- **F7.5 byte-range-skip** — Pass 1 substitution spans are tracked; Pass 2 trap scan skips fully-contained matches. Closes the cascade false-positive where adopter raw values matching trap arms (`Order_42`, `Song_42`, `User_7`) were rejected in strict mode (PR #22).
- **Migration:** legacy top-level `[[detector]]` is rejected with `LegacyDetectorUnsupported`. Move blocks to `[[policy.custom_recognizers]]` — see [docs/policy.md](docs/policy.md#migrating-detector).

Known limits to surface during dogfooding (tracked for v0.4.1):

- `token.family` / `token.format` and `context.hotwords` / `boost` / `window` are parsed only for schema validation in v0.4.0-rc.1; non-default values fail closed with `RulepackError::UnsupportedField` until runtime consumers ship in v0.4.1.
- Dictionary audit log carries `dictionary:{name}`; per-term `[#term_index]` is v0.4.1.
- NER context-sensitivity gap on prompt boilerplate / RFC822 email headers — workarounds + roadmap in issue #24.

Deferred beyond v0.4:

- real sandbox backend implementations
- k-anonymity / query-budget controls
- full format-preserving fake generation

See [docs/ROADMAP.md](docs/ROADMAP.md) for v0.4.1 and v0.5 directions.

## License

Apache-2.0.
