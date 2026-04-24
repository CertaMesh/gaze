# CLAUDE.md — Gaze

See [AGENTS.md](./AGENTS.md) for canonical project rules + the Gaze north star. This file adds only Claude-Code-specific addenda.

Repo-level guidance for Claude Code sessions working in this project.

## Project north star

**Gaze is the most reliable, reversible PII pseudonymization runtime for agentic workflows. Zero PII leaks between the agent and the data owner — ever. Any byte of PII that reaches an LLM outside the manifest contract is a critical defect.**

Verbatim user directive (2026-04-24): *"set a north star to be focused on never leaking any PII data and making this lib the best PII [pseudonymization] there is for agentic interaction with information"*.

"Pseudonymization" is the GDPR Art. 4(5) term for reversible substitution with tokens — chosen over "redaction" (one-way, loses the restore moat), "obscuring" (vague), and "tokenization" (overloaded with payment industry usage).

### The five axes

1. **Reliability (never leak).** Fail-closed always. Defense in depth (regex + NER + dictionary + optional neural safety net). Every known detection gap is a todo; every leak incident is a postmortem + fix pattern baked into skill/memory.
2. **Reversibility.** Manifest-first restore. Format-preserving tokens stay restorable. No one-way primitives in the core contract. Anything that breaks restore round-trip is a design regression.
3. **Agentic-first.** Decisions prioritize agent workflow needs over generic text handling — tool-call JSON embedding, streaming LLM, multi-turn sessions with evolving context, tenant-specific PII (songs, order IDs, artist names).
4. **Trust (auditable + deterministic).** Rule-based detectors preferred over neural for precise classes. Neural is an addon (safety net, free-text NER), not the floor. Every token emission traceable to a rule/recognizer. Typed exceptions + closed error-variant set. No silent mismatches.
5. **Adopter ergonomics.** Low-friction integration (Laravel adapter pattern, clear TOML policy, sane defaults). Framework adapters pave the 80% case; library API serves the 20% power case. Adopter can pick Gaze up in under a day without deep PII domain expertise.

### How to apply

All design, implementation, and review decisions in this repo must be evaluated against these axes. If a decision weakens any axis, call it out in the PR description and justify the tradeoff. Correctness axes 1–4 always beat performance.

Full rationale (including what the north star rejects and how drift is measured) lives in [docs/research/gaze-first-principles-vision.md](docs/research/gaze-first-principles-vision.md#north-star-locked-2026-04-24) and in MemPalace drawer `drawer_gaze_decisions_ba559e1cf1fbca5c1098b12f` (wing=gaze, room=decisions).
