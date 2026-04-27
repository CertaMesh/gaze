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

## v0.5 architecture primer

As of v0.5 dev complete (`[Unreleased]`) the repo is split into six published-shape crates plus one internal `xtask` crate. v0.5 dev introduced two new crates (`gaze-types`, `gaze-audit`) and replaced the syn-walker audit-isolation gate with a Dylint resolver-based gate. Detection contract is unchanged from v0.4. Always `cargo test --workspace --all-features` (the `--all-features` flag enables `gaze`'s `audit` feature shim that the dev-dep compatibility tests rely on).

- **Crates:** `gaze` (core: pipeline, session, policy, registry, locale, rulepack, `RedactionLogger` trait — no longer depends on `rusqlite`), `gaze-types` (shared value contracts, serde-only, no ML/sql deps; consumable by adopters who want the contract surface without `ort`/`tokenizers`/`ndarray`), `gaze-recognizers` (regex/dictionary/NER backends + embedded `core` and `core-extended` rulepacks + locale bundles), `gaze-audit` (passive sink: `SqliteLogger`, `AuditFilter`, `AuditLogRow`, `build_audit_query_sql`, `AUDIT_RESTRICTED_COLUMNS`; `rusqlite` lives only here), `gaze-assembly` (policy-to-pipeline builder used by CLI-style adopters), `gaze-cli` (standalone `gaze` binary; the only allowlisted `gaze-audit` consumer outside compatibility tests), `xtask` (internal gate runner; the protected-path Dylint lint crate lives in `xtask/dylint/` as a detached workspace pinned to `nightly-2025-09-18`). External consumer: [piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens), formerly the in-tree `debug-proxy` crate.
- **Detection is recognizer-native (unchanged from v0.4).** Every detector runs through `gaze::RecognizerRegistry` with a typed `DetectContext` envelope. The legacy standalone `Detector` path was removed in v0.4.0-rc.1. New detection features should land as a `Recognizer` impl in `gaze-recognizers`, not as a bespoke pipeline hook.
- **Audit-log path (v0.5 Phase C):** Rust adopters import `use gaze_audit::SqliteLogger;` directly. `gaze` no longer carries `rusqlite` in default or `--no-default-features` builds. A one-minor `audit` feature shim on `gaze` re-exports the previous `gaze::SqliteLogger` path for migration; it is scheduled to drop in v0.6 (decision drawer `gaze_decisions_6c60bce3b9f8ed7a4de538d8`). `cargo run -p xtask -- cargo-metadata-audit-isolation` plus a `cargo deny` ban rule keep the protected default graph clean.
- **Audit-sink protected-path enforcer (v0.5 Phase D, replaces Phase E removed walker):** the canonical gate is the `gaze_module_isolation` Dylint lint resolved via `LateContext::qpath_res`. Pinned toolchain: `nightly-2025-09-18`, `clippy_utils@20ce69b9...`, `dylint_linting`/`dylint_testing` 5.0. 18 UI fixtures cover all known bypass classes including macro call-site hygiene, `#[path]`, `include!`, type positions, trait bounds, and `extern crate`. The legacy syn walker (`audit-metadata-only` xtask + workflow) was decommissioned in Phase E (PR #77, `f4fde12`). Architecture, toolchain pins, and timings: [`docs/research/v0.5-dylint-audit-gate.md`](docs/research/v0.5-dylint-audit-gate.md).
- **Policy surface (unchanged from v0.4):** `[policy.rulepacks]` (bundled + path) + `[[policy.custom_recognizers]]`. Top-level `[[detector]]` is rejected. When editing `crates/gaze/src/policy.rs`, cross-check [docs/policy.md](docs/policy.md) and the `gaze-cli` integration suite in `crates/gaze-cli/tests/cli_pipe.rs`.
- **Locale chain is 4-tier** (CLI > policy > rulepack default > system default) with strict `LocaleTag::Other(_)` matching. Recognizers gate on `locales = [...]`.
- **Conflict resolution:** class-priority > rule-priority > score > span-length > recognizer-id with multi-overlap fixed point. Losers are logged with `decided_by: ConflictTier` in the redaction log.
- **Validator/normalizer enums (v0.4.2+):** `ValidatorKind` closed (`EmailRfc`, `E164Phone` gated behind `phone-parser`, `Luhn`, `IbanMod97`); `NormalizerKind` closed (`EmailCanonical`, `IbanCanonical`). Unknown names fail closed at rulepack load with `RulepackError::UnsupportedValidator` / `UnsupportedNormalizer`.
- **Active xtask gates (v0.5):** `symmetric-potemkin`, `class-map-override-safety`, `recognizer-composition-validator`, `no-tenant-knowledge`, `bundle-tokenization-drift`, `fixture-citation-lint`, `ci-feature-matrix`, `cargo-metadata-audit-isolation` (Phase C), `dylint-gate` (Phase D, canonical audit-sink isolation). The legacy `audit-metadata-only` gate was removed in Phase E. All gates must invoke at least one behavioral test; symbol-or-string-presence-only checks are recursive-Potemkin and forbidden.
- **Migration knobs for v0.5 → v0.6:** drop the `audit` feature shim on `gaze` (removes `gaze::SqliteLogger` re-export); move `RedactionLogger` trait from `gaze` to `gaze-types` per todo #252.
- **Rulepack fields parsed but gated** (v0.4.1-pending): `token.format`, `context.hotwords`, `context.boost`, `context.window`. `token.family` was un-gated in v0.4.2.
