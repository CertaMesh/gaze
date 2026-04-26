# Gaze — Threat Model

**Status:** Draft
**Date:** 2026-04-15
**Scope:** v0.2 read-side + v0.5 operations proxy

---

## Assets

1. **Raw PII** — customer names, emails, phones, addresses, IDs, order numbers, domain-specific strings (song titles, artist names). Legally protected under DSGVO/GDPR.
2. **Session HMAC key** — 32 random bytes. Grants ability to *generate* new tokens linkable to a session.
3. **SessionSnapshot** (reversal map) — bidirectional `(class, raw) ↔ token` map without the key. Possession ⇒ full PII recovery. **As sensitive as raw PII.**
4. **Audit / redaction log** — what was redacted, when, by which detector. Must never contain raw values or token↔value pairs (EDPB 01/2025).

## Trust Zones

| Zone | Trust | Notes |
|------|-------|-------|
| Agent (LLM + host) | **Untrusted** | Adversarial prompt, adversarial output, adversarial tool calls. Never sees raw PII. |
| Gaze core + consumer crate | **Trusted** | Runs with DB/file credentials. Handles raw PII. |
| Session store (Laravel DB, disk) | **Semi-trusted** | Assumed AEAD-encrypted at rest; key managed outside Gaze. |
| Data sources (MySQL, log files, email) | **Trusted** (source of truth) | |
| Sandbox kernel (v0.5: nono/birdcage/Landlock/Seatbelt) | **Trusted** | Enforces read-allow on clean paths, deny on raw. |

## Adversaries

### A1 — Curious LLM Provider

**Capability:** Reads every prompt Gaze's consumer sends to the LLM. Logs indefinitely. May be subpoenaed.

**Goal:** Re-identify individuals or correlate activity across sessions/customers.

**Mitigations:**
- Tokens in prompts only, never raw PII.
- Per-session random HMAC key ⇒ same individual gets different tokens in different sessions (cross-session unlinkability).
- Token format (`Person_7`) carries no structural info about the original value.
- `Action::FormatPreserve` is opt-in and deferred (v0.3); default is `Tokenize` for sensitive classes because format-preserving fakes leak local-part length / domain distribution (measurable NoPII-style leakage).

**Residual risk:** If consumer reuses one session across many turns, the cumulative map becomes a richer re-ID surface if later leaked. → See A3.

### A2 — Malicious / Compromised Agent

**Capability:** Chooses which queries to run, what arguments to pass. Can craft output designed to manipulate downstream code. In v0.5, invokes `gaze exec` with arbitrary argv/env.

**Goal (v0.2 read side):** Extract raw PII via inference — narrowing queries (`WHERE city='X' AND age=Y`), compositional attacks across tools, SQL literal injection of known partial values.

**Goal (v0.5 action side):** Trick Gaze into executing unauthorized actions; exfiltrate data via action side-effects (send email to attacker-controlled address using legitimate Token).

**Mitigations (in scope for v0.2):**
- Policy-level allowlists for tables/columns in external MCP consumers such as [piinuts/glance](https://github.com/PIInuts/glance), formerly the in-tree `debug-proxy`.
- Query budgets + k-anonymity thresholds (open issues #1, #2 — deferred to v0.3, documented here as known gap).
- Filter translation validated: agent-supplied tokens go through `session.restore()` with **fail-closed** semantics on unknown tokens (action-phase; read-phase may warn + pass through).
- All detector output goes through pipeline; no raw PII path bypasses redaction.
- Unicode normalization (NFC + zero-width-joiner strip) on every input before detection — defeats obfuscation attacks where adversary embeds zero-width chars to bypass regex/NER.

**Mitigations (v0.5):**
- `Sandbox` trait (pluggable: birdcage default, nono upgrade path). Agent has `exec` on gaze binary, no `read` on raw PII paths.
- Every argument passed to `gaze exec` is validated as untrusted input: script path whitelist, shell metachar rejection, env var allowlist. Gaze runs trusted but parses untrusted input.
- All action output (stdout + stderr + structured errors) goes through `pipeline.redact()` before returning to agent. DB constraint errors quoting values, SMTP bounces with raw addresses, etc. must not leak.

**Residual risk:** Statistical inference from k-anon-compliant results + external knowledge. Accepted; documented for consumers.

### A3 — On-Path / At-Rest Attacker

**Capability:** Steals the SessionSnapshot blob from the consumer's store (Laravel DB, disk cache).

**Goal:** Recover PII from stored snapshots.

**Mitigations:**
- `Session::export()` returns **opaque signed bytes** (`SensitiveSnapshot(Vec<u8>)`), not a structured type. Signature binds the blob to its creation event; tampering is detected on import.
- Doc-level invariant: `SessionSnapshot is as sensitive as raw PII. Consumers MUST encrypt at rest and in transit.` (No longer "safe to serialize".)
- Version byte in wire format to allow future crypto rotation without breaking stored blobs.
- Recommended deployment: AEAD envelope (AES-GCM / XChaCha20-Poly1305) with key outside Gaze (Laravel APP_KEY, HSM, KMS).
- Session scope/TTL: consumers declare `Session::scope` = `Ephemeral | Conversation(id) | Persistent`. Persistent sessions must set an explicit TTL; snapshot rotation recommended.

**Residual risk:** Consumer misconfig (no encryption, shared APP_KEY). Documented fail-loud on detection where possible.

### A4 — Supply Chain

**Capability:** Pushes malicious code to an upstream dependency between builds.

**Goal:** Exfiltrate raw PII during detection (NER crate logs raw input, regex crate sends spans to attacker, etc.).

**Mitigations:**
- NER backing is `ort` + `tokenizers` with pinned local ONNX artifacts (Phase 0). All Git deps pinned to rev SHA, not branch.
- `cargo-deny` / `cargo-audit` in CI.
- Core has **zero network I/O**; any detector attempting to open sockets would fail in v0.5 sandbox even if compromised.
- Long-term: fork critical deps, vendor.

**Residual risk:** Compromise of rustc/cargo toolchain. Out of scope.

### A5 — Prompt Injection into Detection Path

**Capability:** Attacker crafts data that reaches Gaze's detectors (e.g., user-controlled support-message content, log lines with attacker-controlled payloads).

**Goal:** Hide real PII from detection OR inject fake tokens the LLM will later treat as authoritative.

**Mitigations:**
- Unicode normalization pre-pass (NFC, ZWJ/ZWNJ strip, full-width ↔ ASCII) before detector stack.
- Detector output is authoritative only for *spans* it owns; overlapping detector disagreement (different `PiiClass` on same span) is resolved deterministically (longest wins, first-on-tie) and **the losing detection is logged via the auditor** for detection-QA.
- Agent-emitted tokens that don't exist in `session.restore()` are rejected in action phase (fail-closed).

## Non-Goals

- **Side-channel attacks** against memory (cold-boot, Spectre). Mitigated best-effort via `secrecy` + `mlock` + `MADV_DONTDUMP`; not hardened.
- **Root compromise** of the host running Gaze. If the attacker is root, game over.
- **Quantum attackers.** HMAC-SHA-256 assumed secure; rotate if NIST guidance changes.
- **Model-stealing / prompt extraction** at the LLM provider — property of the provider, not Gaze.

## Open Questions → Mapped to Issues

| Adversary | Gap | Tracked |
|-----------|-----|---------|
| A2 (malicious agent, v0.2) | k-anonymity / cardinality guards | Issue #1 — deferred to v0.3 |
| A2 (compositional attack) | per-session query budget | Issue #2 — deferred to v0.3 |
| A3 (at-rest) | audit log must not enable reversal | Issue #3 — **closed by core Auditor contract** (v0.2) |
| A5 (prompt injection into ghostwriter) | typed terms for customer-specific values | Issue #4 — IndexDetector in v0.2 |

## Review Cadence

Re-review this document at each milestone gate (v0.2 ship, v0.3 ship, v0.5 design freeze). Any new consumer product triggers a fresh pass.
