# Gaze

Channel-agnostic redaction workspace for AI-facing production tooling.

The workspace now has two crates:

- `crates/gaze` — shared redaction core library (and, in v0.3, the standalone `gaze clean` / `gaze restore` CLI)
- `crates/debug-proxy` — MCP debug server for MySQL + Laravel logs

## Workspace Layout

```text
crates/
  gaze/         core library + standalone CLI (v0.3)
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

v0.3 adds a standalone CLI that consumes the library for LLM pipe-mode integration (Laravel wrapper ships out-of-tree via `gaze/laravel`). See `docs/roadmap/v0.3/cli.md` for the surface and `docs/roadmap/v0.3/laravel.md` for the host integration.

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

Implemented for the v0.2 rewrite:

- shared `gaze` core
- `debug-proxy` consumer
- core sandbox trait shape

In progress for v0.3:

- standalone `gaze clean` / `gaze restore` CLI (see `docs/roadmap/v0.3/cli.md`)
- `policy.toml` loader → `Pipeline::from_policy(...)` helper

Deferred beyond v0.3:

- real sandbox backend implementations
- k-anonymity / query-budget controls
- full format-preserving fake generation

## License

Apache-2.0.
