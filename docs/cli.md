# Gaze CLI

The canonical CLI reference is [`crates/gaze-cli/README.md`](../crates/gaze-cli/README.md). It documents
every subcommand, flag, exit code, and feature gate exposed by the `gaze`
binary. This page is a short index plus a few adopter-facing walk-throughs that
are not covered there.

## Subcommands

Every verb below is defined by the clap `Subcommand` enum in
[`crates/gaze-cli/src/commands/mod.rs`](../crates/gaze-cli/src/commands/mod.rs).

| Subcommand | One-line summary | Feature gate |
|------------|------------------|--------------|
| [`gaze clean`](../crates/gaze-cli/README.md#clean) | Read raw text from stdin; emit `{clean_text, session_blob, stats}` JSON. | always |
| `gaze daemon` | Run a long-lived JSONL stdio cleaner with one process-level pipeline and per-`session_id` manifests. See [guide](#gaze-daemon). | always |
| [`gaze restore`](../crates/gaze-cli/README.md#restore) | Read `{session_blob, text}` JSON from stdin; emit restored `{text}` JSON. | always |
| [`gaze audit query`](../crates/gaze-cli/README.md#audit-query) | Print filtered redaction-log metadata rows as TSV from a read-only SQLite DB. | always |
| [`gaze audit export`](../crates/gaze-cli/README.md#audit-export) | Export filtered redaction-log metadata rows as JSONL for downstream processing. | always |
| `gaze audit purge` | Manually delete redaction-log metadata rows older than an ISO 8601 UTC timestamp. See [guide](#gaze-audit-purge). | always |
| [`gaze audit safety-net query`](../crates/gaze-cli/README.md#audit-safety-net-query) | Print filtered `safety_net_log` rows as TSV. See [guide](#gaze-audit-safety-net-query). | always |
| `gaze document clean` | OCR a PNG/JPG/PDF into a `SafeBundle` (`clean.md` + `manifest.json` + `report.json`). See [guide](#gaze-document-clean) and [crate README](../crates/gaze-document/README.md). | `document` |
| `gaze mcp install` | Install `gaze mcp serve` into a supported MCP client config. See [guide](#gaze-mcp-install--doctor--serve) and [crate README](../crates/gaze-cli/README.md#mcp-installation). | `mcp` |
| `gaze mcp doctor` | Diagnose MCP runtime dependencies, client config, and AGENTS.md guidance. See [guide](#gaze-mcp-install--doctor--serve) and [crate README](../crates/gaze-cli/README.md#mcp-installation). | `mcp` |
| `gaze mcp serve` | Run the stdio MCP server exposing agent-tier document tools. See [guide](#gaze-mcp-install--doctor--serve) and [crate README](../crates/gaze-cli/README.md#mcp-installation). | `mcp` |
| `gaze proxy` | Run or manage the multi-provider HTTP chokepoint daemon. See [guide](#gaze-proxy) and [crate README](../crates/gaze-proxy/README.md). | built into the default release binary since v0.8.1 |

For exit codes and stderr error JSON, see
[Exit codes](../crates/gaze-cli/README.md#exit-codes) in the crate README.
For policy schema details, see [`docs/policy.md`](policy.md).

## Guides

### `gaze proxy`

`gaze proxy` is the multi-provider HTTP chokepoint daemon for SDK and agent
traffic that authenticates with provider API keys. It preserves each provider's
native request and response shape while redacting request PII, restoring
owner-visible response text, and accumulating SSE streams and tool-call JSON
arguments chunk-by-chunk before the text crosses the model boundary. The proxy
is built into the default release binary as of v0.8.1.

```sh
gaze proxy start --policy ./policy.toml
gaze proxy status
gaze proxy logs --follow
gaze proxy restart
gaze proxy stop
```

Supported management verbs are `serve`, `start`, `stop`, `status`, `logs`, and
`restart`. `install-launchd` and `install-systemd-user` are present as opt-in
supervisor hooks, but currently return a typed message directing adopters to
`gaze proxy start` / `gaze proxy stop`.

The provider adapters claim these API-key-authenticated endpoints:

- OpenAI: `POST /v1/chat/completions`
- Anthropic: `POST /v1/messages`
- Gemini: `POST /v1beta/models/*:{generateContent,streamGenerateContent}`

Consumer subscription tiers such as ChatGPT Plus, Claude.ai, and Gemini
Advanced use cookie-authenticated web endpoints and are out of scope for this
proxy.

| Option | Purpose |
|--------|---------|
| `--bind <addr>` | Listener address. Default for `serve`: `127.0.0.1:8787`; `start` uses the persisted config unless overridden. |
| `--policy <path>` | Optional policy TOML. When omitted, the built-in core rulepack is used. |
| `--rulepack <name>` | Bundled rulepack name. Default for `serve`: `core`; `start` persists the override. |
| `--session-ttl <duration>` | In-memory session retention such as `30m`, `10s`, or `1h`. Default for `serve`: `30m`. |
| `--upstream-openai <url>` | OpenAI upstream. Default: `https://api.openai.com`. |
| `--upstream-anthropic <url>` | Anthropic upstream. Default: `https://api.anthropic.com`. |
| `--upstream-gemini <url>` | Gemini upstream. Default: `https://generativelanguage.googleapis.com`. |
| `--force` | For `stop` / `restart`, send the hard stop after the bounded wait. |
| `--timeout <duration>` | Stop / restart wait before force. Default: `10s`. |

SafetyNet activation follows the normal policy and CLI behavior: use policy
configuration for the deterministic floor and activate observer-only safety-net
backends with the same `safety_tier` posture used by the pipeline. Locale
coverage comes from the policy, merged bundled rulepack defaults, and active
locale chain.

See [`docs/architecture/proxy-runtime.md`](architecture/proxy-runtime.md) for
the adapter and daemon contract. See
[`crates/gaze-proxy/README.md`](../crates/gaze-proxy/README.md) for the crate
README.

### `gaze mcp install / doctor / serve`

`gaze mcp install`, `gaze mcp doctor`, and `gaze mcp serve` surface the MCP
chokepoint for adopters whose agent hosts already speak MCP. `install` writes a
supported client config that points at the absolute `current_exe()` path with
`["mcp", "serve"]`, then creates or updates an idempotent marker-fenced
`AGENTS.md` guidance section. `doctor` checks runtime dependencies, client
config, manifest storage, and the AGENTS.md marker. `serve` runs the stdio MCP
server and exposes the agent-tier `gaze_read_file` and `gaze_read_text` tool
implementations.

```sh
cargo install gaze-cli --features mcp,document
gaze mcp install --client=claude-code
gaze mcp doctor
gaze mcp serve
```

The CLI verbs require the `mcp` feature. Document tools require the `document`
feature as well, because the tool implementations live in `gaze-document`.

| Option | Purpose |
|--------|---------|
| `install --client <client>` | Supported values: `claude-code`, `claude-desktop`, `cursor`, `all`. |
| `install --agents-md <path>` | AGENTS.md path to create or update. Default: `./AGENTS.md`. |
| `install --dry-run` | Print the planned install summary without writing files. |
| `install --skip-agents-md` | Update client config only. |
| `doctor --agents-md <path>` | AGENTS.md path to inspect. Default: `./AGENTS.md`. |
| `doctor --strict` | Exit non-zero when any warning is present. |
| `doctor --json` | Emit machine-readable diagnostic JSON. |
| `serve --manifest-dir <path>` | Directory where MCP call manifest records are written. |
| `serve --max-file-size <bytes>` | Maximum file size accepted by `gaze_read_file`. |

`gaze_read_text` accepts already-extracted text. `gaze_read_file` accepts a
PNG, JPG, or PDF path and returns safe content through the same
`PiiEnvelope::dispatch` ordering as custom tools. Responses include
`clean_markdown`, `manifest_id`, and `file_metadata` so the agent can use the
safe Markdown while the owner retains restore material.

See [`docs/architecture/mcp-runtime.md`](architecture/mcp-runtime.md) for the
runtime contract.

### `gaze audit safety-net query`

`gaze audit safety-net query` prints filtered TSV rows from the
`safety_net_log` table. It is the SafetyNet companion to the redaction-log audit
query verbs: the deterministic redaction audit table records emitted token
metadata, while `safety_net_log` records observer-only leak suspects after the
clean text and manifest already exist.

```sh
gaze audit safety-net query --audit-db .gaze/audit.sqlite --leak-kind uncovered --mapped-class email
```

| Option | Purpose |
|--------|---------|
| `--audit-db <path>` | SQLite redaction-log database path. |
| `--leak-kind <kind>` | Filter by typed leak classification. |
| `--raw-label <label>` | Filter by backend-native label. |
| `--mapped-class <class>` | Filter by mapped Gaze PII class. |
| `--field-path <path>` | Filter by structured field path. |
| `--from <iso8601>` | Include rows created at or after this timestamp. |
| `--to <iso8601>` | Include rows created at or before this timestamp. |

The leak-kind filter corresponds to the closed `LeakKind` set:
`Uncovered`, `PartialBleed`, and `ClassMismatch`. The TSV values are the
lowercase wire forms `uncovered`, `partial_bleed`, and `class_mismatch`.

See
[`crates/gaze-cli/README.md#audit-safety-net-query`](../crates/gaze-cli/README.md#audit-safety-net-query)
for the crate README reference.

### `gaze daemon`

`gaze daemon` is the long-lived stdio runtime for adapters that need repeated
low-latency redaction without paying binary startup and model-load cost on every
request. It builds one pipeline from `--policy`, keeps it hot across JSONL
requests, and isolates manifests per client-provided `session_id` so multi-turn
agent sessions do not share restore material.

The wire format is one JSON request per stdin line and one JSON response per
stdout line.

Request:

```json
{"session_id":"conversation-1","text":"input text"}
```

Success:

```json
{"session_id":"conversation-1","clean_text":"output text","manifest":[],"tokens":[]}
```

Error:

```json
{"session_id":"conversation-1","error":"Pipeline","detail":"gaze daemon request failed closed"}
```

Malformed JSON fails closed per line as `JsonMalformed` with `session_id: null`.
Errors never echo the input text.

| Flag | Purpose |
|------|---------|
| `--policy <path>` | Required policy TOML loaded once at daemon startup. |
| `--session-cap <N>` | Maximum live sessions before LRU eviction. Default: `1000`. |
| `--session-idle-timeout <secs>` | Evict sessions idle for this many seconds. Default: `3600`. |
| `--idle-timeout <secs>` | Exit the process after stdin inactivity for this many seconds. Default: `1800`. |

The default values above are also documented in
[`crates/gaze-cli/README.md`](../crates/gaze-cli/README.md#daemon-mode).

SIGINT and SIGTERM set a shutdown flag; the daemon finishes the current line,
flushes stdout and audit writes, then exits. The session registry evicts by LRU
when `--session-cap` is exceeded and by idle timeout when a session is quiet too
long. Each `session_id` owns its own manifest, and eviction emits audit metadata
with source `daemon.session_eviction`.

Daemon redaction audit rows are stamped with
`provenance_stage = "daemon"`, which lets adopters filter daemon-emitted rows
separately from one-shot `gaze clean` rows.

Five-axis check:

- Reliability: malformed protocol input produces typed JSON errors and the
  daemon keeps reading.
- Reversibility: restore material is scoped to one `session_id` manifest.
- Agentic-first: JSONL over one stdio process fits multi-turn adapter loops.
- Trust: daemon audit rows carry explicit provenance and eviction metadata.
- Adopter ergonomics: adapters can keep one process hot and avoid per-call cold
  starts.

See [`docs/architecture/daemon-mode.md`](architecture/daemon-mode.md) for the
full contract. See
[getting-started/daemon-adapter.md](getting-started/daemon-adapter.md) for an
adopter quickstart.

### `gaze audit purge`

`gaze audit purge` manually removes redaction-log metadata rows older than an
ISO 8601 UTC timestamp. It never touches session manifests and does not run in
the background.

```sh
gaze audit purge --audit-db .gaze/audit.sqlite --before 2026-04-01T00:00:00Z --dry-run
gaze audit purge --audit-db .gaze/audit.sqlite --before 2026-04-01T00:00:00Z --count
gaze audit purge --audit-db .gaze/audit.sqlite --before 2026-04-01T00:00:00Z
```

`--count` is an alias for `--dry-run`; both flags count matching rows without
deleting them.

Successful output is JSON on stdout:

```json
{"dry_run":true,"matched":12,"deleted":0}
```

Invalid `--before` values fail closed with a typed JSON error that quotes the
input:

```json
{"error":"AuditPurgeIso8601","exit":2,"input":"not-iso8601"}
```

### `gaze document clean`

`gaze document clean` is the OSS document ingestion verb. It OCRs the input
through Tesseract, redacts the recognized text through the standard Gaze
pipeline, and writes a `SafeBundle` to `--out`: `clean.md` (tokenized text),
`manifest.json` (restore mapping), and `report.json` (per-detection metadata).
Requires the binary to be built with `--features document`, and the host must
have `tesseract` on PATH plus the pdfium runtime for PDF input.

```sh
cargo install gaze-cli --features document
gaze document clean ./invoice.pdf --out ./safe-bundle/
```

The supported inputs are `.png`, `.jpg`, `.jpeg`, and single-page `.pdf`. The
`BundleReport` schema is versioned via `bundle_version = 1`. See the
`gaze-document` crate for the full bundle contract.

### Legacy audit databases

Audit databases written before v0.4.4 lack a `created_at` column. Unfiltered
`gaze audit query` calls still surface those rows. Filtered queries that use
`--from` or `--to` omit NULL `created_at` rows by SQL semantics; drop the time
filter to access legacy rows.
