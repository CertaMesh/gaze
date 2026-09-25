# Audit and restore

This guide covers the optional metadata audit log and how restore resolves tokens.

## How restore resolves tokens

Restore is manifest-first. Tokens are session-scoped, counted by class, and only resolvable through a signed `SensitiveSnapshot`. There is no string-map fallback.

## Write, query, export, and purge the audit log

Optional metadata audit log:

```sh
gaze clean --policy policy.toml --audit-db audit.sqlite < input.txt
gaze audit query --audit-db audit.sqlite --class email --action tokenize
gaze audit export --audit-db audit.sqlite --format jsonl --output redactions.jsonl
gaze audit purge --audit-db audit.sqlite --before 2026-01-01T00:00:00Z
```

The audit DB is opened read-only by `query` and `export`. The exported column set excludes raw PII payloads. Every row carries `recognizer_id` plus `recognizer_version_id` for lineage; pre-v0.8 rows carry a `legacy_unversioned` marker. There is no policy-level retention default and no background auto-purge — adopters drive retention explicitly.

## Command reference

Command details: [`gaze audit query`](../../crates/gaze-cli/README.md#audit-query), [`gaze audit export`](../../crates/gaze-cli/README.md#audit-export), and [`gaze restore`](../../crates/gaze-cli/README.md#restore) in the CLI reference. Audit columns: [metrics](../reference/metrics.md).
