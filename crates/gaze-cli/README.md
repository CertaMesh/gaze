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

Current subcommands in [`src/main.rs`](src/main.rs):

| Subcommand | Purpose |
|------------|---------|
| `clean` | Reads raw UTF-8 text from stdin and emits `{"clean_text","session_blob","stats"}` JSON. |
| `restore` | Reads `{"session_blob","text"}` JSON from stdin and emits restored `{"text"}` JSON, plus `restore_warning` when tolerant restore allows an unknown token. |

There is no separate `audit` subcommand in the current binary. Audit logging is
enabled on `clean` with `--audit-db <path>`.

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
| `--locale <tag[,tag...]>` | Active locale fallback chain, comma separated and priority ordered. |
| `--ner-threshold <float>` | Override policy `[ner]` threshold. Must be between `0.0` and `1.0` inclusive. |
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
