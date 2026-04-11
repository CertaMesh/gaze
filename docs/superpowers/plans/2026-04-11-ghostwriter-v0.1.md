# Ghostwriter v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI `ghostwriter` that deterministically sanitizes inbound text into LLM-safe placeholders and restores exact placeholders in LLM drafts, installed locally via `cargo install` and publishable via a Homebrew tap formula.

**Architecture:** New workspace member crate `crates/ghostwriter` inside the Gaze repo. Library exposes `sanitize(req) -> SanitizeResponse` and `restore(req) -> RestoreResponse`. Two-stage sanitize: (1) known-context replacement (`<CUSTOMER_*>`), (2) worka `pii` detection + typed generic placeholders (`<EMAIL_1>`). Session blob is JSON serialized, base64 wrapped, opaque to callers. Strict exact restore — no heuristics. CLI binary reads JSON requests from stdin, writes JSON responses to stdout.

**Tech Stack:** Rust 1.89, Cargo workspace, `serde`/`serde_json`, `thiserror`, `base64`, `clap` v4, `pii` (github.com/worka-ai/pii), `anyhow` (bin only), `assert_cmd` (dev), `tempfile` (dev).

**Reference spec:** `docs/superpowers/specs/2026-04-11-ghostwriter-sanitization-design.md`

---

## File Structure

```
Cargo.toml                                    # root: add [workspace] block
crates/
  ghostwriter/
    Cargo.toml                                # ghostwriter crate manifest (lib + bin)
    src/
      lib.rs                                  # public re-exports + module decls
      types.rs                                # Context, Sanitize/Restore req/resp, Warning, Metadata
      errors.rs                               # SanitizeError, RestoreError (thiserror)
      blob.rs                                 # SessionBlob struct + encode/decode (base64+json)
      placeholder.rs                          # PlaceholderMap: dedupe + per-type numbering
      known_context.rs                        # Stage 1: replace customer_name/email/phone
      detect.rs                               # worka pii detector adapter → (start, end, kind)
      typed_unknown.rs                        # Stage 2: detections → typed placeholders
      sanitize.rs                             # sanitize() orchestrator
      restore.rs                              # restore() strict token substitution
      main.rs                                 # CLI binary (clap subcommands)
    tests/
      roundtrip.rs                            # library-level end-to-end (spec example)
      cli.rs                                  # assert_cmd-based CLI integration
dist/
  homebrew/
    ghostwriter.rb                            # Homebrew formula for naoray/homebrew-tap
```

Each file has one responsibility:
- `types.rs` owns the JSON-serializable data contracts only.
- `placeholder.rs` owns numbering + dedupe logic, nothing else.
- `known_context.rs` and `typed_unknown.rs` each implement one stage.
- `sanitize.rs` composes them; `restore.rs` is independent.
- `main.rs` is a thin CLI shell that calls the library.

---

## Conventions

All commits use the `[agent]` prefix per project convention. Commit after each step marked "Commit". Run tests from the worktree root (`/Users/krishankonig/.anvil/worktrees/gaze/ghostwriter-v0.1`). Current branch is `ghostwriter-v0.1`, already checked out.

All code below is Rust 1.89 edition 2021 syntax. Modules are declared in `lib.rs`.

---

## Task 1: Cargo workspace conversion + crate skeleton

**Files:**
- Modify: `Cargo.toml` (root) — add workspace block
- Create: `crates/ghostwriter/Cargo.toml`
- Create: `crates/ghostwriter/src/lib.rs`
- Create: `crates/ghostwriter/src/main.rs`

- [ ] **Step 1: Add workspace block to root Cargo.toml**

Add at the top of `/Cargo.toml`, above the existing `[package]` block:

```toml
[workspace]
members = ["crates/ghostwriter"]
resolver = "2"
```

Root package `gaze` remains as-is. This creates a hybrid: root `gaze` package + workspace member `ghostwriter`.

- [ ] **Step 2: Create ghostwriter crate manifest**

Create `crates/ghostwriter/Cargo.toml`:

```toml
[package]
name = "ghostwriter"
version = "0.1.0"
edition = "2021"
rust-version = "1.89"
license = "Apache-2.0"
description = "Deterministic text sanitization and exact-token restoration for LLM prompts"

[lib]
name = "ghostwriter"
path = "src/lib.rs"

[[bin]]
name = "ghostwriter"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
base64 = "0.22"
clap = { version = "4.5", features = ["derive"] }
anyhow = "1"
pii = { git = "https://github.com/worka-ai/pii", branch = "main" }

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

- [ ] **Step 3: Create minimal lib.rs**

Create `crates/ghostwriter/src/lib.rs`:

```rust
//! Ghostwriter — deterministic text sanitization and exact-token restoration.
//!
//! See `docs/superpowers/specs/2026-04-11-ghostwriter-sanitization-design.md`
//! for the full design.

pub mod blob;
pub mod detect;
pub mod errors;
pub mod known_context;
pub mod placeholder;
pub mod restore;
pub mod sanitize;
pub mod typed_unknown;
pub mod types;

pub use errors::{RestoreError, SanitizeError};
pub use restore::restore;
pub use sanitize::sanitize;
pub use types::{
    Context, Metadata, RestoreRequest, RestoreResponse, SanitizeRequest, SanitizeResponse, Warning,
};
```

Create placeholder files for every module declared above so compilation succeeds. Each file gets a single header comment + empty content:

- `crates/ghostwriter/src/blob.rs` → `//! Session blob schema.`
- `crates/ghostwriter/src/detect.rs` → `//! Worka pii detector adapter.`
- `crates/ghostwriter/src/errors.rs` → `//! Error types.`
- `crates/ghostwriter/src/known_context.rs` → `//! Stage 1: known context replacement.`
- `crates/ghostwriter/src/placeholder.rs` → `//! Placeholder numbering + dedupe.`
- `crates/ghostwriter/src/restore.rs` → `//! Restore: strict exact token substitution.`
- `crates/ghostwriter/src/sanitize.rs` → `//! Sanitize orchestration.`
- `crates/ghostwriter/src/typed_unknown.rs` → `//! Stage 2: typed unknown placeholders.`
- `crates/ghostwriter/src/types.rs` → `//! Public JSON data contracts.`

Each empty file blocks lib.rs imports until later tasks. Add stub items in types/errors/restore/sanitize to satisfy `lib.rs` re-exports:

`crates/ghostwriter/src/types.rs`:
```rust
//! Public JSON data contracts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Context;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizeRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizeResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warning;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata;
```

`crates/ghostwriter/src/errors.rs`:
```rust
//! Error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SanitizeError {
    #[error("stub")]
    Stub,
}

#[derive(Debug, Error)]
pub enum RestoreError {
    #[error("stub")]
    Stub,
}
```

`crates/ghostwriter/src/sanitize.rs`:
```rust
//! Sanitize orchestration.

use crate::errors::SanitizeError;
use crate::types::{SanitizeRequest, SanitizeResponse};

pub fn sanitize(_req: SanitizeRequest) -> Result<SanitizeResponse, SanitizeError> {
    Err(SanitizeError::Stub)
}
```

`crates/ghostwriter/src/restore.rs`:
```rust
//! Restore: strict exact token substitution.

use crate::errors::RestoreError;
use crate::types::{RestoreRequest, RestoreResponse};

pub fn restore(_req: RestoreRequest) -> Result<RestoreResponse, RestoreError> {
    Err(RestoreError::Stub)
}
```

- [ ] **Step 4: Create stub main.rs**

Create `crates/ghostwriter/src/main.rs`:

```rust
fn main() -> anyhow::Result<()> {
    println!("ghostwriter v{}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
```

- [ ] **Step 5: Verify build**

Run: `cargo build -p ghostwriter`
Expected: compiles clean, produces `target/debug/ghostwriter`.

Run: `cargo build` (root)
Expected: both `gaze` and `ghostwriter` build.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/ghostwriter
git commit -m "[agent] feat: ghostwriter crate skeleton

Step 1 of ghostwriter-v0.1: cargo workspace member with
stub modules so later tasks can fill them in under TDD."
```

---

## Task 2: Public data contracts (types)

**Files:**
- Modify: `crates/ghostwriter/src/types.rs`
- Test: `crates/ghostwriter/src/types.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Replace `types.rs` contents with the full struct definitions plus a serde round-trip test. Start with ONLY the test block at the bottom of the current stub file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_request_roundtrip_matches_spec_example() {
        let json = r#"{
            "text": "Hi Markus Mueller here",
            "context": {
                "customer_name": "Markus Mueller",
                "customer_email": "mueller.markus@icloud.com",
                "customer_phone": "+49 151 23456789"
            }
        }"#;

        let req: SanitizeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hi Markus Mueller here");
        assert_eq!(req.context.customer_name.as_deref(), Some("Markus Mueller"));
        assert_eq!(
            req.context.customer_email.as_deref(),
            Some("mueller.markus@icloud.com")
        );
        assert_eq!(
            req.context.customer_phone.as_deref(),
            Some("+49 151 23456789")
        );
    }

    #[test]
    fn sanitize_response_serializes_placeholders_metadata() {
        let resp = SanitizeResponse {
            clean_text: "Hi <CUSTOMER_NAME>".into(),
            session_blob: "abc".into(),
            warnings: vec![],
            metadata: Metadata {
                placeholders: vec!["<CUSTOMER_NAME>".into()],
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["clean_text"], "Hi <CUSTOMER_NAME>");
        assert_eq!(json["session_blob"], "abc");
        assert_eq!(json["metadata"]["placeholders"][0], "<CUSTOMER_NAME>");
    }

    #[test]
    fn restore_request_requires_text_and_blob() {
        let json = r#"{"text":"Hi <CUSTOMER_NAME>","session_blob":"abc"}"#;
        let req: RestoreRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "Hi <CUSTOMER_NAME>");
        assert_eq!(req.session_blob, "abc");
    }

    #[test]
    fn context_fields_all_optional() {
        let json = r#"{}"#;
        let ctx: Context = serde_json::from_str(json).unwrap();
        assert!(ctx.customer_name.is_none());
        assert!(ctx.customer_email.is_none());
        assert!(ctx.customer_phone.is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ghostwriter types::tests`
Expected: compile errors (fields don't exist on stub structs).

- [ ] **Step 3: Implement real types**

Replace the stub struct definitions at the top of `types.rs` with:

```rust
//! Public JSON data contracts.
//!
//! These types mirror the spec exactly. Field names use snake_case so
//! they match the JSON on the wire without serde rename attributes.

use serde::{Deserialize, Serialize};

/// Known primary customer identity supplied by the caller.
/// Every field is optional — callers pass only what they know.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_phone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizeRequest {
    pub text: String,
    #[serde(default)]
    pub context: Context,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizeResponse {
    pub clean_text: String,
    pub session_blob: String,
    #[serde(default)]
    pub warnings: Vec<Warning>,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub placeholders: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub text: String,
    pub session_blob: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResponse {
    pub restored_text: String,
    #[serde(default)]
    pub warnings: Vec<Warning>,
}

/// Informational warning. Always serialized as a plain string so callers
/// can render them directly without schema knowledge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Warning(pub String);

impl Warning {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ghostwriter types::tests`
Expected: 4 passed.

Run: `cargo build -p ghostwriter`
Expected: stub `sanitize`/`restore` functions still reference old unit-struct types — rebuild might fail because `SanitizeRequest` et al. are no longer unit structs. Fix `sanitize.rs` / `restore.rs` stubs to keep them compiling:

`crates/ghostwriter/src/sanitize.rs`:
```rust
//! Sanitize orchestration.

use crate::errors::SanitizeError;
use crate::types::{Metadata, SanitizeRequest, SanitizeResponse};

pub fn sanitize(_req: SanitizeRequest) -> Result<SanitizeResponse, SanitizeError> {
    Ok(SanitizeResponse {
        clean_text: String::new(),
        session_blob: String::new(),
        warnings: vec![],
        metadata: Metadata::default(),
    })
}
```

`crates/ghostwriter/src/restore.rs`:
```rust
//! Restore: strict exact token substitution.

use crate::errors::RestoreError;
use crate::types::{RestoreRequest, RestoreResponse};

pub fn restore(_req: RestoreRequest) -> Result<RestoreResponse, RestoreError> {
    Ok(RestoreResponse {
        restored_text: String::new(),
        warnings: vec![],
    })
}
```

Re-run: `cargo test -p ghostwriter`
Expected: 4 passed, 0 failed.

- [ ] **Step 5: Commit**

```bash
git add crates/ghostwriter/src/types.rs crates/ghostwriter/src/sanitize.rs crates/ghostwriter/src/restore.rs
git commit -m "[agent] feat: ghostwriter public data contracts

Step 2 of ghostwriter-v0.1: Context/Sanitize/Restore request
and response types with serde round-trip tests."
```

---

## Task 3: Error types

**Files:**
- Modify: `crates/ghostwriter/src/errors.rs`

- [ ] **Step 1: Write the failing test**

Append to `errors.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_error_display_contains_detector_failure() {
        let e = SanitizeError::DetectorFailure("simple nlp died".into());
        assert!(e.to_string().contains("detector"));
        assert!(e.to_string().contains("simple nlp died"));
    }

    #[test]
    fn restore_error_invalid_blob_is_distinct_from_missing() {
        let missing = RestoreError::MissingSessionBlob;
        let invalid = RestoreError::InvalidSessionBlob("base64 decode failed".into());
        assert_ne!(missing.to_string(), invalid.to_string());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ghostwriter errors::tests`
Expected: compile error — variants don't exist.

- [ ] **Step 3: Implement errors**

Replace `errors.rs` with:

```rust
//! Error types.
//!
//! Error surface mirrors the spec's "Sanitize Errors" and "Restore Errors"
//! sections. Non-fatal situations are expressed via `Warning` in types.rs,
//! not as errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SanitizeError {
    #[error("invalid request payload: {0}")]
    InvalidRequest(String),

    #[error("detector failure: {0}")]
    DetectorFailure(String),

    #[error("placeholder mapping failure: {0}")]
    PlaceholderMapping(String),

    #[error("blob encoding failure: {0}")]
    BlobEncoding(String),
}

#[derive(Debug, Error)]
pub enum RestoreError {
    #[error("missing session blob")]
    MissingSessionBlob,

    #[error("invalid session blob: {0}")]
    InvalidSessionBlob(String),

    #[error("invalid request payload: {0}")]
    InvalidRequest(String),
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ghostwriter errors::tests`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/ghostwriter/src/errors.rs
git commit -m "[agent] feat: ghostwriter error types

Step 3 of ghostwriter-v0.1: SanitizeError and RestoreError
covering the spec's failure surface."
```

---

## Task 4: Session blob (encode / decode)

**Files:**
- Modify: `crates/ghostwriter/src/blob.rs`

- [ ] **Step 1: Write the failing tests**

Write the full module with tests first:

```rust
//! Session blob schema.
//!
//! A `SessionBlob` carries the mapping from placeholder tokens back to raw
//! values. It is opaque to callers: we serialize to JSON, then wrap in
//! base64 so Laravel can transport it as a single string.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::errors::{RestoreError, SanitizeError};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionBlob {
    pub schema_version: u32,
    /// Map from placeholder token (e.g. "<CUSTOMER_NAME>") to raw value.
    /// Uses BTreeMap so the serialized form is deterministic.
    pub placeholders: BTreeMap<String, String>,
}

impl SessionBlob {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            placeholders: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, placeholder: impl Into<String>, raw: impl Into<String>) {
        self.placeholders.insert(placeholder.into(), raw.into());
    }

    pub fn encode(&self) -> Result<String, SanitizeError> {
        let json = serde_json::to_vec(self)
            .map_err(|e| SanitizeError::BlobEncoding(e.to_string()))?;
        Ok(B64.encode(json))
    }

    pub fn decode(s: &str) -> Result<Self, RestoreError> {
        if s.is_empty() {
            return Err(RestoreError::MissingSessionBlob);
        }
        let bytes = B64
            .decode(s.as_bytes())
            .map_err(|e| RestoreError::InvalidSessionBlob(format!("base64: {e}")))?;
        let blob: SessionBlob = serde_json::from_slice(&bytes)
            .map_err(|e| RestoreError::InvalidSessionBlob(format!("json: {e}")))?;
        if blob.schema_version != SCHEMA_VERSION {
            return Err(RestoreError::InvalidSessionBlob(format!(
                "unsupported schema_version {}",
                blob.schema_version
            )));
        }
        Ok(blob)
    }
}

impl Default for SessionBlob {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_blob_roundtrips() {
        let b = SessionBlob::new();
        let encoded = b.encode().unwrap();
        let decoded = SessionBlob::decode(&encoded).unwrap();
        assert_eq!(b, decoded);
    }

    #[test]
    fn blob_with_entries_roundtrips() {
        let mut b = SessionBlob::new();
        b.insert("<CUSTOMER_NAME>", "Markus Mueller");
        b.insert("<EMAIL_1>", "markus.mueller@example.de");
        let encoded = b.encode().unwrap();
        let decoded = SessionBlob::decode(&encoded).unwrap();
        assert_eq!(b, decoded);
        assert_eq!(
            decoded.placeholders.get("<CUSTOMER_NAME>").unwrap(),
            "Markus Mueller"
        );
    }

    #[test]
    fn decode_empty_string_returns_missing() {
        let err = SessionBlob::decode("").unwrap_err();
        matches!(err, RestoreError::MissingSessionBlob);
    }

    #[test]
    fn decode_invalid_base64_returns_invalid() {
        let err = SessionBlob::decode("not-base64-!!!").unwrap_err();
        assert!(matches!(err, RestoreError::InvalidSessionBlob(_)));
    }

    #[test]
    fn decode_rejects_wrong_schema_version() {
        let bad = serde_json::json!({
            "schema_version": 999,
            "placeholders": {}
        });
        let encoded = B64.encode(serde_json::to_vec(&bad).unwrap());
        let err = SessionBlob::decode(&encoded).unwrap_err();
        assert!(matches!(err, RestoreError::InvalidSessionBlob(_)));
    }

    #[test]
    fn encoding_is_deterministic() {
        let mut a = SessionBlob::new();
        a.insert("<B>", "second");
        a.insert("<A>", "first");
        let mut b = SessionBlob::new();
        b.insert("<A>", "first");
        b.insert("<B>", "second");
        assert_eq!(a.encode().unwrap(), b.encode().unwrap());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail then pass**

Run: `cargo test -p ghostwriter blob::tests`
Expected: compile, 6 passed. (Module body implements everything tests need.)

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/src/blob.rs
git commit -m "[agent] feat: ghostwriter session blob

Step 4 of ghostwriter-v0.1: base64+JSON SessionBlob with schema
version guard, deterministic ordering, and roundtrip tests."
```

---

## Task 5: Placeholder map (dedupe + numbering)

**Files:**
- Modify: `crates/ghostwriter/src/placeholder.rs`

- [ ] **Step 1: Write the failing tests**

```rust
//! Placeholder numbering + dedupe.
//!
//! `PlaceholderMap` owns the mapping from raw values to placeholder
//! tokens during a single sanitize call. It guarantees:
//!
//! - Same raw value within one call → same placeholder token.
//! - Numbering is per `PlaceholderKind` (EMAIL_1, EMAIL_2, PHONE_1, ...).
//! - Insertion order within a kind determines numbering.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaceholderKind {
    Email,
    Phone,
    Name,
    Address,
    Iban,
    Ip,
    GenericPii,
}

impl PlaceholderKind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Email => "EMAIL",
            Self::Phone => "PHONE",
            Self::Name => "NAME",
            Self::Address => "ADDRESS",
            Self::Iban => "IBAN",
            Self::Ip => "IP",
            Self::GenericPii => "PII",
        }
    }
}

#[derive(Debug, Default)]
pub struct PlaceholderMap {
    /// raw value → placeholder token (for dedupe).
    raw_to_token: HashMap<String, String>,
    /// token → raw value (for blob assembly and ordered listing).
    token_to_raw: Vec<(String, String)>,
    counters: HashMap<PlaceholderKind, u32>,
}

impl PlaceholderMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a KNOWN semantic placeholder (e.g. <CUSTOMER_NAME>).
    /// Does not consume a counter. If `token` already maps to a different
    /// raw value the original mapping wins — known context is inserted
    /// exactly once.
    pub fn insert_known(&mut self, token: &str, raw: &str) {
        if self.raw_to_token.contains_key(raw) {
            return;
        }
        self.raw_to_token.insert(raw.to_string(), token.to_string());
        self.token_to_raw.push((token.to_string(), raw.to_string()));
    }

    /// Lookup or allocate a typed placeholder for `raw` under `kind`.
    /// Same raw value returns the same token; a fresh raw value
    /// increments the per-kind counter.
    pub fn intern_typed(&mut self, kind: PlaceholderKind, raw: &str) -> String {
        if let Some(t) = self.raw_to_token.get(raw) {
            return t.clone();
        }
        let counter = self.counters.entry(kind).or_insert(0);
        *counter += 1;
        let token = format!("<{}_{}>", kind.prefix(), counter);
        self.raw_to_token.insert(raw.to_string(), token.clone());
        self.token_to_raw.push((token.clone(), raw.to_string()));
        token
    }

    pub fn token_for(&self, raw: &str) -> Option<&str> {
        self.raw_to_token.get(raw).map(String::as_str)
    }

    /// Ordered pairs (token, raw) in insertion order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.token_to_raw
    }

    pub fn token_list(&self) -> Vec<String> {
        self.token_to_raw.iter().map(|(t, _)| t.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_known_is_idempotent() {
        let mut m = PlaceholderMap::new();
        m.insert_known("<CUSTOMER_NAME>", "Markus Mueller");
        m.insert_known("<CUSTOMER_NAME>", "Markus Mueller");
        assert_eq!(m.entries().len(), 1);
    }

    #[test]
    fn intern_typed_allocates_sequential_numbering() {
        let mut m = PlaceholderMap::new();
        let a = m.intern_typed(PlaceholderKind::Email, "a@x.com");
        let b = m.intern_typed(PlaceholderKind::Email, "b@x.com");
        assert_eq!(a, "<EMAIL_1>");
        assert_eq!(b, "<EMAIL_2>");
    }

    #[test]
    fn intern_typed_dedupes_same_raw_value() {
        let mut m = PlaceholderMap::new();
        let a = m.intern_typed(PlaceholderKind::Email, "a@x.com");
        let a2 = m.intern_typed(PlaceholderKind::Email, "a@x.com");
        assert_eq!(a, a2);
    }

    #[test]
    fn counters_are_per_kind() {
        let mut m = PlaceholderMap::new();
        let e1 = m.intern_typed(PlaceholderKind::Email, "a@x.com");
        let p1 = m.intern_typed(PlaceholderKind::Phone, "+49 151 1");
        assert_eq!(e1, "<EMAIL_1>");
        assert_eq!(p1, "<PHONE_1>");
    }

    #[test]
    fn known_insertion_prevents_later_typed_collision() {
        let mut m = PlaceholderMap::new();
        m.insert_known("<CUSTOMER_EMAIL>", "m@x.com");
        let t = m.intern_typed(PlaceholderKind::Email, "m@x.com");
        assert_eq!(t, "<CUSTOMER_EMAIL>");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ghostwriter placeholder::tests`
Expected: 5 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/src/placeholder.rs
git commit -m "[agent] feat: ghostwriter placeholder map

Step 5 of ghostwriter-v0.1: PlaceholderKind + PlaceholderMap
providing dedupe and per-kind numbering for both stages."
```

---

## Task 6: Stage 1 — known context replacement

**Files:**
- Modify: `crates/ghostwriter/src/known_context.rs`

- [ ] **Step 1: Write the failing tests**

```rust
//! Stage 1: known context replacement.
//!
//! Replace exact string matches of customer_name / customer_email /
//! customer_phone with semantic placeholders BEFORE generic detection
//! runs. Multi-occurrence matches are all replaced.

use crate::placeholder::PlaceholderMap;
use crate::types::Context;

pub const CUSTOMER_NAME: &str = "<CUSTOMER_NAME>";
pub const CUSTOMER_EMAIL: &str = "<CUSTOMER_EMAIL>";
pub const CUSTOMER_PHONE: &str = "<CUSTOMER_PHONE>";

/// Replace known customer identity in `text` using exact string match.
/// Mutates `map` with the inserted known placeholders.
pub fn apply(text: &str, context: &Context, map: &mut PlaceholderMap) -> String {
    let mut out = text.to_string();
    if let Some(name) = context.customer_name.as_deref().filter(|s| !s.is_empty()) {
        if out.contains(name) {
            out = out.replace(name, CUSTOMER_NAME);
            map.insert_known(CUSTOMER_NAME, name);
        }
    }
    if let Some(email) = context.customer_email.as_deref().filter(|s| !s.is_empty()) {
        if out.contains(email) {
            out = out.replace(email, CUSTOMER_EMAIL);
            map.insert_known(CUSTOMER_EMAIL, email);
        }
    }
    if let Some(phone) = context.customer_phone.as_deref().filter(|s| !s.is_empty()) {
        if out.contains(phone) {
            out = out.replace(phone, CUSTOMER_PHONE);
            map.insert_known(CUSTOMER_PHONE, phone);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(n: Option<&str>, e: Option<&str>, p: Option<&str>) -> Context {
        Context {
            customer_name: n.map(String::from),
            customer_email: e.map(String::from),
            customer_phone: p.map(String::from),
        }
    }

    #[test]
    fn replaces_all_occurrences_of_customer_name() {
        let text = "Markus Mueller wrote to Markus Mueller";
        let mut map = PlaceholderMap::new();
        let out = apply(text, &ctx(Some("Markus Mueller"), None, None), &mut map);
        assert_eq!(out, "<CUSTOMER_NAME> wrote to <CUSTOMER_NAME>");
        assert_eq!(map.entries().len(), 1);
    }

    #[test]
    fn replaces_known_email_before_known_phone() {
        let text = "email m@x.com, call +49 151 1";
        let mut map = PlaceholderMap::new();
        let out = apply(
            text,
            &ctx(None, Some("m@x.com"), Some("+49 151 1")),
            &mut map,
        );
        assert_eq!(out, "email <CUSTOMER_EMAIL>, call <CUSTOMER_PHONE>");
    }

    #[test]
    fn missing_context_fields_are_ignored() {
        let text = "Hi there";
        let mut map = PlaceholderMap::new();
        let out = apply(text, &ctx(None, None, None), &mut map);
        assert_eq!(out, "Hi there");
        assert_eq!(map.entries().len(), 0);
    }

    #[test]
    fn absent_match_does_not_insert_placeholder() {
        let text = "Hi there";
        let mut map = PlaceholderMap::new();
        let out = apply(
            text,
            &ctx(Some("Markus Mueller"), None, None),
            &mut map,
        );
        assert_eq!(out, "Hi there");
        assert_eq!(map.entries().len(), 0);
    }

    #[test]
    fn spec_example_preserves_alternate_email_for_stage_two() {
        // From the spec example: alternate email stays raw after stage 1,
        // stage 2 will later tokenize it.
        let text = "Can you send it to markus.mueller@example.de instead of mueller.markus@icloud.com? Thanks, Markus Mueller";
        let mut map = PlaceholderMap::new();
        let out = apply(
            text,
            &ctx(
                Some("Markus Mueller"),
                Some("mueller.markus@icloud.com"),
                None,
            ),
            &mut map,
        );
        assert_eq!(
            out,
            "Can you send it to markus.mueller@example.de instead of <CUSTOMER_EMAIL>? Thanks, <CUSTOMER_NAME>"
        );
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ghostwriter known_context::tests`
Expected: 5 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/src/known_context.rs
git commit -m "[agent] feat: ghostwriter known-context replacement

Step 6 of ghostwriter-v0.1: stage 1 replaces customer_name,
customer_email, customer_phone with semantic placeholders."
```

---

## Task 7: Worka pii detector adapter

**Files:**
- Modify: `crates/ghostwriter/src/detect.rs`

**Context note:** Gaze already wraps the `pii` crate in `src/anon/detector_worka.rs`. Mirror that approach but produce ghostwriter's `Detection` shape. The `pii` crate exposes `Analyzer::new(nlp, recognizers, ad_hoc, policy)` and `analyzer.analyze(text, &language, &[])` returning `Vec<RecognizerResult>` with `start`, `end`, `entity_type`. Entity types include `Email`, `PhoneNumber`, `Person`, `Location`, `Iban`, `IpAddress`, etc.

- [ ] **Step 1: Write the failing tests**

```rust
//! Worka pii detector adapter.
//!
//! Ghostwriter uses `pii` as a detection primitive only. We translate its
//! `RecognizerResult` into our `Detection`, then `typed_unknown` decides
//! placeholder tokens from those detections.

use crate::errors::SanitizeError;
use crate::placeholder::PlaceholderKind;
use pii::nlp::SimpleNlpEngine;
use pii::types::{EntityType, Language};
use pii::{default_recognizers, Analyzer, PolicyConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub start: usize,
    pub end: usize,
    pub kind: PlaceholderKind,
    pub raw: String,
}

pub struct WorkaDetector {
    analyzer: Analyzer,
    language: Language,
}

impl WorkaDetector {
    pub fn new() -> Self {
        Self {
            analyzer: Analyzer::new(
                Box::new(SimpleNlpEngine::default()),
                default_recognizers(),
                Vec::new(),
                PolicyConfig::default(),
            ),
            language: Language::from("en"),
        }
    }

    /// Run detection across `text`. Returns detections sorted by start ASC,
    /// end DESC (so longer spans at the same start win when overlaps are
    /// resolved by callers).
    pub fn detect(&self, text: &str) -> Result<Vec<Detection>, SanitizeError> {
        let results = self
            .analyzer
            .analyze(text, &self.language, &[])
            .map_err(|e| SanitizeError::DetectorFailure(e.to_string()))?;

        let mut detections: Vec<Detection> = results
            .into_iter()
            .filter_map(|r| {
                let kind = map_entity(&r.entity_type)?;
                let raw = text.get(r.start..r.end)?.to_string();
                Some(Detection {
                    start: r.start,
                    end: r.end,
                    kind,
                    raw,
                })
            })
            .collect();

        detections.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        Ok(detections)
    }
}

impl Default for WorkaDetector {
    fn default() -> Self {
        Self::new()
    }
}

fn map_entity(ty: &EntityType) -> Option<PlaceholderKind> {
    match ty {
        EntityType::Email => Some(PlaceholderKind::Email),
        EntityType::PhoneNumber => Some(PlaceholderKind::Phone),
        EntityType::Person => Some(PlaceholderKind::Name),
        EntityType::Location => Some(PlaceholderKind::Address),
        EntityType::Iban => Some(PlaceholderKind::Iban),
        EntityType::IpAddress => Some(PlaceholderKind::Ip),
        // Unknown / unsupported entity types are skipped. Business-ID
        // leakage is acceptable in v1 per the spec.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plain_email() {
        let det = WorkaDetector::new();
        let out = det.detect("reach me at mueller@example.com please").unwrap();
        assert!(
            out.iter().any(|d| d.kind == PlaceholderKind::Email
                && d.raw == "mueller@example.com"),
            "expected email detection, got: {:?}",
            out
        );
    }

    #[test]
    fn detections_are_sorted_by_start() {
        let det = WorkaDetector::new();
        let out = det
            .detect("first a@x.com, then b@y.com, finally c@z.com")
            .unwrap();
        let starts: Vec<usize> = out.iter().map(|d| d.start).collect();
        let mut sorted = starts.clone();
        sorted.sort();
        assert_eq!(starts, sorted);
    }

    #[test]
    fn empty_text_returns_no_detections() {
        let det = WorkaDetector::new();
        let out = det.detect("").unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn text_with_no_pii_returns_no_detections() {
        let det = WorkaDetector::new();
        let out = det.detect("the quick brown fox jumps over the lazy dog").unwrap();
        // Some NLP backends may flag "fox" or similar — only assert no emails.
        assert!(out.iter().all(|d| d.kind != PlaceholderKind::Email));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ghostwriter detect::tests`
Expected: compile clean, 4 passed.

If compilation fails because the `pii` crate's `EntityType` variants don't match (e.g. variant is `EMAIL` not `Email`), refer to Gaze's own `src/anon/detector_worka.rs` for the exact variant names used by the pinned branch and adjust `map_entity`. The variant names are stable within the pinned branch.

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/src/detect.rs
git commit -m "[agent] feat: ghostwriter worka pii detector adapter

Step 7 of ghostwriter-v0.1: WorkaDetector produces Detection
structs mapped to PlaceholderKind, sorted by offset."
```

---

## Task 8: Stage 2 — typed unknown placeholder assignment

**Files:**
- Modify: `crates/ghostwriter/src/typed_unknown.rs`

- [ ] **Step 1: Write the failing tests**

```rust
//! Stage 2: typed unknown placeholders.
//!
//! Given detections over a (partially stage-1-substituted) text, replace
//! detected spans with typed placeholder tokens allocated from the shared
//! `PlaceholderMap`. Overlapping detections are resolved by taking the
//! first in sorted order and skipping any that start before the previous
//! end.

use crate::detect::Detection;
use crate::placeholder::PlaceholderMap;

pub fn apply(text: &str, detections: &[Detection], map: &mut PlaceholderMap) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor: usize = 0;
    let mut last_end: usize = 0;

    for d in detections {
        if d.start < last_end {
            // overlapping with a previously applied detection — skip
            continue;
        }
        if d.start < cursor {
            // sanity guard — detections must be sorted by start
            continue;
        }
        // Append verbatim text up to the detection.
        out.push_str(&text[cursor..d.start]);
        // Allocate (or reuse) a token for this raw value.
        let token = map.intern_typed(d.kind, &d.raw);
        out.push_str(&token);
        cursor = d.end;
        last_end = d.end;
    }
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placeholder::PlaceholderKind;

    fn det(start: usize, end: usize, kind: PlaceholderKind, raw: &str) -> Detection {
        Detection {
            start,
            end,
            kind,
            raw: raw.to_string(),
        }
    }

    #[test]
    fn replaces_single_email() {
        let text = "write a@x.com soon";
        let dets = vec![det(6, 13, PlaceholderKind::Email, "a@x.com")];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "write <EMAIL_1> soon");
    }

    #[test]
    fn repeated_value_reuses_same_token() {
        let text = "a@x.com then a@x.com";
        let dets = vec![
            det(0, 7, PlaceholderKind::Email, "a@x.com"),
            det(13, 20, PlaceholderKind::Email, "a@x.com"),
        ];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "<EMAIL_1> then <EMAIL_1>");
    }

    #[test]
    fn distinct_values_get_sequential_numbering() {
        let text = "a@x.com and b@y.com";
        let dets = vec![
            det(0, 7, PlaceholderKind::Email, "a@x.com"),
            det(12, 19, PlaceholderKind::Email, "b@y.com"),
        ];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "<EMAIL_1> and <EMAIL_2>");
    }

    #[test]
    fn mixed_kinds_get_independent_counters() {
        let text = "call +49 151 1 or mail a@x.com";
        let dets = vec![
            det(5, 14, PlaceholderKind::Phone, "+49 151 1"),
            det(23, 30, PlaceholderKind::Email, "a@x.com"),
        ];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "call <PHONE_1> or mail <EMAIL_1>");
    }

    #[test]
    fn overlapping_detections_later_one_is_dropped() {
        let text = "a@example.com";
        // Two overlapping detections: the full email and a sub-span.
        let dets = vec![
            det(0, 13, PlaceholderKind::Email, "a@example.com"),
            det(2, 13, PlaceholderKind::Email, "example.com"),
        ];
        let mut map = PlaceholderMap::new();
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "<EMAIL_1>");
    }

    #[test]
    fn known_placeholder_collision_reuses_semantic_token() {
        // If stage 1 already registered the customer email, stage 2
        // must reuse <CUSTOMER_EMAIL> for the same raw value.
        let text = "<CUSTOMER_EMAIL> and also m@x.com";
        let dets = vec![det(25, 33, PlaceholderKind::Email, "m@x.com")];
        let mut map = PlaceholderMap::new();
        map.insert_known("<CUSTOMER_EMAIL>", "m@x.com");
        let out = apply(text, &dets, &mut map);
        assert_eq!(out, "<CUSTOMER_EMAIL> and also <CUSTOMER_EMAIL>");
    }

    #[test]
    fn empty_detections_returns_text_unchanged() {
        let text = "nothing to see here";
        let mut map = PlaceholderMap::new();
        let out = apply(text, &[], &mut map);
        assert_eq!(out, text);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ghostwriter typed_unknown::tests`
Expected: 7 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/src/typed_unknown.rs
git commit -m "[agent] feat: ghostwriter stage 2 typed placeholders

Step 8 of ghostwriter-v0.1: typed_unknown::apply walks
detections, allocates tokens via PlaceholderMap, and
skips overlaps."
```

---

## Task 9: Sanitize orchestration

**Files:**
- Modify: `crates/ghostwriter/src/sanitize.rs`

- [ ] **Step 1: Write the failing tests**

Replace `sanitize.rs` entirely:

```rust
//! Sanitize orchestration.

use crate::blob::SessionBlob;
use crate::detect::WorkaDetector;
use crate::errors::SanitizeError;
use crate::known_context;
use crate::placeholder::PlaceholderMap;
use crate::typed_unknown;
use crate::types::{Metadata, SanitizeRequest, SanitizeResponse};

pub fn sanitize(req: SanitizeRequest) -> Result<SanitizeResponse, SanitizeError> {
    let detector = WorkaDetector::new();
    sanitize_with_detector(req, &detector)
}

pub fn sanitize_with_detector(
    req: SanitizeRequest,
    detector: &WorkaDetector,
) -> Result<SanitizeResponse, SanitizeError> {
    if req.text.is_empty() {
        // Empty input is valid per spec ("sanitize should not fail merely
        // because no PII was detected"). Return an empty response.
        return Ok(SanitizeResponse {
            clean_text: String::new(),
            session_blob: SessionBlob::new().encode()?,
            warnings: vec![],
            metadata: Metadata::default(),
        });
    }

    let mut map = PlaceholderMap::new();

    // Stage 1: known context replacement.
    let stage1 = known_context::apply(&req.text, &req.context, &mut map);

    // Stage 2: worka detection on the stage-1 output.
    let detections = detector.detect(&stage1)?;
    let stage2 = typed_unknown::apply(&stage1, &detections, &mut map);

    // Assemble session blob in insertion order.
    let mut blob = SessionBlob::new();
    for (token, raw) in map.entries() {
        blob.insert(token.clone(), raw.clone());
    }
    let session_blob = blob.encode()?;

    Ok(SanitizeResponse {
        clean_text: stage2,
        session_blob,
        warnings: vec![],
        metadata: Metadata {
            placeholders: map.token_list(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::SessionBlob;
    use crate::types::Context;

    fn req(text: &str, ctx: Context) -> SanitizeRequest {
        SanitizeRequest {
            text: text.to_string(),
            context: ctx,
        }
    }

    fn ctx_markus() -> Context {
        Context {
            customer_name: Some("Markus Mueller".into()),
            customer_email: Some("mueller.markus@icloud.com".into()),
            customer_phone: Some("+49 151 23456789".into()),
        }
    }

    #[test]
    fn empty_text_returns_empty_response() {
        let resp = sanitize(req("", Context::default())).unwrap();
        assert_eq!(resp.clean_text, "");
        assert!(resp.metadata.placeholders.is_empty());
    }

    #[test]
    fn spec_example_stage1_replaces_known_fields() {
        let text = "Hi Artistfy, Markus Mueller here. Please resend to mueller.markus@icloud.com. If needed call +49 151 23456789. Alternate email: markus.mueller@example.de";
        let resp = sanitize(req(text, ctx_markus())).unwrap();
        assert!(resp.clean_text.contains("<CUSTOMER_NAME>"));
        assert!(resp.clean_text.contains("<CUSTOMER_EMAIL>"));
        assert!(resp.clean_text.contains("<CUSTOMER_PHONE>"));
        // The alternate email must be tokenized by stage 2 into <EMAIL_1>.
        assert!(resp.clean_text.contains("<EMAIL_1>"));
        // Raw PII must not appear in clean_text.
        assert!(!resp.clean_text.contains("Markus Mueller"));
        assert!(!resp.clean_text.contains("mueller.markus@icloud.com"));
        assert!(!resp.clean_text.contains("+49 151 23456789"));
        assert!(!resp.clean_text.contains("markus.mueller@example.de"));
    }

    #[test]
    fn session_blob_decodes_and_contains_known_tokens() {
        let text = "Markus Mueller at mueller.markus@icloud.com";
        let resp = sanitize(req(text, ctx_markus())).unwrap();
        let blob = SessionBlob::decode(&resp.session_blob).unwrap();
        assert_eq!(
            blob.placeholders.get("<CUSTOMER_NAME>").unwrap(),
            "Markus Mueller"
        );
        assert_eq!(
            blob.placeholders.get("<CUSTOMER_EMAIL>").unwrap(),
            "mueller.markus@icloud.com"
        );
    }

    #[test]
    fn metadata_lists_placeholders_in_insertion_order() {
        let text = "Markus Mueller at mueller.markus@icloud.com and other@x.com";
        let resp = sanitize(req(text, ctx_markus())).unwrap();
        assert_eq!(resp.metadata.placeholders[0], "<CUSTOMER_NAME>");
        assert_eq!(resp.metadata.placeholders[1], "<CUSTOMER_EMAIL>");
        assert!(resp.metadata.placeholders.iter().any(|p| p == "<EMAIL_1>"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ghostwriter sanitize::tests`
Expected: 4 passed. If the detector fails to flag `markus.mueller@example.de` as an email on the pinned `pii` branch, inspect the detector output by adding a debug print temporarily, then adjust the assertion to only require that raw values are absent (the primary guarantee).

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/src/sanitize.rs
git commit -m "[agent] feat: ghostwriter sanitize orchestration

Step 9 of ghostwriter-v0.1: sanitize() composes stage 1,
worka detection, and stage 2 into a SanitizeResponse
with blob + metadata."
```

---

## Task 10: Restore (strict exact substitution)

**Files:**
- Modify: `crates/ghostwriter/src/restore.rs`

- [ ] **Step 1: Write the failing tests**

Replace `restore.rs` entirely:

```rust
//! Restore: strict exact token substitution.
//!
//! Per spec: restore only replaces exact placeholder tokens that exist in
//! the session blob. Paraphrased or invented text stays as written. Unused
//! placeholders in the blob become informational warnings.

use crate::blob::SessionBlob;
use crate::errors::RestoreError;
use crate::types::{RestoreRequest, RestoreResponse, Warning};

pub fn restore(req: RestoreRequest) -> Result<RestoreResponse, RestoreError> {
    let blob = SessionBlob::decode(&req.session_blob)?;

    let mut restored = req.text.clone();
    let mut used: Vec<String> = Vec::new();

    // Replace longest tokens first so that e.g. <EMAIL_11> is replaced
    // before <EMAIL_1> would be considered.
    let mut tokens: Vec<(&String, &String)> = blob.placeholders.iter().collect();
    tokens.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (token, raw) in tokens {
        if restored.contains(token.as_str()) {
            restored = restored.replace(token.as_str(), raw);
            used.push(token.clone());
        }
    }

    let mut warnings: Vec<Warning> = blob
        .placeholders
        .keys()
        .filter(|t| !used.contains(t))
        .map(|t| Warning::new(format!("placeholder {t} was not used")))
        .collect();

    // Look for placeholder-shaped tokens that survived because they are
    // NOT in the blob (e.g. the model invented <EMAIL_9>).
    for token in find_placeholder_tokens(&restored) {
        if !blob.placeholders.contains_key(&token) {
            warnings.push(Warning::new(format!(
                "unknown placeholder {token} left unchanged"
            )));
        }
    }

    Ok(RestoreResponse {
        restored_text: restored,
        warnings,
    })
}

/// Very small placeholder finder: matches `<UPPERCASE[_DIGITS_OR_UPPERCASE]*>`.
fn find_placeholder_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(end) = text[i..].find('>') {
                let token = &text[i..i + end + 1];
                let inner = &token[1..token.len() - 1];
                if !inner.is_empty()
                    && inner
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                {
                    out.push(token.to_string());
                }
                i += end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::SessionBlob;

    fn blob_with(pairs: &[(&str, &str)]) -> String {
        let mut b = SessionBlob::new();
        for (t, r) in pairs {
            b.insert(*t, *r);
        }
        b.encode().unwrap()
    }

    #[test]
    fn missing_session_blob_errors() {
        let err = restore(RestoreRequest {
            text: "hi".into(),
            session_blob: String::new(),
        })
        .unwrap_err();
        matches!(err, RestoreError::MissingSessionBlob);
    }

    #[test]
    fn corrupt_blob_errors() {
        let err = restore(RestoreRequest {
            text: "hi".into(),
            session_blob: "!!!not-base64!!!".into(),
        })
        .unwrap_err();
        assert!(matches!(err, RestoreError::InvalidSessionBlob(_)));
    }

    #[test]
    fn replaces_all_exact_placeholders() {
        let blob = blob_with(&[
            ("<CUSTOMER_NAME>", "Markus Mueller"),
            ("<CUSTOMER_EMAIL>", "mueller.markus@icloud.com"),
        ]);
        let resp = restore(RestoreRequest {
            text: "Hello <CUSTOMER_NAME>, we'll resend to <CUSTOMER_EMAIL>.".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(
            resp.restored_text,
            "Hello Markus Mueller, we'll resend to mueller.markus@icloud.com."
        );
        assert!(resp.warnings.is_empty());
    }

    #[test]
    fn unused_blob_placeholder_produces_warning() {
        let blob = blob_with(&[
            ("<CUSTOMER_NAME>", "Markus"),
            ("<EMAIL_1>", "a@x.com"),
        ]);
        let resp = restore(RestoreRequest {
            text: "Hello <CUSTOMER_NAME>".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(resp.restored_text, "Hello Markus");
        assert!(resp
            .warnings
            .iter()
            .any(|w| w.0.contains("<EMAIL_1>") && w.0.contains("not used")));
    }

    #[test]
    fn unknown_placeholder_shape_is_left_unchanged_with_warning() {
        let blob = blob_with(&[("<CUSTOMER_NAME>", "Markus")]);
        let resp = restore(RestoreRequest {
            text: "Hi <CUSTOMER_NAME>, contact <EMAIL_9>".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(resp.restored_text, "Hi Markus, contact <EMAIL_9>");
        assert!(resp
            .warnings
            .iter()
            .any(|w| w.0.contains("<EMAIL_9>") && w.0.contains("unknown")));
    }

    #[test]
    fn does_not_infer_from_nearby_words() {
        // The model drops the placeholder and writes "Markus" directly.
        // Restore must not touch "Markus" because it is not a token.
        let blob = blob_with(&[("<CUSTOMER_NAME>", "Markus Mueller")]);
        let resp = restore(RestoreRequest {
            text: "Hello Markus, see attached.".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(resp.restored_text, "Hello Markus, see attached.");
        assert!(resp.warnings.iter().any(|w| w.0.contains("not used")));
    }

    #[test]
    fn longer_tokens_replaced_before_shorter_prefixes() {
        let blob = blob_with(&[
            ("<EMAIL_1>", "a@x.com"),
            ("<EMAIL_11>", "k@y.com"),
        ]);
        let resp = restore(RestoreRequest {
            text: "write <EMAIL_11> then <EMAIL_1>".into(),
            session_blob: blob,
        })
        .unwrap();
        assert_eq!(resp.restored_text, "write k@y.com then a@x.com");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ghostwriter restore::tests`
Expected: 7 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/src/restore.rs
git commit -m "[agent] feat: ghostwriter restore

Step 10 of ghostwriter-v0.1: strict-exact token substitution
with longest-first ordering, unused-placeholder warnings,
and unknown placeholder detection."
```

---

## Task 11: Roundtrip integration test

**Files:**
- Create: `crates/ghostwriter/tests/roundtrip.rs`

- [ ] **Step 1: Write the failing test**

```rust
//! End-to-end: sanitize → restore → original values reappear.

use ghostwriter::{restore, sanitize, Context, RestoreRequest, SanitizeRequest};

fn ctx_markus() -> Context {
    Context {
        customer_name: Some("Markus Mueller".into()),
        customer_email: Some("mueller.markus@icloud.com".into()),
        customer_phone: Some("+49 151 23456789".into()),
    }
}

#[test]
fn spec_example_roundtrip() {
    let text = "Hi Artistfy, Markus Mueller here. Please resend to mueller.markus@icloud.com. If needed call +49 151 23456789. Alternate email: markus.mueller@example.de";

    let sanitized = sanitize(SanitizeRequest {
        text: text.into(),
        context: ctx_markus(),
    })
    .expect("sanitize");

    // clean_text has no raw PII
    assert!(!sanitized.clean_text.contains("Markus Mueller"));
    assert!(!sanitized.clean_text.contains("mueller.markus@icloud.com"));
    assert!(!sanitized.clean_text.contains("+49 151 23456789"));

    // Simulated LLM draft uses only exact placeholders.
    let draft = format!(
        "Hello <CUSTOMER_NAME>, we will resend the files to <CUSTOMER_EMAIL> today. If needed we will contact you at <CUSTOMER_PHONE>."
    );

    let restored = restore(RestoreRequest {
        text: draft,
        session_blob: sanitized.session_blob,
    })
    .expect("restore");

    assert_eq!(
        restored.restored_text,
        "Hello Markus Mueller, we will resend the files to mueller.markus@icloud.com today. If needed we will contact you at +49 151 23456789."
    );
}

#[test]
fn paraphrased_draft_leaves_invented_names_alone() {
    let sanitized = sanitize(SanitizeRequest {
        text: "Markus Mueller here".into(),
        context: ctx_markus(),
    })
    .unwrap();

    let restored = restore(RestoreRequest {
        text: "Hello Markus, see attached.".into(),
        session_blob: sanitized.session_blob,
    })
    .unwrap();

    assert_eq!(restored.restored_text, "Hello Markus, see attached.");
    assert!(restored
        .warnings
        .iter()
        .any(|w| w.0.contains("<CUSTOMER_NAME>") && w.0.contains("not used")));
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p ghostwriter --test roundtrip`
Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/tests/roundtrip.rs
git commit -m "[agent] test: ghostwriter sanitize/restore roundtrip

Step 11 of ghostwriter-v0.1: library-level end-to-end
matching the spec's worked example."
```

---

## Task 12: CLI binary (sanitize + restore subcommands)

**Files:**
- Modify: `crates/ghostwriter/src/main.rs`

- [ ] **Step 1: Write the CLI**

Replace `main.rs` entirely:

```rust
//! Ghostwriter CLI — sanitize and restore JSON requests over stdin/stdout.

use std::io::{self, Read, Write};

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};

use ghostwriter::{restore, sanitize, RestoreRequest, SanitizeRequest};

#[derive(Parser, Debug)]
#[command(name = "ghostwriter", version, about = "Deterministic PII sanitization for LLM prompts")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Read a SanitizeRequest JSON from stdin; write SanitizeResponse JSON to stdout.
    Sanitize,
    /// Read a RestoreRequest JSON from stdin; write RestoreResponse JSON to stdout.
    Restore,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Sanitize => run_sanitize(),
        Cmd::Restore => run_restore(),
    }
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf)
}

fn run_sanitize() -> Result<()> {
    let raw = read_stdin()?;
    let req: SanitizeRequest =
        serde_json::from_str(&raw).context("parsing SanitizeRequest JSON")?;
    let resp = sanitize(req).map_err(|e| anyhow::anyhow!("sanitize failed: {e}"))?;
    let json = serde_json::to_string(&resp).context("serializing SanitizeResponse")?;
    writeln!(io::stdout(), "{json}").context("writing stdout")?;
    Ok(())
}

fn run_restore() -> Result<()> {
    let raw = read_stdin()?;
    let req: RestoreRequest =
        serde_json::from_str(&raw).context("parsing RestoreRequest JSON")?;
    let resp = restore(req).map_err(|e| anyhow::anyhow!("restore failed: {e}"))?;
    let json = serde_json::to_string(&resp).context("serializing RestoreResponse")?;
    writeln!(io::stdout(), "{json}").context("writing stdout")?;
    Ok(())
}
```

- [ ] **Step 2: Build**

Run: `cargo build -p ghostwriter`
Expected: compiles, `target/debug/ghostwriter` exists.

- [ ] **Step 3: Smoke test manually**

Run:
```bash
echo '{"text":"Markus here","context":{"customer_name":"Markus"}}' | ./target/debug/ghostwriter sanitize
```
Expected: a single JSON line with `"clean_text":"<CUSTOMER_NAME> here"`, a non-empty `session_blob`, and `"placeholders":["<CUSTOMER_NAME>"]`.

- [ ] **Step 4: Commit**

```bash
git add crates/ghostwriter/src/main.rs
git commit -m "[agent] feat: ghostwriter CLI sanitize/restore

Step 12 of ghostwriter-v0.1: clap-based stdin/stdout CLI
with sanitize and restore subcommands."
```

---

## Task 13: CLI integration tests

**Files:**
- Create: `crates/ghostwriter/tests/cli.rs`

- [ ] **Step 1: Write the failing tests**

```rust
//! CLI integration tests via assert_cmd.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::json;

fn ghostwriter() -> Command {
    Command::cargo_bin("ghostwriter").expect("binary built")
}

#[test]
fn sanitize_replaces_customer_name() {
    let req = json!({
        "text": "Hi Markus Mueller, please reply",
        "context": { "customer_name": "Markus Mueller" }
    })
    .to_string();

    let assert = ghostwriter()
        .arg("sanitize")
        .write_stdin(req)
        .assert()
        .success();

    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let resp: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        resp["clean_text"], "Hi <CUSTOMER_NAME>, please reply"
    );
    assert!(resp["session_blob"].as_str().unwrap().len() > 0);
    assert_eq!(resp["metadata"]["placeholders"][0], "<CUSTOMER_NAME>");
}

#[test]
fn sanitize_then_restore_roundtrip_via_cli() {
    // sanitize
    let req = json!({
        "text": "Markus Mueller wrote from mueller@icloud.com",
        "context": {
            "customer_name": "Markus Mueller",
            "customer_email": "mueller@icloud.com"
        }
    })
    .to_string();

    let assert = ghostwriter()
        .arg("sanitize")
        .write_stdin(req)
        .assert()
        .success();
    let sanitize_out: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let blob = sanitize_out["session_blob"].as_str().unwrap().to_string();

    // restore using an LLM-style draft with exact placeholders
    let draft = "Hello <CUSTOMER_NAME>, we received your note at <CUSTOMER_EMAIL>.";
    let restore_req = json!({ "text": draft, "session_blob": blob }).to_string();

    let assert = ghostwriter()
        .arg("restore")
        .write_stdin(restore_req)
        .assert()
        .success();
    let restore_out: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(
        restore_out["restored_text"],
        "Hello Markus Mueller, we received your note at mueller@icloud.com."
    );
}

#[test]
fn invalid_json_stdin_exits_nonzero() {
    ghostwriter()
        .arg("sanitize")
        .write_stdin("not json at all")
        .assert()
        .failure()
        .stderr(predicate::str::contains("parsing SanitizeRequest JSON"));
}

#[test]
fn version_flag_prints_version() {
    ghostwriter()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("ghostwriter"));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ghostwriter --test cli`
Expected: 4 passed. (`assert_cmd` auto-builds the binary before running.)

- [ ] **Step 3: Commit**

```bash
git add crates/ghostwriter/tests/cli.rs
git commit -m "[agent] test: ghostwriter CLI integration tests

Step 13 of ghostwriter-v0.1: assert_cmd roundtrip tests
covering sanitize, restore, bad JSON, and --version."
```

---

## Task 14: Local install via cargo install

**Files:**
- None (verification step that produces a binary on the user's machine)

- [ ] **Step 1: Run full test suite**

Run: `cargo test -p ghostwriter`
Expected: all ghostwriter tests pass.

- [ ] **Step 2: Release build**

Run: `cargo build -p ghostwriter --release`
Expected: `target/release/ghostwriter` exists.

- [ ] **Step 3: Install to ~/.cargo/bin**

Run from the worktree root:
```bash
cargo install --path crates/ghostwriter --force
```
Expected: `Installed package 'ghostwriter v0.1.0' (...)`; binary at `~/.cargo/bin/ghostwriter`.

- [ ] **Step 4: Verify the installed binary**

Run:
```bash
ghostwriter --version
```
Expected: `ghostwriter 0.1.0`.

Run:
```bash
echo '{"text":"Markus writes","context":{"customer_name":"Markus"}}' | ghostwriter sanitize
```
Expected: a single JSON line where `clean_text` is `<CUSTOMER_NAME> writes`.

- [ ] **Step 5: No commit** — this task only produces binaries on the user's machine. Proceed to Task 15 without committing.

---

## Task 15: Homebrew formula (naoray/homebrew-tap)

**Files:**
- Create: `dist/homebrew/ghostwriter.rb`
- Create: `dist/homebrew/README.md`

The reference pattern is `naoray/homebrew-tap/Formula/scribe.rb` which uses GoReleaser-built prebuilt binaries. For Rust we use `cargo` build-from-source until cargo-dist is wired up. The formula below is deliberately source-based so the user can `brew install` immediately after pushing the branch without any release pipeline.

- [ ] **Step 1: Create the formula**

Create `dist/homebrew/ghostwriter.rb`:

```ruby
# frozen_string_literal: true

# Ghostwriter — deterministic text sanitization + exact restoration for LLM prompts.
# Source-build variant. Once cargo-dist is wired up, replace this with
# prebuilt binaries per the scribe.rb pattern.
class Ghostwriter < Formula
  desc "Deterministic PII sanitization + exact restoration for LLM prompts"
  homepage "https://github.com/naoray/gaze"
  url "https://github.com/naoray/gaze/archive/refs/heads/ghostwriter-v0.1.tar.gz"
  version "0.1.0"
  license "Apache-2.0"
  head "https://github.com/naoray/gaze.git", branch: "ghostwriter-v0.1"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/ghostwriter")
  end

  test do
    assert_match "ghostwriter 0.1.0", shell_output("#{bin}/ghostwriter --version")
    out = pipe_output(
      "#{bin}/ghostwriter sanitize",
      '{"text":"Hi Markus","context":{"customer_name":"Markus"}}'
    )
    assert_match "<CUSTOMER_NAME>", out
  end
end
```

Note: the `url` field is a placeholder. The user will:
1. Push `ghostwriter-v0.1` to `origin` on github.com/naoray/gaze.
2. Either tag a release (`v0.1.0-ghostwriter`) and point `url` at the tag tarball, or keep `head` as the install path (`brew install --HEAD naoray/tap/ghostwriter`).

- [ ] **Step 2: Write tap instructions**

Create `dist/homebrew/README.md`:

```markdown
# Ghostwriter Homebrew Formula

This formula installs `ghostwriter` from source using `cargo`.

## Publish to naoray/homebrew-tap

1. Push the `ghostwriter-v0.1` branch (or its merged main commit) to GitHub:

   ```bash
   git push origin ghostwriter-v0.1
   ```

2. In a clone of `naoray/homebrew-tap`, copy this formula into `Formula/`:

   ```bash
   cp dist/homebrew/ghostwriter.rb /path/to/homebrew-tap/Formula/ghostwriter.rb
   ```

3. Update the `url` field to point at a specific tag tarball once a release
   tag exists. Until then, users can install HEAD:

   ```bash
   brew tap naoray/tap
   brew install --HEAD naoray/tap/ghostwriter
   ```

4. Commit and push to the tap repo.

## Local dev install (no tap required)

```bash
cargo install --path crates/ghostwriter --force
```

This installs `ghostwriter` into `~/.cargo/bin`.

## Smoke test

```bash
echo '{"text":"Hi Markus Mueller","context":{"customer_name":"Markus Mueller"}}' \
  | ghostwriter sanitize
```
```

- [ ] **Step 3: Commit**

```bash
git add dist/homebrew/ghostwriter.rb dist/homebrew/README.md
git commit -m "[agent] build: ghostwriter homebrew formula

Step 15 of ghostwriter-v0.1: source-build Homebrew formula
following the naoray/homebrew-tap scribe.rb pattern,
plus instructions for publishing to the tap."
```

---

## Task 16: Plan-to-spec self-check

**Files:**
- None (documentation task)

Re-read `docs/superpowers/specs/2026-04-11-ghostwriter-sanitization-design.md` with the implementation in hand. For each section below, confirm the behavior is observable in the code:

- [ ] **Spec: Sanitize**
  - Stage 1 known context replacement — `known_context::apply`
  - Stage 2 worka detection + typed placeholders — `detect::WorkaDetector` + `typed_unknown::apply`
  - Returns clean_text + session_blob + warnings + metadata — `sanitize::sanitize`

- [ ] **Spec: Restore**
  - Strict exact placeholder substitution only — `restore::restore`
  - Unused placeholder warnings — covered in restore tests
  - Unknown placeholder-shape warnings — `find_placeholder_tokens`

- [ ] **Spec: Placeholder Strategy**
  - Known customer identity → `<CUSTOMER_NAME>` / `<CUSTOMER_EMAIL>` / `<CUSTOMER_PHONE>` — known_context.rs
  - Typed per-kind numbering (`<EMAIL_1>`, `<PHONE_1>`, ...) — placeholder.rs
  - Stable per-message dedupe — placeholder.rs tests

- [ ] **Spec: Session Blob**
  - Opaque from caller perspective — base64(json)
  - Reject corrupt payloads — blob.rs tests

- [ ] **Spec: Error Handling**
  - Sanitize fails only on structural or detector errors — SanitizeError variants
  - Restore fails only on missing/corrupt blob or invalid payload — RestoreError variants
  - No-PII case returns original text with empty blob — sanitize tests

- [ ] **Spec: Success Criteria**
  - Markus can drive the Laravel side with only primary customer identity + blob pass-through — CLI accepts exactly that shape
  - Determinism — blob encoding is sorted; placeholder numbering is insertion-order stable
  - No restore guessing — restore tests

If any row has no matching code or test, file a follow-up task and implement it before declaring the plan complete.

No commit for this task — it is a review gate only.

---

## Task 17: Final verification + push

**Files:**
- None

- [ ] **Step 1: Run the full workspace test suite**

Run: `cargo test -p ghostwriter`
Expected: all pass.

Run: `cargo build -p ghostwriter --release`
Expected: clean release build.

- [ ] **Step 2: Confirm installed binary still works**

Run:
```bash
ghostwriter --version
ghostwriter --help
```

- [ ] **Step 3: Decide on push + PR**

The branch `ghostwriter-v0.1` has not been pushed to origin yet. Ask the user before pushing — per project push cadence, pushes happen per milestone and require explicit approval. Do not push without confirmation.

If the user approves:

```bash
git push -u origin ghostwriter-v0.1
```

Then follow up per the "Homebrew formula publish" steps in `dist/homebrew/README.md`.

---

## Self-Review Notes

- Task 1 uses stub module bodies so `lib.rs` re-exports compile before later tasks fill in real content.
- Task 2 rewrites the stub `sanitize`/`restore` functions so they keep compiling against the new real types before Task 9/10 replace them again. This keeps the build green between tasks.
- Task 7 depends on the exact `pii::types::EntityType` variant names from the pinned branch. The implementer should reference `src/anon/detector_worka.rs` in the same repo if variants differ — that file already uses the same git dep and confirms the variant spelling.
- Task 11 (roundtrip) assumes `markus.mueller@example.de` is tokenized by stage 2 only for the "alternate email" assertion. If the pinned `pii` version does not detect that address, relax the test to check only that raw PII is absent — that is the primary guarantee.
- Task 15 formula is source-build (`cargo install`). When cargo-dist is introduced later it replaces this formula with a prebuilt-binary variant mirroring `scribe.rb` exactly.
- Business identifiers (order IDs, invoice IDs, tracking numbers) are intentionally left alone per the spec's "Business Identifiers" section. No task targets them.
