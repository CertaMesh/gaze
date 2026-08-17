# gaze-audit

[![Crates.io](https://img.shields.io/crates/v/gaze-audit.svg)](https://crates.io/crates/gaze-audit)
[![docs.rs](https://docs.rs/gaze-audit/badge.svg)](https://docs.rs/gaze-audit)
[![License](https://img.shields.io/crates/l/gaze-audit.svg)](https://github.com/CertaMesh/gaze#license)

Passive audit sinks for Gaze metadata-only redaction logs

Part of the [Gaze](https://github.com/CertaMesh/gaze) workspace — a reversible PII pseudonymization runtime for agentic LLM workflows.

Provides `SqliteLogger` - the concrete `RedactionLogger` implementation that writes
session-scoped redaction metadata to a local SQLite database. The audit log is
**metadata-only**: it records class, action, source, field metadata, and timestamp,
but never the original PII values or the pseudonymous tokens.
Safety-net leak suspects are written through the canonical
`LeakSuspectLogger::log_leak_suspect` trait method.

## When to use this crate

Add `gaze-audit` when you need to:
- Query which PII classes were detected in a session
- Export audit records for compliance review
- Detect suspected misses via the SafetyNet log table (`SqliteLogger::query_safety_net`)

Do **not** use the audit log to restore PII. The restore contract lives in `SensitiveSnapshot`
(owned by your application). Only the snapshot can reconstruct original values.

## Usage

```toml
[dependencies]
gaze-pii = "0.12.0"
gaze-audit = "0.12.0"
```

Wire the logger when building the pipeline. Note: `SqliteLogger` is **not** `Clone`;
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

Every column emitted by `SqliteLogger`, the full closed-enum value sets for
`decided_by` / `validator_fail_reason` / `fallback_triggered`, and the
`AuditFilter` query dimensions are cataloged in
[`docs/reference/metrics.md`](../../docs/reference/metrics.md#1-audit-row-fields-gaze-audit) — that
doc is the SSOT for observable surfaces and includes stability guarantees
and the version each column landed.

## Audit-query API surface

Adopters building dashboards, exports, or compliance views interact with five
public items beyond `SqliteLogger`:

| Item | Role |
|------|------|
| `AuditFilter` | Plain-struct filter builder. All fields are `Option<_>` and default to `None`, so `AuditFilter::default()` returns every row. Narrow by `class`, `source`, `action`, `document_kind`, `field_path`, `session_id`, `from_epoch_ms` / `to_epoch_ms`, snapshot scheme / alg / key version, plus the v0.7.x ambiguity columns described below. |
| `AuditLogRow` | Shape returned by `SqliteLogger::query`. Metadata only — `class`, `action`, `field_name`, `document_kind`, `conflict_loser`, `decided_by`, `created_at`, `session_id`, snapshot metadata, and the four v0.7.x ambiguity columns. No raw PII, no token values, no restore material. |
| `PresentColumns` | Column-name set discovered from `PRAGMA table_info(redaction_log)`. It lets cross-version readers describe an older database shape without a positional boolean list. |
| `build_audit_query_sql` | Lower-level helper that constructs the `(SQL, params)` pair used by `SqliteLogger::query`. Takes `PresentColumns` so callers querying older databases project `NULL AS <missing_column>` rather than failing. Exposed so external readers can run the same projection logic against a read replica without re-implementing the filter compiler. |
| `AUDIT_RESTRICTED_COLUMNS` | Canonical allowlist of columns the audit-query path may project. `audit export` and `SqliteLogger::query` select only from this set. `redaction_log` may grow columns over time (e.g. snapshot locators, replay hashes); restricting projection here is defense in depth so future schema additions never accidentally leak raw PII, token bytes, or document content. The clean path is forbidden from touching audit storage by the `gaze_module_isolation` Dylint lint; this constant is the matching read-side guard. |

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

- `validator_fail_reason` — JSON-encoded closed enum (`LuhnFailed`,
  `IbanMod97Failed`, `EmailRfcFailed`, `E164PhoneFailed`) for validator-veto
  losers. Populated only on rows where `decided_by = ValidatorVeto`.
- `ambiguity_record` — JSON-encoded `AmbiguityRecord` (family-level class,
  losing candidate list, closed `AmbiguityReason`). Populated when the
  resolver fell back to a family-level token instead of a precise variant.
- `collision_family` — plain string identifier for the
  `[recognizers.collision]` family this row belongs to. `NULL` for rows
  outside collision-family policy.
- `collision_variant` — plain string variant identifier within the family.
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

`gaze` core has **no compile-time dependency** on `gaze-audit`. The `gaze_module_isolation`
Dylint lint enforces this - the clean/tokenize path cannot accidentally import the audit
path. Wire `SqliteLogger` only in your application layer, never in library crates that
compose pipelines.

## Feature flags

This crate has no optional features. It always depends on `rusqlite`.

## MSRV

`rust-version = "1.89"` (matches the workspace).
