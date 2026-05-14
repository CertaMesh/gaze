# Gaze Architecture

## Purpose And Audience

This document is the root architecture map for contributors and adopters who
need to understand how Gaze's crates fit together before reading individual
crate READMEs or deep-dive design notes.

Gaze's north star is defined in [AGENTS.md](./AGENTS.md): reliable, reversible
PII pseudonymization for agentic workflows, with zero PII leaks between the
agent and the data owner. The short version is: fail closed, preserve restore
round trips, make every token auditable, and keep the adopter path small enough
to integrate without becoming a PII-domain specialist.

## Pipeline

The core pipeline turns source content into safe content plus a restore
manifest. SafetyNet runs after tokenization as an observer, not as a mutating
redaction stage.

```text
Raw text / structured document
        |
        v
+-----------------------+
| Recognizer registry   |
| regex / dictionary /  |
| NER / custom rules    |
+-----------------------+
        |
        v
+-----------------------+      +-------------------------+
| Candidate validation  |----->| loser audit rows        |
| validator veto first  |      | ValidatorVeto metadata  |
+-----------------------+      +-------------------------+
        |
        v
+-----------------------+
| Conflict resolution   |
| class, rule, score,   |
| span, recognizer id   |
+-----------------------+
        |
        v
+-----------------------+      +-------------------------+
| Tokenization          |----->| Manifest                |
| format-preserving     |      | original <-> token map  |
| pseudonyms            |      | restore contract        |
+-----------------------+      +-------------------------+
        |
        v
+-----------------------+      +-------------------------+
| Clean output          |----->| Pass-3 SafetyNet        |
| safe for agent / LLM  |      | observer-only suspects  |
+-----------------------+      +-------------------------+
        |
        v
Restore uses the manifest to recover owner-side originals.
```

Source anchors: [crates/gaze/src/pipeline.rs](./crates/gaze/src/pipeline.rs),
[crates/gaze/src/resolver.rs](./crates/gaze/src/resolver.rs),
[crates/gaze/src/registry.rs](./crates/gaze/src/registry.rs),
[crates/gaze-types/src/lib.rs](./crates/gaze-types/src/lib.rs), and
[docs/architecture/safety-nets.md](./docs/architecture/safety-nets.md).

## Crate Map

The workspace currently has nine published-shape crates plus internal `xtask`.
For the fuller crate boundary table, see
[docs/architecture/crates.md](./docs/architecture/crates.md).

| Crate | Role | You only need this if... |
| --- | --- | --- |
| `gaze` | Core reversible pseudonymization runtime: `Pipeline`, `Session`, policy, rulepacks, registry, locale chain, token shape, restore. | You want to link the library directly and own pipeline/session/audit wiring. |
| `gaze-types` | Shared value contracts: recognizer traits, PII classes, documents, manifest/log types, `RedactionLogger`, SafetyNet contracts. | You need public contract types without pulling SQLite, policy loading, ONNX, tokenizers, or built-in recognizers. |
| `gaze-recognizers` | Built-in regex, dictionary, NER recognizers, embedded rulepacks, validator/normalizer dispatch, and SafetyNet backends. | You want Gaze's shipped detectors instead of implementing recognizers yourself. |
| `gaze-audit` | Passive SQLite audit sink and audit-query API. `rusqlite` lives here. | You want a concrete SQLite redaction-log sink or query surface. |
| `gaze-assembly` | Policy-to-pipeline builder used by CLI-style adopters. | You want CLI-equivalent policy/rulepack assembly without copying CLI code. |
| `gaze-cli` | Published `gaze` binary for process-boundary integrations: clean, restore, audit, document, MCP. | Your adapter or script should shell out instead of linking Rust. |
| `gaze-mcp-core` | Transport-free MCP-shaped chokepoint runtime: tool registry, sealed context, envelope dispatch, manifest store, auth hook, session-id policy. | You are building an MCP tool host and need every tool call through Gaze before reaching a source system. |
| `gaze-mcp-rmcp` | rmcp transport sink for `gaze-mcp-core`, with stdio default and opt-in streamable HTTP. | You want rmcp framing without reimplementing the transport adapter. |
| `gaze-document` | OSS document ingestion: PNG/JPG/PDF to Tesseract OCR to Gaze redaction to `SafeBundle`. | You need `clean.md`, `manifest.json`, and `report.json` from scanned or rasterized documents. |
| `xtask` | Internal gate runner plus detached Dylint workspace for protected-path enforcement. | You are adding or running repository gates and CI-only checks. |

## Three Execution Layers

Gaze has three integration layers. They all rely on the same core invariant:
PII must cross the agent boundary only as manifest-backed pseudonymous tokens.

```text
Direct library integration
  App code
    -> gaze::Pipeline
    -> owner-controlled manifest / restore

MCP source chokepoint
  Agent tool call
    -> gaze-mcp-rmcp transport
    -> gaze-mcp-core PiiEnvelope::dispatch
    -> source system, with safe results returned to the agent

LLM API proxy (v0.8 new / planned)
  User or agent LLM request
    -> gaze-proxy provider driver
    -> vendor API
    -> restore path under owner control
```

`gaze::Pipeline` is the library API for applications that already control their
data path. `gaze-mcp-core` and `gaze-mcp-rmcp` cover the model-to-source axis:
agent tool calls, document tools, manifest handles, and tiered restore access.
They explicitly do not cover raw user paste/upload/screenshot traffic in an
agent host chat UI; that belongs to `gaze-proxy` in the v0.8 proxy runtime.

The planned `gaze-proxy` layer is a provider-driver runtime for the
user-to-model axis. Its architecture should stay adapter-oriented: one proxy
core owns request/response pseudonymization, while provider drivers isolate
vendor-specific request shapes and streaming behavior. Until that crate and
`docs/architecture/proxy-runtime.md` land, references to `gaze-proxy` are v0.8
new/planned, not shipped behavior.

Source anchors: [crates/gaze/src/pipeline.rs](./crates/gaze/src/pipeline.rs),
[docs/architecture/mcp-runtime.md](./docs/architecture/mcp-runtime.md),
[crates/gaze-mcp-core/src/lib.rs](./crates/gaze-mcp-core/src/lib.rs), and
[crates/gaze-mcp-rmcp/src/lib.rs](./crates/gaze-mcp-rmcp/src/lib.rs).

## Key Design Decisions

### KDD-1: Reversibility First

Gaze is pseudonymization, not one-way redaction. The core contract emits clean
tokens plus a manifest that can restore owner-side originals; anything that
breaks clean/restore round trip is an architecture regression.

Source anchors: [AGENTS.md](./AGENTS.md),
[crates/gaze/src/session.rs](./crates/gaze/src/session.rs),
[crates/gaze/src/pipeline.rs](./crates/gaze/src/pipeline.rs), and
[crates/gaze-types/src/lib.rs](./crates/gaze-types/src/lib.rs).

### KDD-2: Rule-Based Detectors Are The Trust Floor

Precise classes should be handled by deterministic recognizers, validators,
dictionaries, and locale-aware rules before neural systems get involved.
Neural components are defense in depth; every emitted token must still trace
back to a recognizer, rule, or typed safety contract.

Source anchors: [AGENTS.md](./AGENTS.md),
[crates/gaze/src/registry.rs](./crates/gaze/src/registry.rs),
[crates/gaze-recognizers/src/regex.rs](./crates/gaze-recognizers/src/regex.rs),
and [docs/architecture/safety-nets.md](./docs/architecture/safety-nets.md).

### KDD-3: Audit Sink Isolation Is Enforced By Dylint

SQLite audit storage is isolated in `gaze-audit`; `gaze` must not grow a
`rusqlite` feature graph. The canonical protected-path gate is the
`gaze_module_isolation` Dylint lint in the detached `xtask/dylint` workspace,
with the older syn walker decommissioned.

Source anchors: [CLAUDE.md](./CLAUDE.md),
[docs/architecture/xtask.md](./docs/architecture/xtask.md),
[crates/gaze-audit/src/sqlite.rs](./crates/gaze-audit/src/sqlite.rs), and
[xtask/dylint/src/lib.rs](./xtask/dylint/src/lib.rs).

### KDD-4: Closed Validator And Normalizer Surfaces Fail Closed

Validator and normalizer names parse into typed enums, and unknown names fail
at rulepack load with explicit unsupported-kind errors. The public enums are
`#[non_exhaustive]` for forward-compatible Rust matching, but runtime accepted
names remain closed and auditable.

Source anchors: [crates/gaze-types/src/lib.rs](./crates/gaze-types/src/lib.rs),
[crates/gaze-recognizers/src/regex.rs](./crates/gaze-recognizers/src/regex.rs),
[crates/gaze-recognizers/src/error.rs](./crates/gaze-recognizers/src/error.rs),
and [crates/gaze/src/rulepack.rs](./crates/gaze/src/rulepack.rs).

### KDD-5: Locale Resolution Has Four Tiers

Active locale resolution is ordered as CLI override, policy locale, rulepack
default locale, then system/default fallback. Recognizers declare locale gates,
and `LocaleTag::Other(_)` matching is strict rather than fuzzy.

Source anchors: [docs/architecture/locale-chain.md](./docs/architecture/locale-chain.md),
[crates/gaze-types/src/lib.rs](./crates/gaze-types/src/lib.rs),
[crates/gaze-cli/src/pipeline/run.rs](./crates/gaze-cli/src/pipeline/run.rs),
and [crates/gaze-assembly/src/defaults.rs](./crates/gaze-assembly/src/defaults.rs).

### KDD-6: Conflict Resolution Is Deterministic

When candidates overlap, Gaze resolves them in a fixed order: PII class
priority, rule priority, score, span length, and recognizer id. Collision-family
policy and mandatory anchors add fail-closed fallback, while `ConflictTier`
keeps losers visible in the audit trail.

Source anchors: [crates/gaze/src/resolver.rs](./crates/gaze/src/resolver.rs),
[crates/gaze/src/pipeline.rs](./crates/gaze/src/pipeline.rs),
[crates/gaze-types/src/lib.rs](./crates/gaze-types/src/lib.rs),
[docs/architecture/collision-family.md](./docs/architecture/collision-family.md),
and [docs/architecture/anchor-resolution.md](./docs/architecture/anchor-resolution.md).

### KDD-7: Pass-3 SafetyNet Is Observer-Only

SafetyNet runs after tokenization against already-clean output and the runtime
manifest. It may emit `LeakSuspect` metadata, warnings, or strict-mode failures,
but it must not mutate clean text or add restore mappings.

Source anchors: [docs/architecture/safety-nets.md](./docs/architecture/safety-nets.md),
[crates/gaze/src/pipeline.rs](./crates/gaze/src/pipeline.rs),
[crates/gaze-recognizers/src/safety_net/test_support.rs](./crates/gaze-recognizers/src/safety_net/test_support.rs),
and [crates/gaze/tests/safety_net.rs](./crates/gaze/tests/safety_net.rs).

### KDD-8: Proxy Providers Use Adapter Drivers (v0.8 New / Planned)

The v0.8 `gaze-proxy` runtime should isolate vendor-specific API shape in
provider drivers while the proxy core owns pseudonymization, manifest handling,
restore boundaries, and fail-closed behavior. Until the proxy crate lands, this
is an architecture direction, not shipped behavior.

Source anchors: [docs/architecture/mcp-runtime.md](./docs/architecture/mcp-runtime.md),
[crates/gaze-mcp-core/src/lib.rs](./crates/gaze-mcp-core/src/lib.rs), and
[crates/gaze-mcp-rmcp/src/lib.rs](./crates/gaze-mcp-rmcp/src/lib.rs).

## Cross-Cutting Invariants

**Fail closed everywhere.** Unsupported validators, malformed locale tags,
missing mandatory anchors, unavailable strict-mode SafetyNet backends, and
invalid policies must surface typed errors or family-level safe fallback.

**Protected-path enforcement.** `rusqlite` and concrete SQLite audit behavior
belong in `gaze-audit`; the core library remains free of that dependency. The
Dylint protected-path gate is part of the architecture, not an optional hygiene
check.

**Bundle activation is explicit.**

| Invocation shape | Active national / postal recognizers | Adopter implication |
| --- | --- | --- |
| `core` bundled rulepack | Core global recognizers only. | Use for the narrow default surface. |
| `core-extended` with no policy | German and US national phone and postal recognizers activate through bundled defaults. | Pass `--locale=global` or a narrower policy if this is not wanted. |
| Policy with locale gates | Recognizers run only when their locales intersect the active locale chain. | Put activation intent in TOML rather than relying on host locale guesses. |
| Custom rulepack | Rulepack defaults fill in only below CLI and policy locale choices. | Keep rulepack defaults conservative and documented. |

Source anchors: [CLAUDE.md](./CLAUDE.md),
[crates/gaze-cli/src/pipeline/run.rs](./crates/gaze-cli/src/pipeline/run.rs),
[crates/gaze-assembly/src/defaults.rs](./crates/gaze-assembly/src/defaults.rs),
and [docs/architecture/locale-chain.md](./docs/architecture/locale-chain.md).

**Ambiguity is a side channel, not a leak.** Validator vetoes, collision-family
ties, no-anchor fallback, and related metadata travel as structured audit and
manifest-side metadata while clean output remains pseudonymized.

Source anchors:
[docs/architecture/ambiguity-side-channel.md](./docs/architecture/ambiguity-side-channel.md),
[docs/architecture/validator-veto.md](./docs/architecture/validator-veto.md),
[docs/architecture/collision-family.md](./docs/architecture/collision-family.md),
and [crates/gaze-audit/src/sqlite.rs](./crates/gaze-audit/src/sqlite.rs).

## Deep-Dive Companions

- [docs/architecture/validator-veto.md](./docs/architecture/validator-veto.md)
  explains validator-backed candidate rejection before conflict resolution.
- [docs/architecture/collision-family.md](./docs/architecture/collision-family.md)
  defines cross-class rivalry policy and family-level fallback.
- [docs/architecture/anchor-resolution.md](./docs/architecture/anchor-resolution.md)
  covers mandatory anchors, locale cue bundles, and no-anchor behavior.
- [docs/architecture/ambiguity-side-channel.md](./docs/architecture/ambiguity-side-channel.md)
  documents structured metadata for validator failures and ambiguity records.
- [docs/architecture/mcp-runtime.md](./docs/architecture/mcp-runtime.md)
  describes the MCP chokepoint, sealed tool context, tiers, and rmcp sink.
- [docs/architecture/safety-nets.md](./docs/architecture/safety-nets.md)
  defines observer-only SafetyNet behavior, subprocess hardening, and audit.
- `docs/architecture/proxy-runtime.md` is the v0.8 new/planned deep dive for
  the user-to-model proxy runtime and provider-driver pattern.

Related companion docs:
[docs/architecture/crates.md](./docs/architecture/crates.md),
[docs/architecture/document-extension.md](./docs/architecture/document-extension.md),
[docs/architecture/feedback-loop.md](./docs/architecture/feedback-loop.md),
[docs/architecture/locale-chain.md](./docs/architecture/locale-chain.md), and
[docs/architecture/xtask.md](./docs/architecture/xtask.md).

## Non-Goals

- Per-recognizer rule documentation belongs in [docs/policy.md](./docs/policy.md).
- Release notes and chronology belong in [CHANGELOG.md](./CHANGELOG.md).
- Adopter quickstart material belongs in [README.md](./README.md).
- Version-to-version migration instructions belong in [UPGRADE.md](./UPGRADE.md)
  when present.
- This document is not a substitute for source review before changing a
  correctness-sensitive path.
