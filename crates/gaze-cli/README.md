# gaze-cli

[![Crates.io](https://img.shields.io/crates/v/gaze-cli.svg)](https://crates.io/crates/gaze-cli)
[![docs.rs](https://docs.rs/gaze-cli/badge.svg)](https://docs.rs/gaze-cli)
[![License](https://img.shields.io/crates/l/gaze-cli.svg)](https://github.com/CertaMesh/gaze#license)

Gaze command-line interface

This crate publishes the `gaze` binary. It is the process boundary used by
shell integrations and language adapters that should not link the Rust library
directly.

The CLI reads from stdin, writes JSON to stdout, and emits sanitized structured
errors to stderr. Panic handling is overridden so dependency panics do not dump
raw input or backtraces into caller logs.

## Install

Install from crates.io:

```console
$ cargo install gaze-cli --version 0.16.0
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
| `setup` | Installs and verifies the pinned NER model, writes a policy, and runs a doctor check. Included in the default build. |
| `daemon` | Runs a long-lived JSONL stdio cleaner with one process-level pipeline and per-`session_id` manifests. |
| `restore` | Reads `{"session_blob","text"}` JSON from stdin and emits restored `{"text"}` JSON, plus `restore_warning` when tolerant restore allows an unknown token. |
| `audit query` | Prints filtered audit metadata rows from a `--audit-db` SQLite log, opened read-only. |
| `audit export` | Exports filtered audit metadata rows in JSONL (default) for downstream processing. |
| `audit safety-net query` | Prints filtered `safety_net_log` rows from a `--audit-db` SQLite log, opened read-only. |
| `document clean` | OCRs PNG/JPG/PDF input into a SafeBundle. Requires `--features document`. |
| `index ingest/search` | Builds and searches a local owner-side text/Markdown index. Requires `--features index`. |
| `mcp install` | Installs `gaze mcp serve` into supported MCP client configs. Requires `--features mcp`. |
| `mcp doctor` | Diagnoses MCP runtime dependencies, client config, and AGENTS.md guidance. Requires `--features mcp`. |
| `mcp serve` | Runs the stdio MCP server exposing `gaze_read_file` and `gaze_read_text`. Requires `--features mcp`. |
| `proxy serve/start/stop/status/restart` | Runs or manages the local LLM API proxy. Included in the default build. |
| `proxy serve --dashboard` | Launches the opt-in, memory-only inspection dashboard child alongside the proxy. Requires `--features dashboard` (default-off); owner-domain capture additionally requires explicit per-domain risk-acknowledgement flags, and any activation failure disables only the dashboard while the proxy keeps serving. On Linux the launcher must hold (or promptly open) the read end of the pairing FIFO, otherwise `gaze proxy serve` blocks in the write-only open before provider startup. Exact flags, pairing display, and failure behavior: [`docs/reference/cli.md`](../../docs/reference/cli.md#dashboard-flags-opt-in-dashboard-cargo-feature). |

Audit logging is captured on `clean` via `--audit-db <path>`; the
`audit query` and `audit export` subcommands read the same database back.

## `clean`

```console
$ printf 'Email %s@%s now' alice example.invalid | gaze clean --policy policy.toml
```

Without `--policy`, `clean` uses bundled `core` and tokenizes every active class. Pass a policy in production. Output is `{clean_text, session_blob, stats}` JSON.

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
| `--rulepack-bundled <name[,name...]>` | Override `[policy.rulepacks].bundled`. Comma separated. `none` disables bundled packs, matching TOML `bundled = []`. A successful run without `core` or its `core-extended` alias prints a core-floor-off notice. `core-extended` is deprecated since v0.8.0; use `core --locale=<lang>` for explicit locale-gated activation. |
| `--rulepack-path <path>` | Override `[policy.rulepacks].paths`. Repeatable; keeps the default `core` bundle unless `--rulepack-bundled` overrides it. |
| `--max-bytes <bytes>` | Stdin byte cap. Defaults to `10485760`. |
| `--context-json <path>` | Typed context envelope with dictionaries, class map, and fields. |
| `--audit-db <path>` | Optional SQLite redaction-log database path for metadata-only audit entries. |
| `--safety-net <kind>` | Repeatable `openai-filter` or `nym` selection; multiple nets stack. A CLI list replaces policy selection. `none` disables all nets for this run and cannot be combined with another value. |
| `--safety-net-backend <backend>` | Replaces exactly one explicit `--safety-net` value. Zero or multiple values are a usage error. Cannot be combined with `--safety-net-registry`. |
| `--safety-net-registry` | Enables locale-aware Pass-3 dispatch through `LocaleAwareModelRegistry`. Requires one or more `--safety-net-add` flags. |
| `--safety-net-add <backend>` | Adds one backend to the registry. Repeatable. First resolved backend wins for v1. The only registry-capable backend is `openai-filter`. |
| `--openai-filter-command <path>` | Path to the local OpenAI Privacy Filter `opf` command. Required with the `openai-filter` backend. |
| `--openai-filter-checkpoint <path>` | Path to the OPF checkpoint or model directory. Required with the `openai-filter` backend. |
| `--opf-command <path>` / `--opf-checkpoint <path>` | Registry-example aliases for the OpenAI Privacy Filter command and checkpoint. |
| `--opf-locales <tag[,tag...]>` | Native locales for the OpenAI Privacy Filter registry entry. Empty keeps the backend default. |
| `--nym-model-dir <path>` | Pinned Nym-small int8 bundle (`SHA256SUMS`, `config.json`, `model_int8.onnx`, `tokenizer.json`). Takes precedence over `GAZE_NYM_MODEL_DIR`, then policy `[safety_net.nym].model_dir`; install with `gaze setup --safety-net nym`. |
| `--nym-intra-threads <n>` | ONNX Runtime intra-op threads for the `nym` backend. Defaults to `1`. |
| `--safety-net-timeout-ms <ms>` | Subprocess deadline. Defaults to `5000`. |
| `--safety-net-input-limit-bytes <bytes>` | Clean-text input cap forwarded to the safety net. Defaults to `1048576`. |
| `--safety-net-mode <strict\|tolerant\|redact\|resolve>` | Production action on `Uncovered`/`PartialBleed` suspects. `strict` exits `3`; `tolerant` emits warnings on stderr and continues (dev-only, fires a stderr warning on every invocation); `redact` replaces the suspect span with a one-way `[REDACTED:<class>]` marker (the bytes do not come back; the marker is not a token and restore returns it verbatim) and records an audit row; `resolve` tokenizes the suspect span directly as a restorable token of the suspect's class, then runs the nets once more; what it cannot handle goes to `--safety-net-fallback`. Defaults to `resolve`. Mode catalog and posture guide: [`docs/explanation/safety-net/safety-net-modes.md`](../../docs/explanation/safety-net/safety-net-modes.md). |
| `--safety-net-fallback <strict\|tolerant\|redact>` | Residual action for `--safety-net-mode resolve`, and only for `resolve`: what happens when the resolve pass cannot tokenize a suspect because it overlaps an existing token (`OverlapConflict`) or the post-resolve re-run still reports one (`ResidualSuspect`). `strict` rejects the document and exits `3`; `tolerant` ships the residual bytes; `redact` tokenizes the re-run's residuals when every one can be tokenized reversibly and otherwise replaces them with a one-way `[REDACTED:<class>]` marker (a first-pass refusal always gets the marker). Defaults to `redact`. Ignored by `strict`, `tolerant`, and `redact` modes: those are terminal per suspect and their failure paths are typed errors that fail closed, not a cascade. One-hop only. `tolerant` requires `GAZE_ALLOW_TOLERANT=1`. Lowering table and audit rows: [`docs/explanation/safety-net/safety-net-modes.md`](../../docs/explanation/safety-net/safety-net-modes.md#the-fallback-applies-only-under-resolve). |

Full command reference: [CLI reference](../../docs/reference/cli.md). Safety-net `resolve` defaults to a `redact` fallback; this can make one-way replacements. Use the [mode catalog](../../docs/explanation/safety-net/safety-net-modes.md) to choose the required restore and refusal behavior.

## `restore`

```console
$ printf '%s' '{"session_blob":"<base64>","text":"Email <token> now"}' | gaze restore
```

`--restore-mode strict` (default) rejects unknown tokens. `tolerant` preserves them and returns `restore_warning`. Output is JSON (`--format json` only); `--max-bytes` defaults to `10485760`.

## `setup`

`gaze setup` (default `--safety-net nym`) downloads and SHA-verifies the pinned
Davlan mBERT NER bundle, the same model the benchmark scores:
`onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` at commit
`cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`, `SHA256SUMS` digest
`7b0b9d0d200bf7f3a39654257f8723998316600852edff8404834eb7edfc5c16`. The default
directory is `$XDG_DATA_HOME/gaze/models/davlan-mbert-ner-hrl`, else
`~/.local/share/gaze/models/davlan-mbert-ner-hrl`. The written policy's `[ner]`
section points at it, and setup prints `For gaze index: export
GAZE_NER_MODEL_DIR=<dir>`.
The generated policy enables every bundled rulepack, including `secrets`, and
its declared locales, and tokenizes every detected class.
For a hand-authored policy, pinned NER fetch script, OPF build steps, and safety-net
modes, see [Manual policy setup](../../docs/how-to/manual-policy.md).

Releases up to v0.14.0 installed a different, unbenchmarked DistilBERT NER
bundle as the `[ner]` model. Re-run `gaze setup` to get the benchmarked model.

`gaze setup` also installs the pinned Nym-small bundle and writes
`[safety_net].backend = "nym"` with its absolute `[safety_net.nym].model_dir`.
`--safety-net none` writes the NER-only policy; the removed `ner` value migrates
to `none`. `--safety-net opf` additionally verifies an OpenAI Privacy Filter
checkpoint and prints a Nym + OPF command; OPF remains CLI-only. Setup prints
the model card MIT licence, upstream revision, the open [training-data licence
review](../../docs/explanation/safety-net/safety-nets.md#licence-review-open),
and the opt-out. The doctor checks that Nym catches a synthetic licence plate.

The policy is written owner-only (mode `0600`). When setup and gaze run as
different accounts, see
[Policy file permissions](../../docs/reference/policy.md#policy-file-permissions).

`--dob-judge` additionally installs the SHA-pinned GLiNER multi PII int8 bundle
and enables the local date-of-birth judge in `[dob_judge]`. It is off by
default until the bundle is shrunk; its measured gain and its
memory cost are in the benchmark's
[per-mechanism arms](../../docs/reference/benchmarks/README.md#per-mechanism-arms). `--dob-model-dir <path>` selects the bundle directory and requires
`--dob-judge`. The doctor checks a synthetic cue-less birth date before
publishing the policy.

## Daemon mode

`gaze daemon` is a stdio server in the LSP / MCP tradition, not a Unix daemon
in the strict sense. The subcommand verb is preserved for binary stability; see
[`docs/explanation/daemon/daemon-mode.md`](../../docs/explanation/daemon/daemon-mode.md)
for the terminology note.

`gaze daemon --policy policy.toml` keeps one pipeline alive and reads one JSON
request per stdin line:

JSON decodes the Unicode escape before detection, so this request contains a
synthetic email address without spelling it literally in release text.

```json
{"session_id":"conversation-1","text":"Contact alice\u0040example.invalid"}
```

The clean response has an email token in `clean_text` and nonempty `manifest`
and `tokens` arrays. The token's session prefix changes each run.

On failure, stdout contains a typed protocol or cleaning error:

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

## `audit query`

```console
$ gaze audit query --audit-db audit.sqlite --class email --action tokenize
```

Reads metadata as TSV with `SQLITE_OPEN_READ_ONLY`. Filters cover class, source, action, document kind, and inclusive `--from` / `--to` ISO8601 timestamps. Time filters omit legacy NULL timestamps; unfiltered queries retain them. Complete filters: [CLI reference](../../docs/reference/cli.md).

## `audit export`

```console
$ gaze audit export --audit-db audit.sqlite --format jsonl --output redactions.jsonl
```

Uses the query filters and a restricted metadata-only column set including `created_at`. JSONL is the only format; without `--output`, writes stdout.

## `audit safety-net query`

```console
$ gaze audit safety-net query --audit-db audit.sqlite --leak-kind uncovered --field-path '$.user.email'
```

Reads `safety_net_log` as TSV, read-only. Filters: `--leak-kind` (`uncovered`, `partial_bleed`, `class_mismatch`), `--raw-label`, `--mapped-class`, `--field-path`, inclusive `--from` / `--to`. `raw_label` is a validated label, never upstream text. [Schema](../../docs/explanation/safety-net/safety-nets.md#safety_net_log-audit-table).

## `index`

The `index` feature adds a local owner-side search index for `.txt` and `.md`
corpora:

```console
$ gaze setup
$ export GAZE_NER_MODEL_DIR=~/.local/share/gaze/models/davlan-mbert-ner-hrl
$ cargo run -p gaze-cli --features index -- index ingest ./notes
$ cargo run -p gaze-cli --features index -- index search "$(printf '%s@%s' alice example.invalid)" --class email
```

`index ingest` runs the same deterministic floor as `gaze clean` without a
policy (bundled `core`, every detected span tokenized), plus the `Label: value`
field detector and the NER bundle. It requires the pinned Davlan mBERT NER
bundle that `gaze setup` installs, passed as `--ner-model-dir <dir>` or `GAZE_NER_MODEL_DIR`. It is the
detector for prose names and organizations. The directory must verify against
the pinned digests. A missing or unpinned directory fails closed with the typed
`IndexNerModelMissing` error (exit `2`) and nothing is written. `gaze setup`
prints the `export GAZE_NER_MODEL_DIR=<dir>` line to use.

The safety net is optional for `ingest` and required for `search`: TokenBridge
checks every search snippet with an output net and refuses a search that has
none, so `index search` without `--safety-net` fails closed with a typed
`SafetyNetConfig` error (exit `3`). Both remaining nets, `openai-filter` and
`nym`, satisfy it. Pass the net on the `index` command, before the subcommand:

| Flag | Meaning |
| --- | --- |
| `--safety-net <openai-filter\|nym>` | Checks ingest output (optional) and search snippets (required for `search`). |
| `--opf-command <path>` | OPF `opf` command. Falls back to `GAZE_OPENAI_FILTER_OPF`. |
| `--opf-checkpoint <path>` | OPF checkpoint directory. Falls back to `OPF_CHECKPOINT`. |
| `--nym-model-dir <path>` | Pinned Nym-small bundle. Falls back to `GAZE_NYM_MODEL_DIR`. |
| `--safety-net-timeout-ms <ms>` | Safety-net subprocess timeout. |

With a net configured, residual suspects on ingest are tokenized when all of
them can be, and otherwise redact or fail closed per `index ingest --on-residual {redact,strict}` (default `redact`):

```console
$ gaze index --safety-net nym --nym-model-dir <dir> ingest ./notes --on-residual strict
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
legacy plaintext `index.json` files are not loaded. The sealed payload is
schema-versioned (currently v2: one record per document, with the search
postings derived on load); when a file's schema does not match the loader,
`gaze index` fails closed and the index must be rebuilt with `gaze index ingest`.

Search output is the bridge's agent-facing response: snippets contain only
current-session tokens, never raw PII or index-domain aliases. The v1 ingest path
is text/Markdown only; OCR/PDF/image ingestion stays with `gaze document`.

## MCP installation

The `mcp` feature embeds the rmcp stdio server into the `gaze` binary and
registers `gaze-document` tools:

```console
$ cargo install gaze-cli --version 0.16.0 --features mcp
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

## Safety net

Safety nets report suspects; the pipeline chooses the action. No net runs without policy selection or a CLI choice. Repeat `--safety-net` to stack nets; `--safety-net-backend` replaces exactly one explicit choice.

### Safety-net backends

| Backend | Build / coverage |
| --- | --- |
| `openai-filter` | Opt-in `safety-net-openai`; subprocess with Person, Email, Phone, URL, Address, Date, Account number, Secret labels. Heavier model and per-call startup. |
| `nym` | Default build through `setup` / `safety-net-nym`; pinned Nym-small v3 int8 in-process model. Default op-B labels: building numbers, licence plates, usernames, birth dates. Policy changes labels/thresholds; registry dispatch is unsupported. |

### Build and install the backends

```console
$ cargo build -p gaze-cli --features safety-net-openai
$ gaze setup
```

Setup installs Nym. For OPF, pin an upstream revision/release from [`openai/privacy-filter`](https://github.com/openai/privacy-filter), record it in the deployment manifest, and supply the binary and weights. The adapter downloads or updates neither.

Set `GAZE_OPENAI_FILTER_OPF=/opt/opf/bin/opf` or `--openai-filter-command`. Absolute command paths must be regular files, not symlinks. The checkpoint must be user-owned, mode `0700`, with no group/world write bits. Missing weights return `WeightsMissing` (exit `3`) before spawn; initialization failures are cached for the process lifetime.

### Synthetic example: strict mode

```console
$ printf 'Email %s@%s now' alice example.invalid | gaze clean \
    --policy=policy.toml --safety-net=openai-filter --safety-net-mode=strict \
    --openai-filter-command=/opt/opf/bin/opf --openai-filter-checkpoint=/opt/opf/checkpoint
```

Output adds `leak_report.stats` with suspect, uncovered, partial-bleed, class-mismatch, and locale-skipped counts. Exit `0` with zero suspects means the net reported none; detection cannot certify that all PII was found.

### Synthetic example: tolerant mode

Dev-only `--safety-net-mode tolerant` warns on stderr and exits `0` when uncovered/partial-bleed suspects remain. Strict mode returns `SuspectedLeak` (exit `3`) with empty stdout. Class mismatches warn but do not fail strict mode; findings wholly inside owned placeholders are dropped before action.

Default CLI and `SafetyNetPolicy::default()` behavior is `resolve` plus `redact` fallback. See the [mode catalog](../../docs/explanation/safety-net/safety-net-modes.md) for overlap, residual, one-way replacement, and tolerant opt-in rules.

### Latency budget (`openai-filter`)

Each check spawns OPF. `--safety-net-timeout-ms` defaults to `5000`; timeout kills with `SIGKILL`, reaps, and returns `Timeout` (exit `3`). Startup is not amortized.

### Audit

`--audit-db` stores metadata-only suspects. Read them with [`audit safety-net query`](#audit-safety-net-query). [Schema and bytes-free contract](../../docs/explanation/safety-net/safety-nets.md#safety_net_log-audit-table).

## Exit codes

Exit codes are defined by `CliError` in [`src/error.rs`](src/error.rs).

| Exit | Variants |
|------|----------|
| `0` | Success, help, version output, or tolerant-mode safety-net runs that produced only stderr warnings. |
| `1` | `StdinParse`, `EmptyInput`, `InputTooLarge`, `InvalidEncoding`. |
| `2` | `PolicyConfig`, including unsupported format, invalid policy, invalid locale, invalid NER threshold, unknown rulepack, unsupported CLI column rules, `SafetyNetConfig` (missing backend command/checkpoint or safety-net flags supplied without the matching feature), or `SafetyNetArtifactMissing` (Axis-1 fail-closed when a backend's pinned artifact is absent, including a missing `SHA256SUMS`). |
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

## Approved synthetic PII in examples

Use reserved email domains, fictional phone ranges, and synthetic names. See [fixture rules](../../CONTRIBUTING.md#phone-number-fixtures). Never paste customer or operator data into examples; cite the checked-in test for PII-shaped fixtures.
