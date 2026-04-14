# Gaze v0.2 — Channel-Agnostic Redaction Engine

## Prompt

Design a spec for restructuring Gaze from "MySQL+logs debug proxy" into a channel-agnostic redaction engine.

## Core Reframe

Gaze is the **black marker** on information an agent needs to work with but isn't allowed to see raw. It intercepts any information (structured or unstructured) from any channel (DB, logs, emails, files, API responses), detects PII, and outputs an anonymized version with session-scoped tokens.

The current debug proxy (MCP server for MySQL + Laravel logs) becomes one **product** built on top of the Gaze engine. Ghostwriter is another. Future products: email interceptor, file scanner, API response proxy, sandbox-mode executor.

## Key Architectural Decisions to Explore

### 1. Core abstraction
Current: `RawRow` (structured) + `LogLine` (unstructured) — two separate types, two separate redaction paths.
Proposed: `RawDocument` enum with structured/unstructured variants → single `Redactor` pipeline → `CleanDocument` + session context.

### 2. Workspace layout
```
crates/
  gaze/           ← core engine (redact, detect, session, audit)
  ghostwriter/    ← gaze consumer (sanitize/restore for LLM conversations)
  debug-proxy/    ← gaze consumer (current src/ — MCP, MySQL, SSH, CLI)
```

### 3. What moves into core vs stays in consumers

**Core (gaze crate):**
- `RawDocument` / `CleanDocument` types
- Detection pipeline: column rules + regex + NER (Worka) in one chain
- `SessionKey` + `SessionMap` + HMAC pseudonymization
- `Replacer` — works on both text fragments and structured field values
- Audit trail interface
- No I/O, no protocol knowledge, no channel awareness

**Consumer (debug-proxy):**
- `DatabaseAdapter` trait + MySQL impl
- `LogAdapter` trait + Laravel impl
- SSH tunnel lifecycle
- MCP protocol (rmcp) + tool handlers
- CLI (clap) — init/check/serve/audit
- Policy TOML parser (or this moves to core?)

**Consumer (ghostwriter):**
- CLI (JSON stdin/stdout)
- Session blob format (JSON+base64)
- Known-context replacement (`<CUSTOMER_*>`)
- Restore logic

### 4. Open questions
- **Session ownership:** Does core manage sessions, or do consumers create/pass keys?
- **Audit log:** Core capability or consumer responsibility?
- **Policy/config:** Core defines schema, consumers extend? Or purely consumer-side?
- **Restore/reverse-lookup:** Core capability (bidirectional session map already exists) or consumer-specific?
- **Detection pipeline config:** How does a consumer specify what to detect? Trait? Builder pattern?
- **Log redaction session awareness:** Currently Scanner has no session — unifying means log tokens become session-scoped pseudonyms, enabling cross-channel correlation.

### 5. Sandbox mode idea (future)
Agent executes code → Gaze-capable product runs it → if results contain PII, stores raw in parent directory agent can't access → returns anonymized version to agent. Combines Gaze redaction with execution sandboxing.

## What stays the same
- `SessionKey` + `SessionMap` + HMAC approach — sound for both paths
- `PiiDetector` trait — already abstract enough
- Type-safety invariant (raw never serializable) — just broader scope
- Memory hygiene (mlock, zeroize)

## Context from v0.1
- Gaze v0.1 spec: `docs/superpowers/specs/2026-04-10-gaze-design.md`
- Ghostwriter spec: `docs/superpowers/specs/2026-04-11-ghostwriter-sanitization-design.md`
- Gaze v0.1 plan: `docs/superpowers/plans/2026-04-11-gaze-v0.1.md`
- Ghostwriter plan: `docs/superpowers/plans/2026-04-11-ghostwriter-v0.1.md`
