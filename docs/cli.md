# Gaze CLI

## Audit Metadata

`gaze audit` reads SQLite redaction-log metadata produced by `gaze clean --audit-db`.
It is read-only and intentionally returns a restricted column set:

```text
class    source    action    document_kind    decided_by
```

The audit command must not read raw spans, restore mappings, token payloads, or
future sensitive columns. This preserves the Gaze north star: audit reporting can
explain what happened without moving PII back toward an agent or terminal.

### Query

```sh
gaze audit query --audit-db audit.sqlite
gaze audit query --audit-db audit.sqlite --class email --source email.global
gaze audit query --audit-db audit.sqlite --action tokenize --document-kind text
```

`query` writes tab-separated rows to stdout with a header row.

### Export

```sh
gaze audit export --audit-db audit.sqlite --format jsonl
gaze audit export --audit-db audit.sqlite --format jsonl --output audit.jsonl
```

`export --format jsonl` writes one JSON object per row:

```json
{"class":"email","source":"email.global","action":"tokenize","document_kind":"text","decided_by":"recognizer_id"}
```

### Filters

The v0.4.3 audit CLI supports only fields that already exist in
`RedactionEntry` metadata:

| Filter | v0.4.3 status | Rationale |
|---|---:|---|
| `--class` | in | `RedactionEntry.class` exists |
| `--source` | in | `RedactionEntry.source` exists |
| `--action` | in | `RedactionEntry.action` exists |
| `--document-kind` | in | `RedactionEntry.document_kind` exists |
| `--session` | deferred | no session column in the audit schema |
| `--from` / `--to` | deferred | needs a `created_at` schema migration |

Audit filters are reporting parameters, not runtime policy knobs. They do not
alter recognizer composition, token emission, restore behavior, or the
three-surfaces runtime contract.

Round-trip and recognizer-composition tests are not applicable to `gaze audit`
because it is read-only metadata reporting: it emits no tokens, rewrites no text,
and composes no recognizers. Fixtures must remain synthetic and AGENTS-safe.
