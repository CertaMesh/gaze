# gaze-types

[![Crates.io](https://img.shields.io/crates/v/gaze-types.svg)](https://crates.io/crates/gaze-types)
[![docs.rs](https://docs.rs/gaze-types/badge.svg)](https://docs.rs/gaze-types)
[![License](https://img.shields.io/crates/l/gaze-types.svg)](https://github.com/CertaMesh/gaze#license)

Shared value contracts for Gaze

Part of the [Gaze](https://github.com/CertaMesh/gaze) workspace — a reversible PII pseudonymization runtime for agentic LLM workflows.

Serde-only — no ML, no SQLite, no ONNX. This crate exists so that:
- Restore-side adapters can take `gaze-types` without pulling in `ort` / `tokenizers` / `ndarray`
- Audit sinks (`gaze-audit`) share the `RedactionLogger` trait without depending on `gaze` core

## When to depend on this crate directly

Use `gaze-types` instead of `gaze` when building:
- An audit sink implementing `RedactionLogger`
- A crate that needs `PiiClass`, `Action`, or `RedactionEntry` without the full pipeline

Otherwise depend on `gaze` — it re-exports the public types from this crate.

## Cargo

```toml
[dependencies]
gaze-types = "0.12.0"
```

## Key types

| Type | Purpose |
|------|---------|
| `PiiClass` | PII category vocabulary (`Email`, `Name`, `Location`, `Organization`, `Custom(String)`) — `#[non_exhaustive]` |
| `Action` | Disposition for a detected span — `#[non_exhaustive]` |
| `RawDocument` | Input variant — `Text(String)` or `Structured(BTreeMap<String, Value>)` — `#[non_exhaustive]` |
| `CleanDocument` | Cleaned output variant — same shape as `RawDocument` — `#[non_exhaustive]` |
| `RedactionLogger` | Trait for audit sinks (metadata-only contract) |
| `RedactionEntry` | One audit row: class, action, span, session, timestamp — no raw PII |
| `ConflictTier` | Precedence tier for resolving overlapping detections |
| `SafetyNet` | Observer-only post-clean trait (does not mutate the manifest) |
| `LeakReport` / `LeakKind` | Suspected-miss report from a `SafetyNet` |

`PiiClass` does **not** include a `Phone` variant. Phone detection is supplied by recognizers
in `gaze-recognizers` (e.g. the `phone-parser` feature) and emitted as `PiiClass::Custom(...)` or
via rulepack-defined classes — see `docs/reference/policy.md`.

## `#[non_exhaustive]` enums

`PiiClass`, `Action`, `RawDocument`, `CleanDocument`, `LeakKind`, `SafetyNet`-related variants
are `#[non_exhaustive]`. Always include a wildcard arm in match statements:

```rust
use gaze_types::PiiClass;

fn label(class: &PiiClass) -> &'static str {
    match class {
        PiiClass::Email        => "email",
        PiiClass::Name         => "name",
        PiiClass::Location     => "location",
        PiiClass::Organization => "org",
        PiiClass::Custom(_)    => "pii",
        _                      => "pii", // forward-compat fallback
    }
}
```

## MSRV

`rust-version = "1.89"` (matches the workspace).
