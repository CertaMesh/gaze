# gaze-cli

[![Crates.io](https://img.shields.io/crates/v/gaze-cli.svg)](https://crates.io/crates/gaze-cli)
[![docs.rs](https://docs.rs/gaze-cli/badge.svg)](https://docs.rs/gaze-cli)
[![License](https://img.shields.io/crates/l/gaze-cli.svg)](https://github.com/CertaMesh/gaze#license)

Gaze command-line interface

Part of the [Gaze](https://github.com/CertaMesh/gaze) workspace — a reversible PII pseudonymization runtime for agentic LLM workflows.

This crate publishes the `gaze` binary. It is the process boundary used by
shell integrations and language adapters that should not link the Rust library
directly.

The CLI reads from stdin, writes JSON to stdout, and emits sanitized structured
errors to stderr. Panic handling is overridden so dependency panics do not dump
raw input or backtraces into caller logs.

## Cargo

Install from crates.io:

```console
$ cargo install gaze-cli --version 0.11.3
```

Build from the workspace root:

```console
$ cargo build -p gaze-cli
```

The default build includes `gaze proxy` for local LLM API proxy use.

Build with the MCP installer/server surface:

```console
$ cargo build -p gaze-cli --features mcp
```

Run from the workspace root:

```console
$ cargo run -p gaze-cli -- clean --policy policy.toml
```

The installed binary name is `gaze`.

## Subcommands

Current subcommands in [`src/commands/mod.rs`](src/commands/mod.rs):

| Subcommand | Purpose |
|------------|---------|
| `clean` | Reads raw UTF-8 text from stdin and emits `{"clean_text","session_blob","stats"}` JSON. |
| `daemon` | Runs a long-lived JSONL stdio cleaner with one process-level pipeline and per-`session_id` manifests. |
| `restore` | Reads `{"session_blob","text"}` JSON from stdin and emits restored `{"text"}` JSON, plus `restore_warning` when tolerant restore allows an unknown token. |
| `audit query` | Prints filtered audit metadata rows from a `--audit-db` SQLite log, opened read-only. |
| `audit export` | Exports filtered audit metadata rows in JSONL (default) for downstream processing. |
| `document clean` | OCRs PNG/JPG/PDF input into a SafeBundle. Requires `--features document`. |
| `index ingest/search` | Builds and searches a local owner-side text/Markdown index. Requires `--features index`. |
| `mcp install` | Installs `gaze mcp serve` into supported MCP client configs. Requires `--features mcp`. |
| `mcp doctor` | Diagnoses MCP runtime dependencies, client config, and AGENTS.md guidance. Requires `--features mcp`. |
| `mcp serve` | Runs the stdio MCP server exposing `gaze_read_file` and `gaze_read_text`. Requires `--features mcp`. |
| `proxy serve/start/stop/status/restart` | Runs or manages the local LLM API proxy. Included in the default build. |

Audit logging is captured on `clean` via `--audit-db <path>`; the
`audit query` and `audit export` subcommands read the same database back.

## Daemon mode

`gaze daemon` is a stdio server in the LSP / MCP tradition, not a Unix daemon
in the strict sense. The subcommand verb is preserved for binary stability; see
[`docs/explanation/daemon/daemon-mode.md`](../../docs/explanation/daemon/daemon-mode.md)
for the terminology note.

`gaze daemon --policy policy.toml` keeps one pipeline alive and reads one JSON
request per stdin line:

```json
{"session_id":"conversation-1","text":"Contact alice@example.invalid"}
```

Each stdout line is either a clean response:

```json
{"session_id":"conversation-1","clean_text":"Contact <...:Email_1>","manifest":[],"tokens":[]}
```

or a typed protocol/cleaning error:

```json
{"session_id":null,"error":"JsonMalformed","detail":"malformed JSON line"}
```

Sessions are isolated by `session_id`, evicted by LRU after `--session-cap`
(default 1000), and evicted after `--session-idle-timeout` seconds (default
3600). `--idle-timeout` exits the process after stdin inactivity (default 1800).
SIGINT and SIGTERM finish the current line, flush stdout/audit writes, and exit.
Daemon audit rows are stamped with `provenance_stage = "daemon"`.

For end-to-end adapter examples, see
[`docs/how-to/daemon/run-daemon.md`](../../docs/how-to/daemon/run-daemon.md).
For the full runtime contract, see
[`docs/explanation/daemon/daemon-mode.md`](../../docs/explanation/daemon/daemon-mode.md).

## MCP installation

The `mcp` feature embeds the rmcp stdio server into the `gaze` binary and
registers `gaze-document` tools:

```console
$ cargo install gaze-cli --version 0.11.3 --features mcp
$ gaze mcp install --client=claude-code
$ gaze mcp doctor
```

`install` always writes the absolute `std::env::current_exe()` path into the
client config:

```json
{
  "mcpServers": {
    "gaze": {
      "command": "/absolute/path/to/gaze",
      "args": ["mcp", "serve"],
      "env": {}
    }
  }
}
```

Supported clients:

| Client | Default config path |
|--------|---------------------|
| `claude-code` | `./.mcp.json` in the current project. |
| `claude-desktop` | macOS `~/Library/Application Support/Claude/claude_desktop_config.json`; Windows `%APPDATA%\Claude\claude_desktop_config.json`; Linux config dir fallback. |
| `cursor` | `./.cursor/mcp.json` in the current project. |
| `all` | Updates all supported client paths. |

Use `--agents-md <path>` to choose where the marker-fenced Gaze MCP guidance
section is written. By default, `install` updates `./AGENTS.md`. Use
`--skip-agents-md` to update only client JSON, and `--dry-run` to preview
without writing.

`doctor` checks whether the current binary matches client config entries,
whether `tesseract` and pdfium are available for `gaze_read_file`, whether the
manifest directory is writable, and whether the AGENTS.md guidance section is
present. Warnings do not fail by default; `--strict` exits non-zero on warnings.
Use `--json` for machine-readable output.

`serve` runs the stdio MCP server:

```console
$ gaze mcp serve --manifest-dir ~/.local/share/gaze/mcp-manifests --max-file-size 26214400
```

The server covers the data-source to model path only. It does not filter text
the user pastes directly into a chat UI.

## `index`

The `index` feature adds a local owner-side search index for `.txt` and `.md`
corpora:

```console
$ cargo run -p gaze-cli --features index -- index ingest ./notes
$ cargo run -p gaze-cli --features index -- index search "alice@example.invalid" --class email
```

By default the index is written under `./.gaze-index/`, or the directory from
`GAZE_INDEX_PATH`; `--index-path <dir>` overrides both. This store is sensitive
owner-side material: it contains raw values needed for session-token translation,
plus generated projection key material. The index file is encrypted at rest with
ChaCha20-Poly1305. Default key resolution is fail-closed and headless-friendly:
`GAZE_INDEX_KEY` must contain a 32-byte key as 64 hex characters. Building with
`--features os-keychain` adds OS keychain fallback for desktop use: macOS
Keychain, Windows Credential Manager, or Linux kernel keyutils (no dbus). If no
key source is available, `gaze index` refuses to read or write the index, and
legacy plaintext `index.json` files are not loaded.

Search output is the bridge's agent-facing response: snippets contain only
current-session tokens, never raw PII or index-domain aliases. The v1 ingest path
is text/Markdown only; OCR/PDF/image ingestion stays with `gaze document`.

## `clean`

```console
$ printf '%s' 'Email alice@example.invalid now' \
  | gaze clean --policy policy.toml
```

Flags:

| Flag | Meaning |
|------|---------|
| `--policy <path>` | Optional `policy.toml` path. Production integrations should pass one. |
| `--format <json>` | Output format. Only `json` is accepted. Defaults to `json`. |
| `--session-ttl <secs>` | Override persistent session TTL from policy. |
| `--session-scope <scope>` | Override `[session].scope` from policy. |
| `--locale <tag[,tag...]>` | Active locale fallback chain, comma separated and priority ordered. |
| `--ner-threshold <float>` | Override policy `[ner]` threshold. Must be between `0.0` and `1.0` inclusive. |
| `--ner-model-dir <path>` | Override `[ner].model_dir` from policy. |
| `--ner-locale <tag>` | Override `[ner].locale` from policy. |
| `--rulepack-bundled <name[,name...]>` | Override `[policy.rulepacks].bundled`. Comma separated. `core-extended` is deprecated since v0.8.0; use `core --locale=<lang>` for explicit locale-gated activation. |
| `--rulepack-path <path>` | Override `[policy.rulepacks].paths`. Repeatable. |
| `--max-bytes <bytes>` | Stdin byte cap. Defaults to `10485760`. |
| `--context-json <path>` | Typed context envelope with dictionaries, class map, and fields. |
| `--audit-db <path>` | Optional SQLite redaction-log database path for metadata-only audit entries. |
| `--safety-net <kind>` | Optional observer-only safety net. Accepts `openai-filter` (v0.6+) or `kiji-distilbert` (v0.8+). Activates the post-clean leak audit. |
| `--safety-net-backend <backend>` | v0.8 single-backend selector: `openai-filter` or `kiji-distilbert`. When set alongside `--safety-net=<kind>`, this flag wins and lets adopters swap the Pass-3 implementation without re-typing the legacy `--safety-net` value. Cannot be combined with `--safety-net-registry`. |
| `--safety-net-registry` | Enables locale-aware Pass-3 dispatch through `LocaleAwareModelRegistry`. Requires one or more `--safety-net-add` flags. |
| `--safety-net-add <backend>` | Adds one backend to the registry. Repeatable. First resolved backend wins for v1. |
| `--openai-filter-command <path>` | Path to the local OpenAI Privacy Filter `opf` command. Required with the `openai-filter` backend. |
| `--openai-filter-checkpoint <path>` | Path to the OPF checkpoint or model directory. Required with the `openai-filter` backend. |
| `--kiji-backend <backend>` | Kiji runtime backend: `subprocess` (default, compatibility path) or `ort` (in-process ONNX Runtime). |
| `--opf-command <path>` / `--opf-checkpoint <path>` | Registry-example aliases for the OpenAI Privacy Filter command and checkpoint. |
| `--opf-locales <tag[,tag...]>` | Native locales for the OpenAI Privacy Filter registry entry. Empty keeps the backend default. |
| `--kiji-distilbert-command <path>` | Path to the local Kiji DistilBERT subprocess command. Required with the `kiji-distilbert` backend. |
| `--kiji-distilbert-model-dir <path>` | Path to the pinned Kiji DistilBERT model directory (must contain `SHA256SUMS`, `labels.json`, `model.onnx`, `tokenizer.json`). Required with the `kiji-distilbert` backend. |
| `--kiji-distilbert-locales <tag[,tag...]>` | Native locales for the Kiji DistilBERT registry entry. Empty keeps the backend default. |
| `--safety-net-timeout-ms <ms>` | Subprocess deadline. Defaults to `5000`. |
| `--safety-net-input-limit-bytes <bytes>` | Clean-text input cap forwarded to the safety net. Defaults to `1048576`. |
| `--safety-net-mode <strict\|tolerant\|redact\|resolve>` | Production action on `Uncovered`/`PartialBleed` suspects. `strict` exits `3`; `tolerant` emits warnings on stderr and continues (dev-only, fires a stderr warning on every invocation); `redact` overwrites the suspect span with a sentinel and records an audit row; `resolve` promotes the suspect into a synthetic custom-recognizer match and re-runs the resolver. Defaults to `resolve`. Mode catalog and posture guide: [`docs/explanation/safety-net/safety-net-modes.md`](../../docs/explanation/safety-net/safety-net-modes.md). |
| `--safety-net-fallback <strict\|tolerant\|redact>` | Cascade action when `--safety-net-mode` is `redact` or `resolve` and the primary action cannot be honored for a specific suspect (manifest overlap or grapheme-cluster break for `redact`; validator-veto, missing mandatory anchor, or residual suspect after the one-shot resolve pass for `resolve`). Ignored when `--safety-net-mode` is `strict` or `tolerant`. Defaults to `redact`. One-hop cascade only. `tolerant` requires `GAZE_ALLOW_TOLERANT=1`. Composition matrix and audit-row delta: [`docs/explanation/safety-net/safety-net-modes.md`](../../docs/explanation/safety-net/safety-net-modes.md#6-fallback-flag). |
| `--safety-net-resolve-threshold <float>` | Confidence threshold for `--safety-net-mode resolve`. Suspects below threshold are dropped before candidate construction. Defaults to `0.7`. `0.0` disables filtering; `1.0` disables resolve entirely. |

When `--policy` is omitted, the CLI runs a stub email pipeline so the process
surface can be exercised. Production use should pass `--policy`.

### Safety net

The optional `--safety-net=<kind>` flag activates the observer-only safety
net documented in
[docs/explanation/safety-net/safety-nets.md](../../docs/explanation/safety-net/safety-nets.md).
The safety net runs after the deterministic clean and reports suspected
leaks against the manifest of emitted tokens. It cannot mutate the clean
text and cannot affect restore.

#### Safety-net backends

Two observer-only backends are available; pick one via
`--safety-net-backend <backend>` (v0.8) or the legacy `--safety-net=<kind>`.
Both share the strict/tolerant exit-code contract, the `LeakReport` shape,
and the `safety_net_log` audit table.

**`openai-filter`** (v0.6+) wraps the official `openai/privacy-filter`
subprocess. Strengths: eight typed labels covering Person, Email, Phone,
URL, Address, Date, Account number, Secret; documented operating points;
mature upstream. Trade-offs: heavier model and slower per-clean latency;
no first-party fetch path; runtime depends on a third-party Python install
the operator pins.

**`kiji-distilbert`** (v0.8+) wraps a pinned DistilBERT NER model. It
supports `--kiji-backend=subprocess` for the existing external command path
and `--kiji-backend=ort` for in-process ONNX Runtime inference without a
Python install. Strengths: lightweight ONNX-served weights, straightforward
fetch script, second NER opinion at the chokepoint that complements OPF's
class set. Trade-offs: narrower closed label set
(`person`, `location`, `organization`, `miscellaneous`) so financial-secret
or account-number suspects are not surfaced; pinned-artifact contract
requires `SHA256SUMS` present on disk (Axis-1 fail-closed — no silent
disable). The default remains `subprocess` for backwards compatibility.

#### Setup

The safety-net features are gated off by default. Build with one or both:

```console
$ cargo build -p gaze-cli --features safety-net-openai
$ cargo build -p gaze-cli --features safety-net-kiji
$ cargo build -p gaze-cli --features safety-net-openai,safety-net-kiji
```

The `opf` command must be installed from a pinned upstream Git revision or
an official release of the
[`openai/privacy-filter`](https://github.com/openai/privacy-filter) repository.
Adopters should record the exact upstream Git SHA or tag they install in
their deployment manifest. The adapter does **not** download or update the
checkpoint; bring-your-own-binary plus bring-your-own-weights is the
v0.6 contract.

Pin the install path with `GAZE_OPENAI_FILTER_OPF=/opt/opf/bin/opf` or pass
`--openai-filter-command=<path>` per invocation. The command path must be a
regular file (not a symlink) when given as an absolute path, and the
checkpoint directory must be owned by the current user with mode `0700` and
no group/world write bits.

If the checkpoint is missing, the CLI fails closed with exit `3` and
variant `WeightsMissing` before any subprocess spawn. Initialization
failures are cached for the lifetime of the process so missing-checkpoint
errors do not retry on every clean.

The Kiji DistilBERT backend follows the same bring-your-own pattern. The
model directory must contain `SHA256SUMS`, `labels.json`, `model.onnx`,
and `tokenizer.json`; populate it via `scripts/fetch/fetch-kiji-safetynet-model.sh`.
A missing artifact (including a missing `SHA256SUMS`) fails closed with the
typed `SafetyNetArtifactMissing` envelope and exit code `2` *before* the
subprocess is spawned — Axis-1 reliability never silent-disables a
requested backend.

See also: [Kiji DistilBERT SafetyNet setup](../../docs/how-to/safety-net/set-up-kiji-safetynet.md).

#### Synthetic example — strict mode

```console
$ printf '%s' 'Email alice@example.invalid or call 555-0100 now' \
  | gaze clean \
      --policy=policy.toml \
      --safety-net=openai-filter \
      --safety-net-mode=strict \
      --openai-filter-command=/opt/opf/bin/opf \
      --openai-filter-checkpoint=/opt/opf/checkpoint
```

A clean run emits the standard `{clean_text, session_blob, stats}` JSON
plus a `leak_report` block on stdout:

```json
{
  "clean_text": "Email <{session_hex}:Email_1> or call <{session_hex}:Phone_1> now",
  "session_blob": "<base64>",
  "stats": {"detections": 2},
  "leak_report": {
    "stats": {
      "suspect_count": 0,
      "uncovered_count": 0,
      "partial_bleed_count": 0,
      "class_mismatch_count": 0,
      "locale_skipped_count": 0
    }
  }
}
```

Exit code `0` and `suspect_count = 0` is the contract for "no leaks".

#### Synthetic example — tolerant mode

```console
$ printf '%s' 'Sender: Bob Example, phone +44 113 496 0123' \
  | gaze clean \
      --policy=policy.toml \
      --safety-net=openai-filter \
      --openai-filter-command=/opt/opf/bin/opf \
      --openai-filter-checkpoint=/opt/opf/checkpoint \
      --safety-net-mode=tolerant
```

If the safety net reports an `Uncovered` or `PartialBleed` suspect that the
deterministic pipeline missed, tolerant mode emits a stderr warning and
exits `0`:

```text
{"warning":"SafetyNet","variant":"SuspectedLeak","count":1}
```

Strict mode (the v0.7.x default; now opt-in via `--safety-net-mode strict`)
would exit `3` with the JSON error
`{"error":"SafetyNet","exit":3,"variant":"SuspectedLeak"}` and stdout would
be empty. `ClassMismatch` suspects always warn but never fail strict mode,
because the manifest still tokenized the bytes — only the class disagrees.
The default mode in v0.8.x+ is `resolve` with a `redact` fallback (see the
flag table above and the
[mode catalog](../../docs/explanation/safety-net/safety-net-modes.md)).

#### Synthetic example — Kiji DistilBERT backend

```console
$ printf '%s' 'Alice Example mailed the package to Berlin' \
  | gaze clean \
      --policy=policy.toml \
      --safety-net=kiji-distilbert \
      --safety-net-backend=kiji-distilbert \
      --kiji-distilbert-command=/opt/kiji/bin/kiji \
      --kiji-distilbert-model-dir=~/.local/share/gaze/models/kiji-distilbert
```

The `--safety-net-backend` flag overrides any legacy `--safety-net=<kind>`
value, so a deployment can keep its existing `--safety-net=openai-filter`
invocation and flip to Kiji by adding one flag. Suspects emit the same
`LeakReport` shape; the `safety_net_id` field switches to
`kiji-distilbert`.

#### Approved synthetic PII

All examples in this README use project-approved synthetic fixtures so the
fixture-citation and no-tenant-knowledge gates remain green:

- Emails: `<local>@example.invalid`, `*.invalid`, `*.test`. RFC 6761
  guarantees these never resolve.
- US/CA phones: NANPA `555-01xx` range (`555-0100` through `555-0199`),
  reserved by the FCC for fictional use.
- UK phones: Ofcom drama ranges (e.g. `+44 113 496 0xxx`), reserved by
  Ofcom for fictional use.
- Names: `Alice Example`, `Bob Example`. Avoid real public-figure names.

Do not paste real customer or operator data into examples or fixtures —
the `fixture-citation-lint` xtask gate will reject any literal that looks
real or that is not cited from a checked-in test.

#### Latency budget

Each safety-net check spawns one `opf` subprocess. The default subprocess
deadline is `5000` ms; tighten it via `--safety-net-timeout-ms` for
latency-sensitive callers. On timeout the adapter sends `SIGKILL`, reaps
the process, and returns exit `3` with variant `Timeout`. The safety net
does not currently amortize subprocess startup across calls; a long-lived
helper is filed for post-v0.6.0 (todo #303).

#### Audit

Combine the safety net with `--audit-db <path>` to persist metadata-only
suspect rows into the `safety_net_log` table. Query the rows back with
`gaze audit safety-net query` (see below). The schema and the bytes-free
invariants are documented in
[`docs/explanation/safety-net/safety-nets.md`](../../docs/explanation/safety-net/safety-nets.md#safety_net_log-audit-table).

## `restore`

```console
$ printf '%s' '{"session_blob":"<base64>","text":"Email <token> now"}' \
  | gaze restore
```

Flags:

| Flag | Meaning |
|------|---------|
| `--format <json>` | Output format. Only `json` is accepted. Defaults to `json`. |
| `--restore-mode <strict\|tolerant>` | Unknown-token handling. Defaults to `strict`. |
| `--max-bytes <bytes>` | Stdin byte cap. Defaults to `10485760`. |

`strict` restore fails on unknown tokens. `tolerant` restore preserves unknown
tokens and returns a warning in the JSON response.

## `audit query`

Reads the SQLite redaction log written by `gaze clean --audit-db <path>` and
prints filtered metadata rows as tab-separated values. The DB is opened
read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY`, so the audit CLI cannot write
back to the log even if compromised.

```console
$ gaze audit query --audit-db audit.sqlite --class email --action tokenize
```

Filters:

| Flag | Meaning |
|------|---------|
| `--audit-db <path>` | Required. SQLite redaction-log database path. |
| `--class <pii_class>` | Filter by PII class such as `email`, `name`, or `custom:term`. |
| `--source <name>` | Filter by source recognizer name. |
| `--action <kind>` | Filter by action: `tokenize`, `redact`, `preserve`. |
| `--document-kind <kind>` | Filter by document kind: `text`, `structured`. |
| `--from <iso8601>` | Include rows whose `created_at` is at or after this timestamp (v0.4.4). |
| `--to <iso8601>` | Include rows whose `created_at` is at or before this timestamp (v0.4.4). |

Time-filtered queries omit NULL `created_at` rows from legacy v0.4.3 audit DBs
by SQL semantics. Unfiltered queries still surface those rows.

## `audit export`

Same filter set as `audit query`, with output destined for downstream
processing rather than the terminal:

```console
$ gaze audit export --audit-db audit.sqlite --format jsonl --output redactions.jsonl
```

| Flag | Meaning |
|------|---------|
| `--format <jsonl>` | Export format. JSONL is the default and currently the only supported format. |
| `--output <path>` | Optional output file. Defaults to stdout. |

Exported JSON rows include `created_at` since v0.4.4. The export ships a
restricted column set so raw PII payloads stay outside the export surface.

## `audit safety-net query`

Reads the `safety_net_log` rows written by `gaze clean --audit-db <path>
--safety-net <kind>` and prints them as tab-separated values. The DB is
opened read-only.

```console
$ gaze audit safety-net query \
    --audit-db audit.sqlite \
    --leak-kind uncovered \
    --field-path '$.user.email'
```

Filters:

| Flag | Meaning |
|------|---------|
| `--audit-db <path>` | Required. SQLite redaction-log database path. |
| `--leak-kind <kind>` | Filter by `uncovered`, `partial_bleed`, or `class_mismatch`. |
| `--raw-label <label>` | Filter by validated upstream label, e.g. `private_email`. |
| `--mapped-class <pii_class>` | Filter by Gaze class produced by the class map. |
| `--field-path <selector>` | Filter by structured-document field path, e.g. `$.user.email`. |
| `--from <iso8601>` | Include rows whose `created_at` is at or after this timestamp. |
| `--to <iso8601>` | Include rows whose `created_at` is at or before this timestamp. |

The `safety_net_log` table stores metadata only — `raw_label` is the
validated upstream label, **not** the upstream raw text. See
[`docs/explanation/safety-net/safety-nets.md`](../../docs/explanation/safety-net/safety-nets.md#safety_net_log-audit-table)
for the full schema.

## Exit codes

Exit codes are defined by `CliError` in [`src/error.rs`](src/error.rs).

| Exit | Variants |
|------|----------|
| `0` | Success, help, version output, or tolerant-mode safety-net runs that produced only stderr warnings. |
| `1` | `StdinParse`, `EmptyInput`, `InputTooLarge`, `InvalidEncoding`. |
| `2` | `PolicyConfig`, including unsupported format, invalid policy, invalid locale, invalid NER threshold, unknown rulepack, unsupported CLI column rules, `SafetyNetConfig` (missing backend command/checkpoint or safety-net flags supplied without the matching feature), or `SafetyNetArtifactMissing` (Axis-1 fail-closed when a backend's pinned artifact is absent, including a missing `SHA256SUMS` for the Kiji DistilBERT backend). |
| `3` | `UnknownToken`, `InvalidSignature`, `InvalidBlobVersion`, `BlobExpired`, `Pipeline`, sanitized panic path, and `SafetyNetFailure` variants: `Unavailable`, `WeightsMissing`, `ModelUnavailable`, `InputTooLarge`, `Timeout`, `Runtime`, `InvalidOutput`, `SuspectedLeak` (strict mode only). |
| `4` | `Io`, `PolicyOpen`. |

Safety-net summary: exit `3` means the safety net (or strict mode) closed
the door; exit `0` with no `leak_report.stats.suspect_count` means a clean
run; exit `0` plus stderr `{"warning":"SafetyNet",...}` means tolerant
mode reported suspects without blocking.

Stderr is JSON with the error variant and exit code, for example:

```json
{"error":"PolicyConfig","exit":2}
```

`UnknownToken` includes the unknown token string because the token is already a
pseudonym emitted by Gaze, not raw PII.

Full exit-code catalog (including `5` document, `6` mcp, `7` proxy feature
codes) and the stability guarantee for each variant:
[`docs/reference/metrics.md`](../../docs/reference/metrics.md#8-cli-exit-codes-gaze-cli).

## Policy path

`clean --policy <path>` loads the TOML policy through `gaze::Policy`, loads
bundled/path rulepacks, resolves locale precedence, builds a pipeline with
`gaze-assembly`, then exports the session as `session_blob`.

For policy schema details, see [docs/reference/policy.md](../../docs/reference/policy.md).
