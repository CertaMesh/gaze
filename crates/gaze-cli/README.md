# gaze-cli

Command-line interface for Gaze.

This crate publishes the `gaze` binary. It is the process boundary used by
shell integrations and language adapters that should not link the Rust library
directly.

The CLI reads from stdin, writes JSON to stdout, and emits sanitized structured
errors to stderr. Panic handling is overridden so dependency panics do not dump
raw input or backtraces into caller logs.

## Cargo

Build from the workspace root:

```console
$ cargo build -p gaze-cli
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
| `restore` | Reads `{"session_blob","text"}` JSON from stdin and emits restored `{"text"}` JSON, plus `restore_warning` when tolerant restore allows an unknown token. |
| `audit query` | Prints filtered audit metadata rows from a `--audit-db` SQLite log, opened read-only. |
| `audit export` | Exports filtered audit metadata rows in JSONL (default) for downstream processing. |

Audit logging is captured on `clean` via `--audit-db <path>`; the
`audit query` and `audit export` subcommands read the same database back.

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
| `--rulepack-bundled <name[,name...]>` | Override `[policy.rulepacks].bundled`. Comma separated. |
| `--rulepack-path <path>` | Override `[policy.rulepacks].paths`. Repeatable. |
| `--max-bytes <bytes>` | Stdin byte cap. Defaults to `10485760`. |
| `--context-json <path>` | Typed context envelope with dictionaries, class map, and fields. |
| `--audit-db <path>` | Optional SQLite redaction-log database path for metadata-only audit entries. |
| `--safety-net <kind>` | Optional observer-only safety net. Currently `openai-filter`. Activates the post-clean leak audit. |
| `--openai-filter-command <path>` | Path to the local OpenAI Privacy Filter `opf` command. Required with `--safety-net=openai-filter`. |
| `--openai-filter-checkpoint <path>` | Path to the OPF checkpoint or model directory. Required with `--safety-net=openai-filter`. |
| `--openai-filter-operating-point <point>` | Operating point: `high-recall`, `balanced`, `high-precision`. |
| `--safety-net-timeout-ms <ms>` | Subprocess deadline. Defaults to `5000`. |
| `--safety-net-input-limit-bytes <bytes>` | Clean-text input cap forwarded to the safety net. Defaults to `1048576`. |
| `--safety-net-mode <strict\|tolerant>` | `strict` exits `3` on `Uncovered`/`PartialBleed` suspects; `tolerant` emits warnings on stderr and continues. Defaults to `strict`. |

When `--policy` is omitted, the CLI runs a stub email pipeline so the process
surface can be exercised. Production use should pass `--policy`.

### Safety net

The optional `--safety-net=openai-filter` flag activates the observer-only
safety net documented in
[docs/architecture/safety-nets.md](../../docs/architecture/safety-nets.md).
The safety net runs after the deterministic clean and reports suspected
leaks against the manifest of emitted tokens. It cannot mutate the clean
text and cannot affect restore.

#### Setup

The safety-net feature is gated off by default. Build with:

```console
$ cargo build -p gaze-cli --features safety-net-openai
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

#### Synthetic example — strict mode

```console
$ printf '%s' 'Email alice@example.invalid or call 555-0100 now' \
  | gaze clean \
      --policy=policy.toml \
      --safety-net=openai-filter \
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

Strict mode (the default) would exit `3` with the JSON error
`{"error":"SafetyNet","exit":3,"variant":"SuspectedLeak"}` and stdout would
be empty. `ClassMismatch` suspects always warn but never fail strict mode,
because the manifest still tokenized the bytes — only the class disagrees.

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
[`docs/architecture/safety-nets.md`](../../docs/architecture/safety-nets.md#safety_net_log-audit-table).

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
[`docs/architecture/safety-nets.md`](../../docs/architecture/safety-nets.md#safety_net_log-audit-table)
for the full schema.

## Exit codes

Exit codes are defined by `CliError` in [`src/error.rs`](src/error.rs).

| Exit | Variants |
|------|----------|
| `0` | Success, help, version output, or tolerant-mode safety-net runs that produced only stderr warnings. |
| `1` | `StdinParse`, `EmptyInput`, `InputTooLarge`, `InvalidEncoding`. |
| `2` | `PolicyConfig`, including unsupported format, invalid policy, invalid locale, invalid NER threshold, unknown rulepack, unsupported CLI column rules, or `SafetyNetConfig` (missing `--openai-filter-command` / `--openai-filter-checkpoint`, or safety-net flags supplied without the `safety-net-openai` feature). |
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

## Policy path

`clean --policy <path>` loads the TOML policy through `gaze::Policy`, loads
bundled/path rulepacks, resolves locale precedence, builds a pipeline with
`gaze-assembly`, then exports the session as `session_blob`.

For policy schema details, see [docs/policy.md](../../docs/policy.md).
