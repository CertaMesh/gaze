# gaze-audit

[![Crates.io](https://img.shields.io/crates/v/gaze-audit.svg)](https://crates.io/crates/gaze-audit)
[![docs.rs](https://docs.rs/gaze-audit/badge.svg)](https://docs.rs/gaze-audit)
[![License](https://img.shields.io/crates/l/gaze-audit.svg)](https://github.com/CertaMesh/gaze#license)

`SqliteLogger` stores session-scoped metadata: class, action, source, fields, and timestamp. It never stores original PII or token values. Use it for queries, exports, and suspected misses (`query_safety_net`). Safety-net writes use `LeakSuspectLogger::log_leak_suspect`.

## When to use this crate

Restore requires the owner-held `SensitiveSnapshot`; audit rows cannot restore PII.

## Usage

```toml
[dependencies]
gaze-pii = "0.16.0"
gaze-audit = "0.16.0"
```

Wire the logger when building the pipeline. Note: `SqliteLogger` is not `Clone`;
construct it where the pipeline is built and pass it directly.

```rust,no_run
use std::path::Path;
use gaze::Pipeline;
use gaze_audit::SqliteLogger;

let logger = SqliteLogger::new(Path::new("audit.db"))?;
let pipeline = Pipeline::builder()
    // ... recognizers and rules ...
    .redaction_logger(logger)
    .build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Query metadata after running redactions. `query` is a static function on `SqliteLogger`
that takes a path; the logger may already be moved into the pipeline:

```rust,no_run
use std::path::Path;
use gaze_audit::{AuditFilter, SqliteLogger};

let rows = SqliteLogger::query(Path::new("audit.db"), &AuditFilter::default())?;
for row in &rows {
    // metadata only - class, action, session_id, field metadata, timestamp; no raw PII
    println!("{:?} {:?}", row.class, row.action);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Fields, enum values, query filters, and stability: [Metrics](../../docs/reference/metrics.md#1-audit-row-fields-gaze-audit).

## Audit-query API surface

| Item | Role |
|------|------|
| `AuditFilter` | Optional filters; `default()` returns all rows. Filter by `class`, `source`, `action`, `document_kind`, `field_path`, `session_id`, `from_epoch_ms` / `to_epoch_ms`, snapshot scheme / alg / key version, plus the v0.7.x ambiguity columns described below. |
| `AuditLogRow` | Query result: `class`, `action`, `field_name`, `document_kind`, `conflict_loser`, `decided_by`, `created_at`, `session_id`, snapshot metadata, and the four v0.7.x ambiguity columns. No raw PII, no token values, no restore material. |
| `PresentColumns` | Columns discovered with `PRAGMA table_info(redaction_log)` for cross-version reads. |
| `build_audit_query_sql` | Builds `(SQL, params)`. Uses `PresentColumns` to project `NULL AS <missing_column>` for older databases; read replicas can reuse it. |
| `AUDIT_RESTRICTED_COLUMNS` | Projection allowlist for `audit export` and `query`. New storage columns cannot expose PII, token bytes, or document content through these reads. |

Safety-net writes use the `LeakSuspectLogger` trait:

```rust,no_run
use std::path::Path;
use gaze_audit::{LeakSuspectLogEntry, LeakSuspectLogger, SqliteLogger};

# fn log(entry: &LeakSuspectLogEntry) -> gaze_audit::Result<()> {
let logger = SqliteLogger::new(Path::new("audit.db"))?;
logger.log_leak_suspect(entry)?;
# Ok(())
# }
```

## Ambiguity side-channel columns (v0.7.2)

`SqliteLogger`'s `redaction_log` migration adds four nullable columns and
`AuditLogRow` mirrors them as `Option<String>`:

- `validator_fail_reason`: JSON-encoded closed `ValidatorFailReason` enum
  (one variant per validator, such as `LuhnFailed`, `IbanMod97Failed`,
  `EmailRfcRejected`, `PhoneE164Rejected`; full list in
  [validator veto](../../docs/explanation/detection/validator-veto.md#type-ownership))
  for validator-veto losers. The older spellings `email_rfc_failed` and
  `e164_phone_failed` still deserialize. Populated only on rows where
  `decided_by = ValidatorVeto`.
- `ambiguity_record`: JSON-encoded `AmbiguityRecord` (family-level class,
  losing candidate list, closed `AmbiguityReason`). Populated when the
  resolver fell back to a family-level token instead of a precise variant.
- `collision_family`: plain string identifier for the
  `[recognizers.collision]` family this row belongs to. `NULL` for rows
  outside collision-family policy.
- `collision_variant`: plain string variant identifier within the family.
  `NULL` when the family-level fallback fired (no specific variant was
  emitted).

Migration is lazy and idempotent: `SqliteLogger::new(path)` runs
`CREATE TABLE IF NOT EXISTS` followed by `PRAGMA table_info` and
`ALTER TABLE ADD COLUMN` for any missing column. There is no schema-version
table; reopening an up-to-date database is a no-op.

`AuditFilter` exposes four matching filter fields (`has_ambiguity`,
`ambiguity_reason`, `collision_family`, `collision_variant`) and the CLI
surfaces them as `--has-ambiguity`, `--ambiguity-reason <variant>` (kebab
case, e.g. `no-anchor`), `--collision-family <id>`, and
`--collision-variant <id>` on `gaze audit query` and `gaze audit export`.
Full contract:
[`docs/explanation/detection/ambiguity-side-channel.md`](../../docs/explanation/detection/ambiguity-side-channel.md).

## Isolation gate

`gaze` core has no compile-time dependency on `gaze-audit`. The `gaze_module_isolation`
Dylint lint enforces this - the clean/tokenize path cannot accidentally import the audit
path. Wire `SqliteLogger` only in your application layer, never in library crates that
compose pipelines.

## Feature flags

This crate has no optional features. It always depends on `rusqlite`.

## MSRV

`rust-version = "1.89"` (matches the workspace).
