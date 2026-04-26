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

When `--policy` is omitted, the CLI runs a stub email pipeline so the process
surface can be exercised. Production use should pass `--policy`.

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

## Exit codes

Exit codes are defined by `CliError` in [`src/error.rs`](src/error.rs).

| Exit | Variants |
|------|----------|
| `0` | Success, help, or version output. |
| `1` | `StdinParse`, `EmptyInput`, `InputTooLarge`, `InvalidEncoding`. |
| `2` | `PolicyConfig`, including unsupported format, invalid policy, invalid locale, invalid NER threshold, unknown rulepack, or unsupported CLI column rules. |
| `3` | `UnknownToken`, `InvalidSignature`, `InvalidBlobVersion`, `BlobExpired`, `Pipeline`, and sanitized panic path. |
| `4` | `Io`, `PolicyOpen`. |

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
