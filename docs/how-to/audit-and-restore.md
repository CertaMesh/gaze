# Audit and restore

## How restore resolves tokens

Restore uses a signed `SensitiveSnapshot`. Tokens belong to one session and have class counters. There is no string-map fallback.

## Write, query, export, and purge the audit log

```sh
gaze clean --policy policy.toml --audit-db audit.sqlite < input.txt
gaze audit query --audit-db audit.sqlite --class email --action tokenize
gaze audit export --audit-db audit.sqlite --format jsonl --output redactions.jsonl
gaze audit purge --audit-db audit.sqlite --before 2026-01-01T00:00:00Z
```

`query` and `export` open the DB read-only and exclude raw PII. Rows carry `recognizer_id` and `recognizer_version_id`; pre-v0.8 rows use `legacy_unversioned`. Set retention yourself: there is no policy default or automatic purge.

## Command reference

Command details: [`gaze audit query`](../../crates/gaze-cli/README.md#audit-query), [`gaze audit export`](../../crates/gaze-cli/README.md#audit-export), and [`gaze restore`](../../crates/gaze-cli/README.md#restore) in the CLI reference. Audit columns: [metrics](../reference/metrics.md).
