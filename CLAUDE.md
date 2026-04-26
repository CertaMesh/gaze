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

As of v0.4.5 the repo is split into a published-crates trio (`gaze`, `gaze-recognizers`, `gaze-cli`) plus an assembly seam (`gaze-assembly`) and one internal crate (`xtask`). Detection is recognizer-native. Future Claude sessions should keep the deltas below in mind before touching code.

- **Crates:** `gaze` (core: pipeline, session, policy, registry, locale, rulepack), `gaze-recognizers` (regex/dictionary/NER backends + embedded `core` and `core-extended` rulepacks + locale bundles), `gaze-assembly` (policy-to-pipeline builder; CLI and adopters call `build_pipeline`), `gaze-cli` (standalone `gaze` binary; subcommands `clean`, `restore`, `audit query`, `audit export`), `xtask` (internal gate runner). External consumer: [piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens), formerly the in-tree `debug-proxy` crate. Always `cargo test --workspace`, never per-crate, unless you're narrowing a single failure.
- **Detection is recognizer-native.** Every detector runs through `gaze::RecognizerRegistry` with a typed `DetectContext` envelope. The legacy standalone `Detector` path was removed. New detection features should land as a `Recognizer` impl in `gaze-recognizers`, not as a bespoke pipeline hook.
- **Policy surface:** `[policy.rulepacks]` (bundled + path) + `[[policy.custom_recognizers]]`. Top-level `[[detector]]` is rejected. When editing `crates/gaze/src/policy.rs`, cross-check [docs/policy.md](docs/policy.md) and the `gaze-cli` integration suite in `crates/gaze-cli/tests/cli_pipe.rs`.
- **Locale chain is 4-tier** (CLI > policy > rulepack default > system default) with strict `LocaleTag::Other(_)` matching. Recognizers gate on `locales = [...]`.
- **Conflict resolution:** class-priority > rule-priority > score > span-length > recognizer-id with multi-overlap fixed point. Losers are logged with `decided_by: ConflictTier` in the redaction log.
- **Validator/normalizer enums (v0.4.2+):** `ValidatorKind` is closed: `EmailRfc`, `E164Phone` (gated behind the `phone-parser` Cargo feature), `Luhn`, `IbanMod97`. `NormalizerKind` is closed: `EmailCanonical`, `IbanCanonical`. Unknown names fail closed at rulepack load time with `RulepackError::UnsupportedValidator` / `UnsupportedNormalizer`. New validator kinds need a deliberate enum extension and rulepack-loader update — do not paper over an unknown name.
- **Bundled rulepacks:** `core` (always-on email + email-header), `core-extended` (opt-in: phone shape + parser-backed E.164, parser-backed DE+US national phone (v0.4.5 S2), IPv4/IPv6, postal codes, IBAN with `iban_mod97` + `iban_canonical`, credit card with `luhn`). Default `[[rule]]` entries ship in `core-extended` so `--rulepack-bundled core,core-extended` tokenizes the new classes out of the box. **No-policy locale activation (v0.4.5):** `--rulepack-bundled core-extended` without a policy now activates `phone.national.de`, `phone.national.us`, `postal.us`, and `postal.de`. Adopters relying on prior locale-gated behavior must pass `--locale=global` or supply a policy with narrower locale gating (todo #171).
- **Audit schema v2 (v0.4.4):** `RedactionEntry` carries `created_at: i64` epoch milliseconds; on-open `ALTER TABLE` migration keeps legacy v0.4.3 audit DBs queryable through a NULL default. `gaze audit query` and `gaze audit export` accept `--from <iso8601>` and `--to <iso8601>` filters and open the SQLite log read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY`. v0.4.5 S1 adds `--session <opaque>` filtering on opaque audit metadata (NOT raw `session_hex`).
- **Audit retention manual purge (v0.4.5 S3):** `gaze audit purge --before <iso8601> [--dry-run | --count]` deletes redaction-log rows older than the cutoff. Calendar-aware ISO 8601 validation; typed `AuditPurgeIso8601` failure mode; restricted DELETE clause; no policy-level retention default; no background auto-purge — adopter-driven.
- **Active xtask gates as of v0.4.5:** `symmetric-potemkin`, `class-map-override-safety` (activated v0.4.4 S1 — runs `t20_context_class_map_overrides_policy_dict_class` + `t20a_class_map_override_fails_closed_when_action_rule_uncovered`), `recognizer-composition-validator`, `no-tenant-knowledge` (v0.4.3 S3 — production-code lint scanner), `audit-metadata-only` (activated v0.4.5 S3 — syn-based AST walker enforcing restore-path code does not import audit metadata symbols; known limitations and v0.5 dylint pivot documented in [`docs/architecture/xtask.md`](docs/architecture/xtask.md)). All gates must invoke at least one behavioral test; symbol or string-presence-only checks are recursive-Potemkin and forbidden.
- **Tenant-class fixture discipline:** production code in `crates/{gaze,gaze-recognizers,gaze-assembly,gaze-cli}/src/` must not contain tenant-specific patterns (`order_id`, `Order_42`, `Song_42`, `User_7`). Phone fixtures must use synthetic non-reachable values (NANPA 555-01xx, Ofcom drama ranges, or out-of-band country codes). See [`CONTRIBUTING.md`](CONTRIBUTING.md).
- **Rulepack fields parsed but gated** (v0.4.1-pending): `token.format`, `context.hotwords`, `context.boost`, `context.window`. If a design needs these, unblock them deliberately rather than editing around the `UnsupportedField` guard. `token.family` was un-gated in v0.4.2 and now threads from recognizers into session snapshot entries.
