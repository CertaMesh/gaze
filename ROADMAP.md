---
title: Gaze roadmap
status: live
audience: humans + AI agents
---

# Gaze roadmap

This is the single live roadmap for Gaze. It replaces the former split between
`docs/ROADMAP.md` and per-version files under `docs/roadmap/`; release work
should keep this root-level file current while detailed specs and architecture
notes stay in the relevant `docs/research/` or `docs/architecture/` files.

## North star (quick reference)

> **Gaze is the most reliable, reversible PII pseudonymization runtime for agentic workflows. Zero PII leaks between the agent and the data owner — ever. Any byte of PII that reaches an LLM outside the manifest contract is a critical defect.**

Every roadmap decision is evaluated against five axes:
**reliability · reversibility · agentic-first · trust · adopter ergonomics**.
Correctness axes 1–4 always beat performance. "Pseudonymization" is the GDPR
Art. 4(5) term for reversible substitution with tokens, chosen over
"redaction", "obscuring", and overloaded payment-industry "tokenization".

## Now (in flight)

v0.4.6 brainstorm pending.

Post-v0.4.5, there is no active implementation scope locked in this roadmap.
The next cycle should start with brainstorm → plan → multi-review before
dispatching implementation work.

## Next (committed but not started)

These items are committed candidates for the v0.4.6 cycle unless the brainstorm
explicitly reorders them:

- **DE national-phone regex broaden** — todo #167.
- **Fixture-citation xtask lint** — todo #166.
- **`audit_metadata_only` follow-ups** — todos #172, #173, #186, #187.
- **Brew tap location decision** — todo #184.
- **Bundle class derive from rulepack** — todo #173.

## Later (under consideration / longer horizon)

- **Crate-shape Option B** — scratchpad 256 locked this as the v0.5 direction:
  extract `gaze-types` and collapse `gaze-assembly` after the v0.4 line settles.
- **v0.5 design follow-up dispatches** — todos #148-#152, scratchpad 359.
- **v0.5 dylint audit gate pivot** — todo #181; replace the current
  syn-walker `audit_metadata_only` gate with a resolver-backed lint. See
  `docs/research/v0.5-dylint-audit-gate.md`.
- **Macro-emitted / `include!` / `let-else` escape coverage** — todos #179,
  #185, #186, folded into the #181 dylint pivot.
- **First-party MCP for Gaze** — drawer `gaze_decisions_69b09f17`; open
  question.
- **PII detection-only thin surface** — drawer `gaze_decisions_92011c25`; open
  question.
- **Operations proxy + agent ACL** — memory `project_operations_proxy_idea.md`;
  separate session, not part of the current Gaze runtime cycle.

## Shipped (last ~5 minor releases)

| Version | Date | Highlights |
|---|---:|---|
| v0.4.5 | 2026-04-26 | DE+US national phones, audit retention purge + `audit_metadata_only` gate, `--session` audit filter, ClassMapOverrideSafety extension, rulepack version bump validation, `gaze-assembly` split |
| v0.4.4 | 2026-04-26 | ClassMapOverrideSafety gate, audit schema v2 with `--from` / `--to`, parser-backed E.164 phone validation, Date-as-PII posture memo |
| v0.4.3 | 2026-04-26 | Luhn + IBAN MOD-97 validators, `core-extended` Phase 2 for IBAN + credit cards, `no_tenant_knowledge` gate, audit query/export |
| v0.4.2 | 2026-04-25 | macOS aarch64 + Linux x86_64 binaries, `core-extended` Phase 1, audit log persistence, three-surfaces CLI backfill, v0.5 design stub |
| v0.4.1 | 2026-04-24 | Markus Phase-1 NER fix, locale-aware email-header names, `[ner] threshold`, rulepack composition validation, snapshot token-family metadata |

Older versions are tracked in `CHANGELOG.md`.

The v0.3 pipe-mode contract remains part of the shipped surface: `gaze clean`
and `gaze restore` exchange stdin/stdout JSON, emit sanitized closed-variant
stderr on failure, and restore through a two-pass strategy of exact session-token
replacement followed by token-shape fail-closed validation. Host adapters such
as the Laravel wrapper should continue to shell out to the standalone `gaze`
binary and encrypt session blobs before they leave local process scope.

## How to pick up an item

Start with `AGENTS.md` and `CLAUDE.md` for project rules, north-star rationale,
worktree discipline, and commit requirements. For non-trivial changes, follow
the established brainstorm → plan → multi-review → dispatch implementation
flow, then update this file when a release ships or a locked roadmap decision
changes.

## See also

- `CHANGELOG.md` — full release history
- `AGENTS.md` — project rules + north star rationale
- `CLAUDE.md` — Claude Code-specific addenda
- `docs/research/gaze-first-principles-vision.md` — north star locked 2026-04-24
- `docs/research/v0.5-dylint-audit-gate.md` — v0.5 architectural pivot stub
- `docs/architecture/xtask.md` — `audit_metadata_only` gate coverage + limitations
