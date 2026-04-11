# Gaze

GDPR-compliant debugging proxy between AI coding agents and production data.

Single-binary Rust MCP server. Anonymizes MySQL access for LLMs. Session-scoped pseudonymous mode only — no raw PII leaves the proxy.

## What it does

AI agents need to poke at prod to debug. Prod has real user data. Gaze sits between them: agent speaks MCP, Gaze runs the query, strips PII, returns sanitized rows. Every call hits an audit log.

Two layers of defense:

1. **Policy allowlist** — `policy.toml` declares which tables/columns are reachable and their PII class. Datenschutzbeauftragter reviews this file.
2. **PII detector** — [`pii`](https://github.com/worka-ai/pii) crate scans results for anything the allowlist missed. Active error sanitization with canary guard so nothing slips through error paths either.

## MCP tools

Served over stdio via `rmcp 0.2`:

- `db.schema` — table/column listing within allowed scope
- `db.sample` — anonymized row sample (`max_rows` capped)
- `db.count` — count on allowed columns
- `db.distinct` — distinct values on allowed columns (`max_distinct` capped)
- `db.explain` — query plan
- `logs.*` — Laravel log adapter with regex strip patterns

## CLI

```
gaze init                  # scaffold policy.toml + .gaze/
gaze check  [policy.toml]  # parse + validate policy
gaze serve  [policy.toml]  # start MCP stdio server
gaze audit                 # print SQLite audit log
```

Global flags: `--global` (use `~/.gaze/audit.db`), `--allow-unlocked-key` (skip mlock in containers).

## Policy

See `policy.example.toml`. One `[connection.production]` block required. SSH tunnel supported for prod access. Allowed operations and per-column PII classes declared explicitly.

```toml
[connection.production]
kind = "mysql"
ssh_host = "deploy@prod.example.com"
database = "myapp"
user = "gaze_ro"
password_env = "GAZE_DB_PASSWORD"

[policy.database]
allowed_tables = ["users", "orders"]
blocked_columns = ["iban", "tax_id"]
max_rows = 50
allowed_operations = ["schema", "sample", "count", "distinct", "explain"]

[[policy.database.columns]]
table = "users"
column = "email"
class = "email"
```

## Build

```
cargo build --release
```

MSRV: Rust 1.89 (forced by transitive deps).

## Status

v0.1 — M0–M5 complete. Scaffold, types, adapter trait + MySQL impl, SSH tunnel, policy parser, CLI wiring, regex scanner + Laravel log adapter, SQLite audit log, all 8 MCP handlers, rmcp stdio bootstrap, canary e2e leak guard. M6 (dogfood) and M7 (release) out of scope for v0.1.

## License

Apache-2.0.
