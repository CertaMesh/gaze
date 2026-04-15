# Gaze

Channel-agnostic redaction workspace for AI-facing production tooling.

`gaze` v0.2 is no longer a single-purpose debug proxy binary. The workspace now has three crates:

- `crates/gaze` — shared redaction core library
- `crates/debug-proxy` — MCP debug server for MySQL + Laravel logs
- `crates/ghostwriter` — deterministic sanitize/restore tool for LLM-facing customer text

## Workspace Layout

```text
crates/
  gaze/         core library
  debug-proxy/  MCP debug server consumer
  ghostwriter/  sanitize/restore consumer
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

What it does not do by itself:

- no standalone `gaze clean` / `gaze restore` CLI yet
- no sandbox backend implementation yet
- no direct application protocol

Those are consumer concerns.

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

See [crates/gaze/testdata/ner/README.md](/Users/krishankonig/Workspace/bets/Gaze/crates/gaze/testdata/ner/README.md) and [docs/research/ner-library-evaluation.md](/Users/krishankonig/Workspace/bets/Gaze/docs/research/ner-library-evaluation.md).

### `ghostwriter`

Deterministic sanitize/restore wrapper around `gaze` for customer-facing LLM flows.

Commands:

```text
ghostwriter sanitize
ghostwriter restore
```

#### Sanitize Input

```json
{
  "text": "Hallo Markus Müller, bitte antworten Sie an mueller.markus@icloud.com",
  "context": {
    "customer_name": "Markus Müller",
    "customer_email": "mueller.markus@icloud.com",
    "customer_phone": "+49 151 23456789",
    "order_ids": ["SO-12345"],
    "songs": ["Midnight City"],
    "artists": ["M83"],
    "locale": "de"
  }
}
```

Supported `context` fields:

- `customer_name`
- `customer_email`
- `customer_phone`
- `order_ids`
- `songs`
- `artists`
- `locale`

Notes:

- known customer fields become semantic placeholders like `<CUSTOMER_NAME>`
- indexed values become placeholders like `<ORDER_ID_1>` or `<SONG_1>`
- regex email detection is always enabled
- transformer NER is only attempted when `GAZE_NER_MODEL_DIR` is set
- if no NER model directory is set, sanitize still succeeds

#### Sanitize Usage

```bash
cargo run -p ghostwriter -- sanitize < sanitize.json
```

Pretty-print:

```bash
cargo run -p ghostwriter -- sanitize < sanitize.json | jq
```

#### Restore Usage

```bash
cargo run -p ghostwriter -- restore < restore.json
```

Restore input shape:

```json
{
  "text": "Hallo <CUSTOMER_NAME>, wir senden an <CUSTOMER_EMAIL>.",
  "session_blob": "opaque blob returned by sanitize"
}
```

#### Example

Sanitize:

```bash
printf '%s\n' '{"text":"Betreff: Rückfrage zu Bestellung SO-12345\n\nHallo Markus Müller,\n\nvielen Dank für Ihre Nachricht. Wir haben die Bestellung SO-12345 geprüft und den Versand der Dateien soeben erneut angestoßen.\n\nDie Unterlagen gehen wie gewünscht an mueller.markus@icloud.com. Falls Sie stattdessen die alternative Adresse markus.mueller@example.de verwenden möchten, geben Sie uns bitte kurz Bescheid.\n\nWenn noch etwas fehlt, erreichen wir Sie auch telefonisch unter +49 151 23456789.\n\nFreundliche Grüße\nAnna Becker\nKundensupport\n","context":{"customer_name":"Markus Müller","customer_email":"mueller.markus@icloud.com","customer_phone":"+49 151 23456789","order_ids":["SO-12345"],"locale":"de"}}' | cargo run -p ghostwriter -- sanitize | jq
```

Restore:

```bash
printf '%s\n' '{"text":"Betreff: Rückfrage zu Bestellung <ORDER_ID_1>\n\nHallo <CUSTOMER_NAME>,\n\nvielen Dank für Ihre Nachricht. Wir haben die Bestellung <ORDER_ID_1> geprüft und den Versand der Dateien soeben erneut angestoßen.\n\nDie Unterlagen gehen wie gewünscht an <CUSTOMER_EMAIL>. Falls Sie stattdessen die alternative Adresse <EMAIL_1> verwenden möchten, geben Sie uns bitte kurz Bescheid.\n\nWenn noch etwas fehlt, erreichen wir Sie auch telefonisch unter <CUSTOMER_PHONE>.\n\nFreundliche Grüße\nAnna Becker\nKundensupport\n","session_blob":"PASTE_SESSION_BLOB_HERE"}' | cargo run -p ghostwriter -- restore | jq
```

### `debug-proxy`

MCP server consumer built on top of `gaze`.

Commands:

```text
debug-proxy init
debug-proxy check [policy.toml]
debug-proxy serve [policy.toml]
debug-proxy audit
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

4. Inspect the redaction log:

```bash
cargo run -p debug-proxy -- audit
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
cargo test -p ghostwriter -p gaze -p debug-proxy
cargo clippy -p ghostwriter -p gaze -p debug-proxy --all-targets --all-features -- -D warnings
```

## Status

Implemented for the v0.2 rewrite:

- shared `gaze` core
- `debug-proxy` consumer
- `ghostwriter` consumer
- core sandbox trait shape

Deferred beyond v0.2:

- standalone `gaze clean` / `gaze restore` CLI
- real sandbox backend implementations
- k-anonymity / query-budget controls
- full format-preserving fake generation

## License

Apache-2.0.
