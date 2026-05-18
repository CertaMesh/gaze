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
| `gaze daemon` | Run a long-lived JSONL stdio cleaner with one process-level pipeline and per-`session_id` manifests. See [Guides](#gaze-daemon). | always |
| [`gaze restore`](../crates/gaze-cli/README.md#restore) | Read `{session_blob, text}` JSON from stdin; emit restored `{text}` JSON. | always |
| [`gaze audit query`](../crates/gaze-cli/README.md#audit-query) | Print filtered redaction-log metadata rows as TSV from a read-only SQLite DB. | always |
| [`gaze audit export`](../crates/gaze-cli/README.md#audit-export) | Export filtered redaction-log metadata rows as JSONL for downstream processing. | always |
| `gaze audit purge` | Manually delete redaction-log metadata rows older than an ISO 8601 UTC timestamp. See [Guides](#guides). | always |
| [`gaze audit safety-net query`](../crates/gaze-cli/README.md#audit-safety-net-query) | Print filtered `safety_net_log` rows as TSV. | always |
| `gaze document clean` | OCR a PNG/JPG/PDF into a `SafeBundle` (`clean.md` + `manifest.json` + `report.json`). | `document` |
| [`gaze mcp install`](../crates/gaze-cli/README.md#mcp-installation) | Install `gaze mcp serve` into a supported MCP client config. | `mcp` |
| [`gaze mcp doctor`](../crates/gaze-cli/README.md#mcp-installation) | Diagnose MCP runtime dependencies, client config, and AGENTS.md guidance. | `mcp` |
| [`gaze mcp serve`](../crates/gaze-cli/README.md#mcp-installation) | Run the stdio MCP server exposing agent-tier document tools. | `mcp` |

For exit codes and stderr error JSON, see
[Exit codes](../crates/gaze-cli/README.md#exit-codes) in the crate README.
For policy schema details, see [`docs/policy.md`](policy.md).

## Guides

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
