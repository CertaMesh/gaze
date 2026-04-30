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

Post-v0.6 work is focused on tightening the newly shipped detection and
SafetyNet surfaces without weakening restore or audit boundaries:

- **Pass-3 SafetyNet follow-ups.** Decide the nightly/live-model drift-check
  shape, the native `ort` backend path, fetch/download UX, and whether a
  long-lived subprocess helper is needed for lower-latency adopters.
- **Cue-anchored Name precision.** Add RegionHint-based `CodeBlock` and `Url`
  exclusions for `anchored_match`, then revisit Subject/Re anchors, broader
  `name_shape` support, and per-region NER thresholding.
- **DE phone separator recall.** Extend `phone.national.de` coverage for
  separator-heavy synthetic fixture shapes while preserving parser-backed
  validation and tenant-numeric false-positive guards.

## Next (committed but not started)

These are open, verified candidates for the next planning pass:

- **OpenAI-filter CI sanity gate.** CI-only drift check over gold-positive
  fixtures for the SafetyNet path, separate from the local pre-push sanity gate.
- **Token grammar G1/G2/G3 brainstorm-pair.**
- **Feature-flag naming brainstorm-pair.**
- **PiiClass `Arc<str>` vs `Box<str>` post-impl bench.**

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
| v0.6.4 | 2026-04-30 | `phone.national.de` 3-digit and 4-digit area-code metro coverage (closes #420); pre-push hook docs-only fast-path skips cargo/xtask gates for allowlisted documentation paths (PR #120, external contributor @naoray) |
| v0.6.3 | 2026-04-30 | `phone.national.de` 10-digit metro landline coverage for Berlin/Hamburg/Frankfurt/Munich (closes #414); `phone.national.us` and `phone.structural` consuming-boundary fix rejects identifier-attached numbers (closes #415); DE phone regex no longer over-matches IBAN tails |
| v0.6.2 | 2026-04-30 | `ip.v6` recognizer RFC 4291 §2.2 IPv4-embedded form support including IPv4-mapped `::ffff:d.d.d.d` and IPv4-compatible `::d.d.d.d`; closes leak path where `::ffff:192.0.2.128` partially tokenized (closes #419) |
| v0.6.1 | 2026-04-30 | `gaze clean --openai-filter-device {auto\|cpu\|cuda\|mps}` SafetyNet device selection (closes #362); `phone.national.de` separator variants for hyphen/space/slash/dot (closes #316); `cargo-metadata-audit-isolation` fails loud on unknown feature names (closes #340, #350) |
| v0.6.0 | 2026-04-29 | Pass-3 SafetyNet runtime, cue-anchored `Name` detection through `anchored_match`, audit feature shim removal, `RedactionLogger` moved to `gaze-types`, tracked pre-push hook with doc-only fast path |
| v0.5.1 | 2026-04-29 | Bundled `rulepack_version` sync — `core`, `core-extended`, `locale-de`, `locale-en` embedded TOMLs now report `rulepack_version = "0.5.1"`, restoring the v0.4.6 contract that bundled rulepacks track `gaze-recognizers` |
| v0.5.0 | 2026-04-27 | `gaze-types` extraction, `gaze-audit` passive sink with one-minor `audit` feature shim, dylint-based `gaze_module_isolation` lint replaces syn-walker `audit_metadata_only` gate, `bundled-recognizers` feature gate frees `gaze` core from `ort` / `tokenizers` / `ndarray` ML deps |
| v0.4.6 | 2026-04-26 | Bundle-tokenization-drift xtask gate, fixture-citation lint, rulepack-derived bundle classes, DE national-phone recall broaden, no-feature phone parser fail-closed regression, Homebrew tap decision |

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
