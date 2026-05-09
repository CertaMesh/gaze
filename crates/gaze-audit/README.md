# gaze-audit

[![Crates.io](https://img.shields.io/crates/v/gaze-audit.svg)](https://crates.io/crates/gaze-audit)
[![docs.rs](https://docs.rs/gaze-audit/badge.svg)](https://docs.rs/gaze-audit)
[![License](https://img.shields.io/crates/l/gaze-audit.svg)](https://github.com/EmpireTwo/gaze#license)

Passive audit sinks for Gaze metadata-only redaction logs

Part of the [Gaze](https://github.com/EmpireTwo/gaze) workspace — a reversible PII pseudonymization runtime for agentic LLM workflows.

Provides `SqliteLogger` - the concrete `RedactionLogger` implementation that writes
session-scoped redaction metadata to a local SQLite database. The audit log is
**metadata-only**: it records class, action, source, field metadata, and timestamp,
but never the original PII values or the pseudonymous tokens.

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
gaze-pii = "0.6"
gaze-audit = "0.6"
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

## Isolation gate

`gaze` core has **no compile-time dependency** on `gaze-audit`. The `gaze_module_isolation`
Dylint lint enforces this - the clean/tokenize path cannot accidentally import the audit
path. Wire `SqliteLogger` only in your application layer, never in library crates that
compose pipelines.

## Feature flags

This crate has no optional features. It always depends on `rusqlite`.

## MSRV

`rust-version = "1.89"` (matches the workspace).
