# Gaze CLI

## Audit Metadata

`gaze audit` reads SQLite redaction-log metadata produced by `gaze clean --audit-db`.
It is intentionally restricted to metadata columns and must not read raw spans,
restore mappings, token payloads, or future sensitive columns.

### Query

```sh
gaze audit query --audit-db audit.sqlite
gaze audit query --audit-db audit.sqlite --class email --source email.global
gaze audit query --audit-db audit.sqlite --action tokenize --document-kind text
gaze audit query --audit-db audit.sqlite --from 2026-04-26T00:00:00Z --to 2026-04-27T00:00:00Z
```

`query` writes tab-separated rows to stdout with a header row. `created_at` is
epoch milliseconds. Legacy audit databases without `created_at` remain
queryable; missing values are empty in TSV output. Unfiltered queries include
legacy rows. Filtered queries omit NULL rows by SQL semantics; to access legacy
rows, omit the matching time filter.

### Export

```sh
gaze audit export --audit-db audit.sqlite --format jsonl
gaze audit export --audit-db audit.sqlite --format jsonl --output audit.jsonl
gaze audit export --audit-db audit.sqlite --format jsonl --from 2026-04-26T00:00:00Z
```

`export --format jsonl` writes one JSON object per row:

```json
{"source":"email.global","class":"email","action":"tokenize","field_name":null,"document_kind":"text","conflict_loser":false,"decided_by":"recognizer_id","created_at":1777161600000}
```

### Purge

`gaze audit purge` manually removes redaction audit metadata rows older than an
ISO 8601 UTC timestamp. It never purges session manifests and does not run in
the background.

```sh
gaze audit purge --audit-db .gaze/audit.sqlite --before 2026-04-01T00:00:00Z --dry-run
gaze audit purge --audit-db .gaze/audit.sqlite --before 2026-04-01T00:00:00Z
```

Successful output is JSON:

```json
{"dry_run":true,"matched":12,"deleted":0}
```

Invalid `--before` values fail closed with a typed JSON error that quotes the
input:

```json
{"error":"AuditPurgeIso8601","exit":2,"input":"not-iso8601"}
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

`--from` and `--to` must be ISO 8601/RFC3339 timestamps with an explicit offset,
for example `2026-04-26T00:00:00Z` or `2026-04-26T01:00:00+01:00`.
Invalid timestamps exit with `PolicyConfig` and code 2.

Audit filters are reporting parameters, not runtime policy knobs. They do not
alter recognizer composition, token emission, restore behavior, or the
three-surfaces runtime contract.

Round-trip and recognizer-composition tests are not applicable to `gaze audit`
because it is read-only metadata reporting: it emits no tokens, rewrites no text,
and composes no recognizers. Fixtures must remain synthetic and AGENTS-safe.
