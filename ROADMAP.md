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

v0.5.1 shipped 2026-04-29 (bundled `rulepack_version` sync — the four embedded
TOMLs now report `rulepack_version = "0.5.1"`, restoring the v0.4.6 contract
that bundled rulepacks track `gaze-recognizers`; todo #267). v0.5.0 shipped
2026-04-27 (gaze-types extraction, gaze-audit passive sink with the one-minor
`audit` feature shim, dylint-based `gaze_module_isolation` lint replacing the
syn-walker `audit_metadata_only` gate, `bundled-recognizers` feature gate).
v0.4.6 shipped 2026-04-26. The v0.5 line is closed.

No v0.6 implementation scope is locked in this roadmap yet. The next cycle
should start with brainstorm → plan → multi-review before dispatching
implementation work. The leading hygiene candidate is the v0.6 decommission of
the `audit` feature shim on `gaze` (per the v0.5 migration window documented in
CHANGELOG.md and decision drawer `gaze_decisions_6c60bce3b9f8ed7a4de538d8`),
optionally landed alongside todo #252.

## Next (committed but not started)

These are open, verified candidates for the v0.6 cycle pending the v0.6
brainstorm:

- **OpenAI-filter Pass-3 safety net recognizer** — todo #65 (high). Retargeted
  to v0.6 from v0.4.1. Adds a fresh independent eye after Gaze's regex +
  dictionary + NER passes; the runtime arm of the never-leak promise.
- **OpenAI-filter CI sanity gate** — todo #66 (medium). Retargeted to v0.6.
  Sibling to #65; CI-only, runs the filter over gold-positive fixtures to
  surface detection drift between releases.
- **`RedactionLogger` trait → `gaze-types`** — todo #252 (low). Hygiene
  follow-on from v0.5 Phases B/C; pairs with the `audit` feature shim
  decommission planned for v0.6.

The following entries are still tagged `v0.5` from the v0.5 design wave but did
not block the v0.5.0 ship; the v0.6 brainstorm should decide whether to fold
them into v0.6, defer further, or close:

- **Token grammar G1/G2/G3 brainstorm-pair** — todo #148 (high).
- **Feature-flag naming brainstorm-pair** — todo #149 (medium).
- **PiiClass `Arc<str>` vs `Box<str>` post-impl bench** — todo #152 (low).

## Later (under consideration / longer horizon)

- **First-party MCP for Gaze** — drawer `gaze_decisions_69b09f17`; open
  question.
- **PII detection-only thin surface** — drawer `gaze_decisions_92011c25`; open
  question.
- **Operations proxy + agent ACL** — memory `project_operations_proxy_idea.md`;
  separate session, not part of the current Gaze runtime cycle.

## Shipped (last ~5 minor releases)

| Version | Date | Highlights |
|---|---:|---|
| v0.5.1 | 2026-04-29 | Bundled `rulepack_version` sync (todo #267) — `core`, `core-extended`, `locale-de`, `locale-en` embedded TOMLs now report `rulepack_version = "0.5.1"`, restoring the v0.4.6 contract that bundled rulepacks track `gaze-recognizers` |
| v0.5.0 | 2026-04-27 | `gaze-types` extraction, `gaze-audit` passive sink with one-minor `audit` feature shim, dylint-based `gaze_module_isolation` lint replaces syn-walker `audit_metadata_only` gate, `bundled-recognizers` feature gate frees `gaze` core from `ort` / `tokenizers` / `ndarray` ML deps |
| v0.4.6 | 2026-04-26 | Bundle-tokenization-drift xtask gate, fixture-citation lint, rulepack-derived bundle classes, DE national-phone recall broaden, no-feature phone parser fail-closed regression, Homebrew tap decision |
| v0.4.5 | 2026-04-26 | DE+US national phones, audit retention purge + `audit_metadata_only` gate, `--session` audit filter, ClassMapOverrideSafety extension, rulepack version bump validation, `gaze-assembly` split |
| v0.4.4 | 2026-04-26 | ClassMapOverrideSafety gate, audit schema v2 with `--from` / `--to`, parser-backed E.164 phone validation, Date-as-PII posture memo |
| v0.4.3 | 2026-04-26 | Luhn + IBAN MOD-97 validators, `core-extended` Phase 2 for IBAN + credit cards, `no_tenant_knowledge` gate, audit query/export |

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
- `docs/architecture/xtask.md` — current xtask gate inventory; `audit_metadata_only` is decommissioned as of v0.5 Phase E and is documented for historical context only
