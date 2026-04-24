# Gaze

**Reversible PII pseudonymization for agentic workflows.**

Gaze lets AI tools inspect logs, database samples, support messages, and production-adjacent data without exposing real personal data to the agent.

It does not merely redact. It replaces PII with stable, reversible tokens, so the data owner can safely restore the original values later — while the agent only ever sees pseudonyms.

**Clean in. Safe out. Restore when needed. No silent leaks.**

## What Gaze is

Gaze is a reversible PII pseudonymization runtime that lets AI agents work with production data without ever seeing the real personal data.

The workspace has two crates:

- `crates/gaze` — shared redaction core library and the standalone `gaze clean` / `gaze restore` CLI
- `crates/debug-proxy` — MCP debug server for MySQL + Laravel logs

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

## Install (v0.3.0)

Apple Silicon macOS via Homebrew (tap):

```bash
brew install Naoray/gaze/gaze
```

Direct binary download from the release assets:

```bash
curl -LO https://github.com/Naoray/gaze/releases/download/v0.3.0/gaze-aarch64-apple-darwin
chmod +x gaze-aarch64-apple-darwin
mv gaze-aarch64-apple-darwin /usr/local/bin/gaze
```

Linux and Intel macOS binaries are not published in v0.3.0; they return in a later release once the runner and runtime story is pinned. Build from source with `cargo build --release` in the meantime.

## Workspace Layout

```text
crates/
  gaze/         core library + standalone CLI
  debug-proxy/  MCP debug server consumer
```

## Crate Guide

### `gaze`

Pure Rust library for:

- detector composition
- rule-based redaction
- session-scoped tokenization
- signed sensitive snapshots
- redaction logging
- pluggable sandbox trait shape for future action-side work

The standalone CLI consumes the library for LLM pipe-mode integration (Laravel wrapper ships out-of-tree via `gaze/laravel`). See `docs/roadmap/v0.3/cli.md` for the surface and `docs/roadmap/v0.3/laravel.md` for the host integration.

#### CLI Example

```bash
echo "Email alice@example.invalid now" | gaze clean --policy=policy.toml
# {"clean_text":"Email <{session_hex}:Email_1> now","session_blob":"<base64>","stats":{"detections":1}}
```

Counter-family tokens (`<{session_hex}:Email_N>`, `<{session_hex}:Name_N>`, `<{session_hex}:Location_N>`, `<{session_hex}:Organization_N>`, `<{session_hex}:Custom:name_N>`) are wrapped in angle brackets so the LLM cannot silently dissolve them into adjacent words. Format-preserving email tokens (`email1.{session_hex}@gaze-fake.invalid`) intentionally stay bare — the whole point is to look like a real email.

#### Policy Configuration

`gaze clean --policy=<path>` loads a TOML policy that declares detectors, classes, and per-class actions. See [`docs/policy.md`](docs/policy.md) for the full schema reference and worked examples.

#### Library Example

```rust
use gaze::{
    Action, ClassRule, Pipeline, RawDocument, RegexDetector, Scope, Session, PiiClass,
};

let pipeline = Pipeline::builder()
    .detector(RegexDetector::emails()?)
    .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
    .build()?;

let session = Session::new(Scope::Conversation("msg-42".into()))?;
let clean = pipeline.redact(
    &session,
    RawDocument::Text("alice@example.com".to_string()),
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
cargo build
```

Release:

```bash
cargo build --release
```

## Verification

```bash
cargo test -p gaze -p debug-proxy
cargo clippy -p gaze -p debug-proxy --all-targets --all-features -- -D warnings
```

## Status

Shipped in v0.3.0 (2026-04-24):

- shared `gaze` core library
- `debug-proxy` MCP consumer
- standalone `gaze clean` / `gaze restore` CLI (see `docs/roadmap/v0.3/cli.md`)
- `policy.toml` loader + `Pipeline::from_policy(...)` helper
- angle-bracket-wrapped counter tokens + `gaze::token_shape` grammar
- two-pass restore (exact token match + shape-validator) with `UnknownToken` fail-closed signal
- session TTL enforcement (`issued_at` on snapshot payload, `BlobExpired` exit bucket)
- structured stderr JSON with stable exit buckets
- Apple Silicon macOS binary + Homebrew tap formula

v0.4 (in flight, plan under review — not imminent):

- engine / corpus crate split
- `RecognizerRegistry` trait for stackable detectors
- full TOML rulepack schema
- DACH + EN locale infrastructure
- `.invalid` domain switch for format-preserving fakes
- dictionary detector + typed `Context` envelope
- text-provenance fingerprint (library-side blob↔text scope isolation)

Deferred beyond v0.4:

- real sandbox backend implementations
- k-anonymity / query-budget controls
- full format-preserving fake generation

## License

Apache-2.0.
