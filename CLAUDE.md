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

North-star rationale and the five-axis summary live in [AGENTS.md](./AGENTS.md).

## Local gates

The repo no longer ships a tracked pre-push hook. Run gates manually before
pushing if you want defense in depth:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- ci-feature-matrix
```

PR-triggered CI (`.github/workflows/docs.yml`) catches doc-test and rustdoc
warnings on every PR. Workspace test gates and xtask gates are not yet in
remote CI — add when CI capacity allows.

## v0.6 architecture primer

As of v0.6.4 the repo is split into six published-shape crates plus one internal `xtask` crate. The v0.5 cycle introduced `gaze-types` and `gaze-audit` and replaced the syn-walker audit-isolation gate with a Dylint resolver-based gate; v0.6 dropped the `gaze` `audit` feature shim and moved the `RedactionLogger` trait into `gaze-types`. Detection contract is unchanged from v0.4. Always `cargo test --workspace --all-features` — the `--all-features` flag exercises the safety-net surface and the `gaze-recognizers` `phone-parser` path.

- **Crates:** `gaze` (core: pipeline, session, policy, registry, locale, rulepack; re-exports `gaze_types::RedactionLogger` for source-compat — no `rusqlite` dep), `gaze-types` (shared value contracts including the canonical `RedactionLogger` trait, serde-only, no ML/sql deps; consumable by adopters who want the contract surface without `ort`/`tokenizers`/`ndarray`), `gaze-recognizers` (regex/dictionary/NER backends + embedded `core` and `core-extended` rulepacks + locale bundles), `gaze-audit` (passive sink: `SqliteLogger`, `AuditFilter`, `AuditLogRow`, `build_audit_query_sql`, `AUDIT_RESTRICTED_COLUMNS`; `rusqlite` lives only here), `gaze-assembly` (policy-to-pipeline builder used by CLI-style adopters), `gaze-cli` (standalone `gaze` binary; the only allowlisted `gaze-audit` consumer outside compatibility tests), `xtask` (internal gate runner; the protected-path Dylint lint crate lives in `xtask/dylint/` as a detached workspace pinned to `nightly-2025-09-18`). External consumer: [EmpireTwo/gaze-lens](https://github.com/EmpireTwo/gaze-lens), formerly the in-tree `debug-proxy` crate.
- **Detection is recognizer-native (unchanged from v0.4).** Every detector runs through `gaze::RecognizerRegistry` with a typed `DetectContext` envelope. The legacy standalone `Detector` path was removed in v0.4.0-rc.1. New detection features should land as a `Recognizer` impl in `gaze-recognizers`, not as a bespoke pipeline hook.
- **Audit-log path:** Rust adopters import `use gaze_audit::SqliteLogger;` directly. `gaze` no longer carries `rusqlite` in any feature graph. The one-minor `audit` feature shim on `gaze` (introduced in v0.5 Phase C) was removed in v0.6; `gaze::SqliteLogger` no longer compiles. `cargo run -p xtask -- cargo-metadata-audit-isolation` plus a `cargo deny` ban rule keep the protected default, `--no-default-features`, and safety-net graphs clean.
- **Audit-sink protected-path enforcer (canonical):** the `gaze_module_isolation` Dylint lint resolved via `LateContext::qpath_res`. Pinned toolchain: `nightly-2025-09-18`, `clippy_utils@20ce69b9...`, `dylint_linting`/`dylint_testing` 5.0. 18 UI fixtures cover all known bypass classes including macro call-site hygiene, `#[path]`, `include!`, type positions, trait bounds, and `extern crate`. The legacy syn walker (`audit-metadata-only` xtask + workflow) was decommissioned in v0.5 Phase E (PR #77, `f4fde12`). Architecture, toolchain pins, and timings: [`docs/research/v0.5-dylint-audit-gate.md`](docs/research/v0.5-dylint-audit-gate.md).
- **Pass-3 SafetyNet (v0.6.0+):** observer-only post-clean check that runs against already-tokenized output without mutating the manifest or restore path. Activation paths are CLI flags (see [`crates/gaze-cli/README.md`](crates/gaze-cli/README.md#safety-net)) and the programmatic `Pipeline::with_safety_net` builder. Architecture contract: [`docs/architecture/safety-nets.md`](docs/architecture/safety-nets.md). The OpenAI-filter subprocess device is selectable via `gaze clean --openai-filter-device {auto|cpu|cuda|mps}` (v0.6.1).
- **Cue-anchored Name detection (v0.6.0+):** `anchored_match` recognizer kind and the `forward_markers` / `agent_recipient_cues` / `footer_cues` locale buckets in `locale-de` and `locale-en`. Composes with the `core` bundle without custom recognizers.
- **Policy surface (unchanged from v0.4):** `[policy.rulepacks]` (bundled + path) + `[[policy.custom_recognizers]]`. Top-level `[[detector]]` is rejected. When editing `crates/gaze/src/policy.rs`, cross-check [docs/policy.md](docs/policy.md) and the `gaze-cli` integration suite in `crates/gaze-cli/tests/cli_pipe.rs`.
- **Locale chain is 4-tier** (CLI > policy > rulepack default > system default) with strict `LocaleTag::Other(_)` matching. Recognizers gate on `locales = [...]`.
- **Conflict resolution:** class-priority > rule-priority > score > span-length > recognizer-id with multi-overlap fixed point. Losers are logged with `decided_by: ConflictTier` in the redaction log.
- **Validator/normalizer enums (v0.4.2+):** `ValidatorKind` closed (`EmailRfc`, `E164Phone` gated behind `phone-parser`, `Luhn`, `IbanMod97`); `NormalizerKind` closed (`EmailCanonical`, `IbanCanonical`). Unknown names fail closed at rulepack load with `RulepackError::UnsupportedValidator` / `UnsupportedNormalizer`.
- **Active xtask gates:** `symmetric-potemkin`, `class-map-override-safety`, `recognizer-composition-validator`, `no-tenant-knowledge`, `bundle-tokenization-drift`, `fixture-citation-lint`, `ci-feature-matrix`, `cargo-metadata-audit-isolation`, `dylint-gate` (canonical audit-sink isolation), `safety-net-sanity` (v0.6 SafetyNet behavioral gate). All gates must invoke at least one behavioral test; symbol-or-string-presence-only checks are recursive-Potemkin and forbidden.
- **`core-extended` no-policy bundled activation (v0.6+):** invocations of `--rulepack-bundled core-extended` without a policy activate `phone.national.de`, `phone.national.us`, `postal.us`, and `postal.de` recognizers. Adopters relying on no national phone tokenization or no bare 5-digit numeric tokenization must pass `--locale=global` or supply a policy with narrower locale gating.
- **Rulepack fields parsed but gated** (v0.4.1-pending): `token.format`, `context.hotwords`, `context.boost`, `context.window`. `token.family` was un-gated in v0.4.2.
