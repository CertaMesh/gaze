# Gaze CLI

## Audit Metadata

`gaze audit` reads SQLite redaction-log metadata produced by `gaze clean --audit-db`.
It is read-only and intentionally returns a restricted column set:

```text
source    class    action    field_name    document_kind    conflict_loser    decided_by    created_at    session_id
```

The audit command must not read raw spans, restore mappings, token payloads, or
future sensitive columns. This preserves the Gaze north star: audit reporting can
explain what happened without moving PII back toward an agent or terminal.

### Query

```sh
gaze audit query --audit-db audit.sqlite
gaze audit query --audit-db audit.sqlite --class email --source email.global
gaze audit query --audit-db audit.sqlite --action tokenize --document-kind text
gaze audit query --audit-db audit.sqlite --from 2026-04-26T00:00:00Z --to 2026-04-27T00:00:00Z
gaze audit query --audit-db audit.sqlite --session 0196758d-5f00-7a8a-9a4c-6b1347a1f0d2
```

`query` writes tab-separated rows to stdout with a header row. `created_at` is
epoch milliseconds. `session_id` is an opaque UUIDv7-shaped audit metadata id,
not the private token `session_hex` prefix. Legacy audit databases without
`created_at` or `session_id` columns remain queryable; missing values are empty
in TSV output. Unfiltered queries include legacy rows. Filtered queries omit
NULL rows by SQL semantics; to access legacy rows, omit the matching time or
session filter.

### Export

```sh
gaze audit export --audit-db audit.sqlite --format jsonl
gaze audit export --audit-db audit.sqlite --format jsonl --output audit.jsonl
gaze audit export --audit-db audit.sqlite --format jsonl --from 2026-04-26T00:00:00Z
gaze audit export --audit-db audit.sqlite --format jsonl --session 0196758d-5f00-7a8a-9a4c-6b1347a1f0d2
```

`export --format jsonl` writes one JSON object per row:

```json
{"source":"email.global","class":"email","action":"tokenize","field_name":null,"document_kind":"text","conflict_loser":false,"decided_by":"recognizer_id","created_at":1777161600000,"session_id":"0196758d-5f00-7a8a-9a4c-6b1347a1f0d2"}
```

### Filters

The audit CLI supports only fields that already exist in `RedactionEntry`
metadata:

| Filter | Status | Rationale |
|---|---:|---|
| `--class` | in | `RedactionEntry.class` exists |
| `--source` | in | `RedactionEntry.source` exists |
| `--action` | in | `RedactionEntry.action` exists |
| `--document-kind` | in | `RedactionEntry.document_kind` exists |
| `--from` / `--to` | in | `RedactionEntry.created_at` exists |
| `--session` | in | `RedactionEntry.session_id` exists |

`--from` and `--to` must be ISO 8601/RFC3339 timestamps with an explicit offset,
for example `2026-04-26T00:00:00Z` or `2026-04-26T01:00:00+01:00`.
Invalid timestamps exit with `PolicyConfig` and code 2.

Audit filters are reporting parameters, not runtime policy knobs. They do not
alter recognizer composition, token emission, restore behavior, or the
three-surfaces runtime contract.

Round-trip and recognizer-composition tests are not applicable to `gaze audit`
because it is read-only metadata reporting: it emits no tokens, rewrites no text,
and composes no recognizers. Fixtures must remain synthetic and AGENTS-safe.
