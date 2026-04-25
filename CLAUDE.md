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

Full rationale (including what the north star rejects and how drift is measured) lives in [docs/research/gaze-first-principles-vision.md](docs/research/gaze-first-principles-vision.md#north-star-locked-2026-04-24).

## v0.4 architecture primer

As of v0.4.0-rc.1 the repo is split into four crates and uses a recognizer-native detection path. Future Claude sessions should keep the deltas below in mind before touching code.

- **Crates:** `gaze` (core: pipeline, session, policy, registry, locale, rulepack), `gaze-recognizers` (regex/dictionary/NER backends + embedded rulepacks + locale bundles), `gaze-cli` (standalone `gaze` binary, the `map_policy_error` surface, `--locale`/`--context-json` flags), `debug-proxy` (MCP consumer). Always `cargo test --workspace`, never per-crate, unless you're narrowing a single failure.
- **Detection is recognizer-native.** Every detector runs through `gaze::RecognizerRegistry` with a typed `DetectContext` envelope. The legacy standalone `Detector` path was removed. New detection features should land as a `Recognizer` impl in `gaze-recognizers`, not as a bespoke pipeline hook.
- **Policy surface:** `[policy.rulepacks]` (bundled + path) + `[[policy.custom_recognizers]]`. Top-level `[[detector]]` is rejected. When editing `crates/gaze/src/policy.rs`, cross-check [docs/policy.md](docs/policy.md) and the `gaze-cli` integration suite in `crates/gaze-cli/tests/cli_pipe.rs`.
- **Locale chain is 4-tier** (CLI > policy > rulepack default > system default) with strict `LocaleTag::Other(_)` matching. Recognizers gate on `locales = [...]`.
- **Conflict resolution:** class-priority > rule-priority > score > span-length > recognizer-id with multi-overlap fixed point. Losers are logged with `decided_by: ConflictTier` in the redaction log.
- **Rulepack fields parsed but gated** (v0.4.1-pending): `token.family`, `token.format`, `context.hotwords`, `context.boost`, `context.window`. If a design needs these, unblock them deliberately rather than editing around the `UnsupportedField` guard.
