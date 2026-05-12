# AGENTS.md — Gaze project agent context

**All AI coding agents operating on this repo (Codex, Claude, Cursor, Gemini, etc.) should read this file at session start and apply the rules below.**

This file is the canonical agent-context for Gaze. `CLAUDE.md` defers to it for shared rules and adds only agent-specific addenda.

## Project north star

> **Gaze is the most reliable, reversible PII pseudonymization runtime for agentic workflows. Zero PII leaks between the agent and the data owner — ever. Any byte of PII that reaches an LLM outside the manifest contract is a critical defect.**

Verbatim user directive (2026-04-24): *"set a north star to be focused on never leaking any PII data and making this lib the best PII [pseudonymization] there is for agentic interaction with information"*.

"Pseudonymization" is the GDPR Art. 4(5) term for reversible substitution with tokens — chosen over "redaction" (one-way, loses the restore moat), "obscuring" (vague), and "tokenization" (overloaded with payment industry usage).

## The five axes

Every design, implementation, and review decision MUST be evaluated against these axes.

1. **Reliability (never leak).** Fail-closed always. Defense in depth (regex + NER + dictionary + optional neural safety net). Every known detection gap is a todo; every leak incident is a postmortem + fix pattern baked into skill/memory.
2. **Reversibility.** Manifest-first restore. Format-preserving tokens stay restorable. No one-way primitives in the core contract. Anything that breaks restore round-trip is a design regression.
3. **Agentic-first.** Decisions prioritize agent workflow needs over generic text handling — tool-call JSON embedding, streaming LLM, multi-turn sessions with evolving context, tenant-specific PII (songs, order IDs, artist names).
4. **Trust (auditable + deterministic).** Rule-based detectors preferred over neural for precise classes. Neural is an addon (safety net, free-text NER), not the floor. Every token emission traceable to a rule or recognizer. Typed exceptions + closed error-variant set. No silent mismatches.
5. **Adopter ergonomics.** Low-friction integration (Laravel adapter pattern, clear TOML policy, sane defaults). Framework adapters pave the 80% case; library API serves the 20% power case. Adopter can pick Gaze up in under a day without deep PII domain expertise.

All design, implementation, and review decisions in this repo are evaluated against the five axes above. If a decision weakens any axis, call it out in the PR description and justify the tradeoff. Correctness axes 1–4 always beat performance.

## Workspace shape (v0.7)

As of v0.7.1, the workspace has nine published-shape crates plus `xtask`:

- `gaze` — core (pipeline, session, policy, registry, locale, rulepack). Re-exports `gaze_types::RedactionLogger` for source-compat. No `rusqlite` dep in any feature graph.
- `gaze-types` — shared value contracts including the canonical `RedactionLogger` trait (serde-only, no ML/sql deps). Introduced in v0.5 Phase B.
- `gaze-recognizers` — regex/dictionary/NER detection backends + embedded rulepacks.
- `gaze-audit` — passive SQLite sink + audit-query API. `rusqlite` lives only here. Introduced in v0.5 Phase C.
- `gaze-assembly` — policy-to-pipeline builder for CLI-style adopters.
- `gaze-cli` — standalone `gaze` binary; only allowlisted `gaze-audit` consumer outside compatibility tests.
- `gaze-mcp-core` — transport-free MCP-shaped chokepoint runtime (`Tool` trait, sealed `ToolCtx`, `ToolRegistry`, `PiiEnvelope::dispatch`, `Frontend`/`DispatchHost`, `ManifestStore`, `AuthHook`, `SessionIdPolicy`). Introduced in v0.7.0.
- `gaze-mcp-rmcp` — rmcp transport sink: `RmcpFrontend`, stdio default transport, opt-in streamable HTTP transport, adopter-supplied `PrincipalResolver`. Introduced in v0.7.0.
- `gaze-document` — OSS document ingestion: PNG/JPG/PDF → Tesseract OCR → gaze redact → `SafeBundle` (`clean.md`, `manifest.json`, `report.json`). Introduced in v0.7.1.
- `xtask` (+ `xtask/dylint/`) — internal gate runner; the Dylint `gaze_module_isolation` lint hosted in `xtask/dylint/` is a detached workspace pinned to `nightly-2025-09-18`. The legacy `audit-metadata-only` syn walker was decommissioned in v0.5 Phase E.

Source-of-truth workspace shape table with full role descriptions: [`CONTRIBUTING.md`](./CONTRIBUTING.md#workspace-shape).

## Universal rules (ALL agents)

1. **Never weaken an axis without an explicit PR-body note.** If a change regresses reliability, reversibility, agentic fit, trust, or adopter ergonomics — say so in the PR description and justify the tradeoff. Correctness axes 1–4 always beat performance.
2. **Never leak PII in examples, tests, or fixtures.** Use `alice@example.invalid` / `Dr. Schmidt` / `<Email_1>` — never real PII, even in docs. Phone numbers MUST come from documented synthetic-non-reachable ranges: NANPA `+1-555-01xx` for US, Ofcom `+44-7700-900xxx` for UK, and `+49 1555 0112233`-style mobile shapes for DE (the parser-backed v0.4.5 S2 national-phone recognizers accept these without using real BNetzA-assigned ranges). See [`CONTRIBUTING.md`](./CONTRIBUTING.md#phone-number-fixtures) for the canonical rule + rationale.
3. **Commit discipline:** `[agent]` prefix on every commit. Stage specific files by name. No `git add -A` or `git add .`. No amend, no force-push, no `--no-verify`. Commit after each logical phase, not only at the end.
4. **Branch per task.** Work on a dedicated branch; keep `main` clean.
5. **Completion signaling:** every agent brief includes a sentinel line (e.g. `IMPL DONE:`, `REVIEW DONE:`, `DOCS DONE:`). Print it on the final stdout line.

## Source of truth

This file is the canonical agent-context for Gaze. `CLAUDE.md` defers to it for shared rules and adds only agent-specific addenda.
