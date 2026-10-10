# Gaze CLI

The [crate README](../../crates/gaze-cli/README.md) lists all flags and exit
codes. This page indexes commands and their operating contracts.

## Subcommands

Every verb below is defined by the clap `Subcommand` enum in
[`crates/gaze-cli/src/commands/mod.rs`](../../crates/gaze-cli/src/commands/mod.rs).

| Subcommand | One-line summary | Feature gate |
|------------|------------------|--------------|
| [`gaze clean`](../../crates/gaze-cli/README.md#clean) | Read raw text from stdin; emit `{clean_text, session_blob, stats}` JSON. | always |
| `gaze setup` | Install the pinned NER and Nym bundles, write a policy, and run a doctor check. See [guide](#gaze-setup). | `setup` (default build) |
| `gaze daemon` | Run a long-lived JSONL stdio cleaner with one process-level pipeline and per-`session_id` manifests. See [guide](#gaze-daemon). | always |
| [`gaze restore`](../../crates/gaze-cli/README.md#restore) | Read `{session_blob, text}` JSON from stdin; emit restored `{text}` JSON. | always |
| [`gaze audit query`](../../crates/gaze-cli/README.md#audit-query) | Print filtered redaction-log metadata rows as TSV from a read-only SQLite DB. | always |
| [`gaze audit export`](../../crates/gaze-cli/README.md#audit-export) | Export filtered redaction-log metadata rows as JSONL for downstream processing. | always |
| `gaze audit purge` | Manually delete redaction-log metadata rows older than an ISO 8601 UTC timestamp. See [guide](#gaze-audit-purge). | always |
| [`gaze audit safety-net query`](../../crates/gaze-cli/README.md#audit-safety-net-query) | Print filtered `safety_net_log` rows as TSV. See [guide](#gaze-audit-safety-net-query). | always |
| `gaze document clean` | OCR a PNG/JPG/PDF into a split `SafeBundle` (`agent/clean.md`, `agent/report.json`, `owner/manifest.json`). See [guide](#gaze-document-clean) and [crate README](../../crates/gaze-document/README.md). | `document` |
| `gaze mcp install` | Install `gaze mcp serve` into a supported MCP client config. See [guide](#gaze-mcp-install--doctor--serve) and [crate README](../../crates/gaze-cli/README.md#mcp-installation). | `mcp` |
| `gaze mcp doctor` | Diagnose MCP runtime dependencies, client config, and AGENTS.md guidance. See [guide](#gaze-mcp-install--doctor--serve) and [crate README](../../crates/gaze-cli/README.md#mcp-installation). | `mcp` |
| `gaze mcp serve` | Run the stdio MCP server exposing agent-tier document tools. See [guide](#gaze-mcp-install--doctor--serve) and [crate README](../../crates/gaze-cli/README.md#mcp-installation). | `mcp` |
| `gaze proxy` | Run or manage the multi-provider HTTP chokepoint daemon. See [guide](#gaze-proxy) and [crate README](../../crates/gaze-proxy/README.md). | built into the default release binary since v0.8.1 |

## Warnings and exit codes

For exit codes and stderr error JSON, see
[Exit codes](../../crates/gaze-cli/README.md#exit-codes) in the crate README.
For policy schema details, see [`docs/reference/policy.md`](policy.md).

`gaze clean`, `gaze daemon`, and `gaze proxy serve` warn on stderr once per
process after successful processing or startup if a loaded policy lets
registered detection classes fall through to `preserve`. The warning names
the classes and a repair path. `gaze proxy start` and `restart` write the child
process warning to the proxy stderr log. Gaze also warns when a reachable
per-class `generalize` rule creates a one-way value. Errors still use one JSON
value on stderr.

## `gaze setup`

`gaze setup` installs and verifies the pinned Davlan NER and Nym bundles, writes
`gaze.toml` with `[safety_net].backend = "nym"` and an absolute
`[safety_net.nym].model_dir`, then checks that Nym tokenizes a synthetic licence
plate. `--safety-net none` writes a NER-only policy. The old `ner` value was
removed; use `none`. `--safety-net opf` verifies the installed OPF checkpoint
and prints a command that stacks Nym and OPF; policy activation remains Nym.
Setup prints the model card MIT licence, pinned upstream source and revision,
the open [licence review](../explanation/safety-net/safety-nets.md#licence-review-open),
and the `gaze setup --safety-net none` opt-out.

Setup refuses to overwrite an existing policy without `--force`, and checks
this before downloading anything. The written policy is owner-only (mode
`0600`). If a different account runs gaze, grant it read access as described
in [Policy file permissions](policy.md#policy-file-permissions); otherwise gaze
fails closed with `PolicyOpen` and names the file.

## Safety-net selection

`gaze clean` and `gaze daemon` read `[safety_net].backend` from their policy.
Use `--safety-net nym` or `--safety-net openai-filter` to replace that policy
choice for one run. Repeat the flag to run both nets; their suspects are
combined by the pipeline. `--safety-net none` disables all nets for one run
and cannot be mixed with another value. Dropping a policy-enabled Nym prints
one stderr notice. `--safety-net-backend` replaces exactly one explicit
`--safety-net` value; it is a usage error with zero or multiple values.

Nym model location precedence is `--nym-model-dir` > `GAZE_NYM_MODEL_DIR` >
policy `[safety_net.nym].model_dir`. A missing or invalid bundle is a
configuration error; install it with `gaze setup`.

## `gaze daemon`

`gaze daemon` keeps one pipeline loaded for repeated requests. It is a
foreground stdio JSONL server; there is no `gaze serve` alias.

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

SIGINT and SIGTERM set a shutdown flag; the daemon finishes the current line,
flushes stdout and audit writes, then exits. The session registry evicts by LRU
when `--session-cap` is exceeded and by idle timeout when a session is quiet too
long. Each `session_id` owns its own manifest, and eviction emits audit metadata
with source `daemon.session_eviction`.

The daemon stamps its redaction audit rows with
`provenance_stage = "daemon"`, which lets adopters filter daemon-emitted rows
separately from one-shot `gaze clean` rows.

See [`docs/explanation/daemon/daemon-mode.md`](../explanation/daemon/daemon-mode.md) for the
full contract. See
[Run the daemon](../how-to/daemon/run-daemon.md) for an
adopter quickstart.

## `gaze audit purge`

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

## `gaze audit safety-net query`

Query `safety_net_log` observer suspects as TSV, separate from emitted-token
metadata in `redaction_log`.

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

## Legacy audit databases

Audit databases written before v0.4.4 lack a `created_at` column. Unfiltered
`gaze audit query` calls still surface those rows. Filtered queries that use
`--from` or `--to` omit NULL `created_at` rows by SQL semantics; drop the time
filter to access legacy rows.

## `gaze document clean`

OCR PNG/JPG/PDF with Tesseract, run Gaze, then split the SafeBundle:
`clean.md`/`report.json` for the agent; `manifest.json` for the owner.
Requires `document`, Tesseract on PATH and pdfium for PDF.

```sh
cargo install gaze-cli --features document

# Convenience shorthand: creates ./safe-bundle/agent and ./safe-bundle/owner.
gaze document clean ./invoice.pdf --out ./safe-bundle/

# Explicit: caller controls both sides of the partition.
gaze document clean ./invoice.pdf --agent-out ./agent-bundle/ --owner-out ./owner-vault/
```

| Flag | Required | Description |
|------|----------|-------------|
| `--out <PATH>` | Either this or both explicit flags | Convenience shorthand that creates `<PATH>/agent` and `<PATH>/owner`. |
| `--agent-out <PATH>` | With `--owner-out` | Agent-visible directory for `clean.md` and `report.json`. |
| `--owner-out <PATH>` | With `--agent-out` | Owner-only directory for `manifest.json`. |

Keep `owner/manifest.json` away from LLM workspaces: it contains restorable PII.

The supported inputs are `.png`, `.jpg`, `.jpeg`, and single-page `.pdf`. The
`BundleReport` schema is versioned via `bundle_version = 2`. See the
`gaze-document` crate for the full bundle contract.

## `gaze mcp install / doctor / serve`

| Verb | Effect |
|---|---|
| `install` | Write client config using absolute `current_exe()` with `["mcp", "serve"]`; update marker-fenced AGENTS.md guidance idempotently. |
| `doctor` | Check dependencies, config, manifest store and guidance marker. |
| `serve` | Run stdio MCP with agent-tier `gaze_read_file` and `gaze_read_text`. |

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

See [`docs/explanation/mcp/mcp-runtime.md`](../explanation/mcp/mcp-runtime.md) for the
runtime contract.

## `gaze proxy`

`gaze proxy` protects provider API-key traffic while preserving native wire
shapes. It redacts requests, restores owner-visible responses, and accumulates
SSE and tool-call JSON chunks before the model boundary. It is in the default
release binary.

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
| `--bind <addr>` | Listener address. Default for `serve`: IPv4 loopback on port 8787; `start` uses the persisted config unless overridden. |
| `--policy <path>` | Optional policy TOML. When omitted, the built-in core rulepack is used. |
| `--rulepack <name>` | Bundled rulepack name. Default for `serve`: `core`; `start` persists the override. |
| `--session-ttl <duration>` | In-memory session retention such as `30m`, `10s`, or `1h`. Default for `serve`: `30m`. |
| `--upstream-openai <url>` | OpenAI upstream. Default: `https://api.openai.com`. |
| `--upstream-anthropic <url>` | Anthropic upstream. Default: `https://api.anthropic.com`. |
| `--upstream-gemini <url>` | Gemini upstream. Default: `https://generativelanguage.googleapis.com`. |
| `--force` | For `stop` / `restart`, send the hard stop after the bounded wait. |
| `--timeout <duration>` | Stop / restart wait before force. Default: `10s`. |

### Dashboard flags (opt-in, `dashboard` cargo feature)

`serve` and `start` accept the dashboard opt-in flags when the binary is built
with the default-off `dashboard` cargo feature. `--dashboard` launches an
isolated, memory-only inspection dashboard as a killable child process before
provider traffic starts. Enabling it expands the local trusted computing
base: captured payloads become visible to the paired browser session.

| Option | Purpose |
|--------|---------|
| `--dashboard` | Launch the dashboard child; the capture baseline is always ProviderVisible plus safe metadata. |
| `--dashboard-capture-owner-raw` + `--dashboard-acknowledge-owner-raw-risk` | Independently opt into OwnerRaw capture. The capture flag without its acknowledgement — or the acknowledgement without the flag — disables only the dashboard before provider startup. |
| `--dashboard-capture-owner-restored` + `--dashboard-acknowledge-owner-restored-risk` | Independently opt into OwnerRestored capture under the same pairing rule. Capturing both owner domains requires both acknowledgements. |
| `--dashboard-bind <ipv4:0>` | Literal loopback address with port 0. Default: a fresh CSPRNG-selected IPv4 loopback literal per launch. Configuring a fixed literal prints an origin-reuse warning. |
| `--dashboard-ttl <duration>` | Retention TTL. Default `5m`; crate hard ceiling one hour. |
| `--dashboard-max-events <n>` | Retained logical-event cap. Default 64; crate hard ceiling 1024. |
| `--dashboard-max-bytes <n>` | Retained byte cap. Default 4 MiB; crate hard ceiling 64 MiB. |
| `--dashboard-pairing-fd <fd>` | Inherited FIFO descriptor for noninteractive pairing-token delivery. Descriptors 0/1/2, terminals and other character devices, regular files, directories, sockets, and unwritable descriptors are rejected. |

Pairing prints exactly one HTTP origin URL line and one
`GazeDashboardV1 <token>` authorization line to the controlling terminal (or
the validated pairing descriptor), never to stdout, stderr, or a log file.
On Linux, the launcher that supplies `--dashboard-pairing-fd` must hold (or
promptly open) the read end of the pairing FIFO; otherwise `gaze proxy serve`
blocks in the write-only open before provider startup.
Any flag-validation or activation failure — including a missing controlling
terminal without a pairing descriptor — prints one sanitized
`gaze dashboard disabled: <reason>` line, and the proxy starts or continues
without any capture. `start` relays the dashboard flags verbatim to the
daemon serve process; `restart` does not, because dashboard activation
is per-invocation and never persisted into the daemon config. The dashboard
child requires verified no-crash-dump readiness (`RLIMIT_CORE=0`), which
currently limits successful activation to non-macOS Unix hosts; on other
hosts the dashboard disables itself and the proxy continues.

See [Run the local dashboard](../how-to/dashboard/run-local-dashboard.md) and
the [dashboard trust boundary](../explanation/dashboard/trust-boundary.md).

Safety nets, rulepacks and locales follow the loaded policy and CLI overrides.

See [`docs/explanation/proxy/proxy-runtime.md`](../explanation/proxy/proxy-runtime.md) for
the adapter and daemon contract. See
[`crates/gaze-proxy/README.md`](../../crates/gaze-proxy/README.md) for the crate
README.
