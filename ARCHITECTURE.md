# Gaze Architecture

## Purpose And Audience

This document is the root architecture map for contributors and adopters who
need to understand how Gaze's crates fit together before reading individual
crate READMEs or deep-dive design notes.

Gaze's north star is defined in [AGENTS.md](AGENTS.md): reliable, reversible
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

Source anchors: [crates/gaze/src/pipeline.rs](crates/gaze/src/pipeline.rs),
[crates/gaze/src/resolver.rs](crates/gaze/src/resolver.rs),
[crates/gaze/src/registry.rs](crates/gaze/src/registry.rs),
[crates/gaze-types/src/lib.rs](crates/gaze-types/src/lib.rs), and
[docs/explanation/safety-net/safety-nets.md](docs/explanation/safety-net/safety-nets.md).

## Crate Map

The workspace currently has nine published-shape crates plus internal `xtask`.
For the fuller crate boundary table, see
[docs/reference/crates.md](docs/reference/crates.md).

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

LLM API proxy (shipped in v0.8)
  User or agent LLM request authenticated by API key
    -> gaze-proxy provider driver
    -> vendor API (OpenAI / Anthropic / Gemini)
    -> restore path under owner control
```

`gaze::Pipeline` is the library API for applications that already control their
data path. `gaze-mcp-core` and `gaze-mcp-rmcp` cover the model-to-source axis:
agent tool calls, document tools, manifest handles, and tiered restore access.
They explicitly do not cover raw API-key-authenticated request traffic from an
SDK or agent host; that belongs to `gaze-proxy`.

The `gaze-proxy` layer (shipped in v0.8) is a provider-driver runtime for the
user-to-model axis. Its architecture is adapter-oriented: one proxy core owns
request/response pseudonymization, while provider drivers isolate
vendor-specific request shapes and streaming behavior. Scope is
API-key-authenticated traffic to `api.openai.com`, `api.anthropic.com`, and
`generativelanguage.googleapis.com`; consumer subscription tiers (web-tier
cookie auth) are out of scope and covered by a separate browser-MITM project.

Source anchors: [crates/gaze/src/pipeline.rs](crates/gaze/src/pipeline.rs),
[docs/explanation/mcp/mcp-runtime.md](docs/explanation/mcp/mcp-runtime.md),
[crates/gaze-mcp-core/src/lib.rs](crates/gaze-mcp-core/src/lib.rs), and
[crates/gaze-mcp-rmcp/src/lib.rs](crates/gaze-mcp-rmcp/src/lib.rs).

## Key Design Decisions

### KDD-1: Reversibility First

Gaze is pseudonymization, not one-way redaction. The core contract emits clean
tokens plus a manifest that can restore owner-side originals; anything that
breaks clean/restore round trip is an architecture regression.

Source anchors: [AGENTS.md](AGENTS.md),
[crates/gaze/src/session.rs](crates/gaze/src/session.rs),
[crates/gaze/src/pipeline.rs](crates/gaze/src/pipeline.rs), and
[crates/gaze-types/src/lib.rs](crates/gaze-types/src/lib.rs).

### KDD-2: Rule-Based Detectors Are The Trust Floor

Precise classes should be handled by deterministic recognizers, validators,
dictionaries, and locale-aware rules before neural systems get involved.
Neural components are defense in depth; every emitted token must still trace
back to a recognizer, rule, or typed safety contract.

Source anchors: [AGENTS.md](AGENTS.md),
[crates/gaze/src/registry.rs](crates/gaze/src/registry.rs),
[crates/gaze-recognizers/src/regex.rs](crates/gaze-recognizers/src/regex.rs),
and [docs/explanation/safety-net/safety-nets.md](docs/explanation/safety-net/safety-nets.md).

### KDD-3: Audit Sink Isolation Is Enforced By Dylint

SQLite audit storage is isolated in `gaze-audit`; `gaze` must not grow a
`rusqlite` feature graph. The canonical protected-path gate is the
`gaze_module_isolation` Dylint lint in the detached `lint/dylint` workspace,
with the older syn walker decommissioned.

Source anchors: [CLAUDE.md](CLAUDE.md),
[docs/explanation/contributing/xtask-gates.md](docs/explanation/contributing/xtask-gates.md),
[crates/gaze-audit/src/sqlite.rs](crates/gaze-audit/src/sqlite.rs), and
[lint/dylint/src/lib.rs](lint/dylint/src/lib.rs).

### KDD-4: Closed Validator And Normalizer Surfaces Fail Closed

Validator and normalizer names parse into typed enums, and unknown names fail
at rulepack load with explicit unsupported-kind errors. The public enums are
`#[non_exhaustive]` for forward-compatible Rust matching, but runtime accepted
names remain closed and auditable.

Source anchors: [crates/gaze-types/src/lib.rs](crates/gaze-types/src/lib.rs),
[crates/gaze-recognizers/src/regex.rs](crates/gaze-recognizers/src/regex.rs),
[crates/gaze-recognizers/src/error.rs](crates/gaze-recognizers/src/error.rs),
and [crates/gaze/src/rulepack.rs](crates/gaze/src/rulepack.rs).

### KDD-5: Locale Resolution Has Four Tiers

Active locale resolution is ordered as CLI override, policy locale, rulepack
default locale, then system/default fallback. Recognizers declare locale gates,
and `LocaleTag::Other(_)` matching is strict rather than fuzzy.

Source anchors: [docs/explanation/policy/locale-chain.md](docs/explanation/policy/locale-chain.md),
[crates/gaze-types/src/lib.rs](crates/gaze-types/src/lib.rs),
[crates/gaze-cli/src/pipeline/run.rs](crates/gaze-cli/src/pipeline/run.rs),
and [crates/gaze-assembly/src/defaults.rs](crates/gaze-assembly/src/defaults.rs).

### KDD-6: Conflict Resolution Is Deterministic

When candidates overlap, Gaze resolves them in a fixed order: PII class
priority, rule priority, score, span length, and recognizer id. Collision-family
policy and mandatory anchors add fail-closed fallback, while `ConflictTier`
keeps losers visible in the audit trail.

Source anchors: [crates/gaze/src/resolver.rs](crates/gaze/src/resolver.rs),
[crates/gaze/src/pipeline.rs](crates/gaze/src/pipeline.rs),
[crates/gaze-types/src/lib.rs](crates/gaze-types/src/lib.rs),
[docs/explanation/detection/collision-family.md](docs/explanation/detection/collision-family.md),
and [docs/explanation/detection/anchor-resolution.md](docs/explanation/detection/anchor-resolution.md).

### KDD-7: Pass-3 SafetyNet Is Observer-Only

SafetyNet runs after tokenization against already-clean output and the runtime
manifest. It may emit `LeakSuspect` metadata, warnings, or strict-mode failures,
but it must not mutate clean text or add restore mappings.

Source anchors: [docs/explanation/safety-net/safety-nets.md](docs/explanation/safety-net/safety-nets.md),
[crates/gaze/src/pipeline.rs](crates/gaze/src/pipeline.rs),
[crates/gaze-recognizers/src/safety_net/test_support.rs](crates/gaze-recognizers/src/safety_net/test_support.rs),
and [crates/gaze/tests/safety_net.rs](crates/gaze/tests/safety_net.rs).

### KDD-8: Proxy Providers Use Adapter Drivers (shipped in v0.8)

The `gaze-proxy` runtime (shipped in v0.8.0) isolates vendor-specific API
shape in provider drivers while the proxy core owns pseudonymization, manifest
handling, restore boundaries, and fail-closed behavior. Adapters ship for
OpenAI, Anthropic, and Gemini API-key paths.

Source anchors: [docs/explanation/mcp/mcp-runtime.md](docs/explanation/mcp/mcp-runtime.md),
[crates/gaze-mcp-core/src/lib.rs](crates/gaze-mcp-core/src/lib.rs), and
[crates/gaze-mcp-rmcp/src/lib.rs](crates/gaze-mcp-rmcp/src/lib.rs).

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
| `core` bundled rulepack | Format-basis identifiers, including US national phone, run for every document locale. DE national phone and both postal recognizers remain document-gated. | Disable an unwanted format-basis recognizer outright; locale mismatch is not a suppression mechanism. |
| `core-extended` with no policy | Compatibility defaults also activate the quarantined DE national phone and postal recognizers. | Prefer `core`; use an explicit policy/rulepack when the compatibility activation is too broad. |
| Policy with locale gates | Locale gates apply to `locale_basis = "document"` recognizers only. Format-basis recognizers run once outside locale fallback. | Put document-language intent in TOML, but use `enabled = false` for intentional format suppression. |
| Custom rulepack | Omitted `locale_basis` retains legacy document gating; rulepack defaults fill in only below CLI and policy locale choices. | Opt into format basis only after collision and negative-corpus review. |

Source anchors: [CLAUDE.md](CLAUDE.md),
[crates/gaze-cli/src/pipeline/run.rs](crates/gaze-cli/src/pipeline/run.rs),
[crates/gaze-assembly/src/defaults.rs](crates/gaze-assembly/src/defaults.rs),
and [docs/explanation/policy/locale-chain.md](docs/explanation/policy/locale-chain.md).

**Ambiguity is a side channel, not a leak.** Validator vetoes, collision-family
ties, no-anchor fallback, and related metadata travel as structured audit and
manifest-side metadata while clean output remains pseudonymized.

Source anchors:
[docs/explanation/detection/ambiguity-side-channel.md](docs/explanation/detection/ambiguity-side-channel.md),
[docs/explanation/detection/validator-veto.md](docs/explanation/detection/validator-veto.md),
[docs/explanation/detection/collision-family.md](docs/explanation/detection/collision-family.md),
and [crates/gaze-audit/src/sqlite.rs](crates/gaze-audit/src/sqlite.rs).

## Deep-Dive Companions

- [docs/explanation/detection/validator-veto.md](docs/explanation/detection/validator-veto.md)
  explains validator-backed candidate rejection before conflict resolution.
- [docs/explanation/detection/collision-family.md](docs/explanation/detection/collision-family.md)
  defines cross-class rivalry policy and family-level fallback.
- [docs/explanation/detection/anchor-resolution.md](docs/explanation/detection/anchor-resolution.md)
  covers mandatory anchors, locale cue bundles, and no-anchor behavior.
- [docs/explanation/detection/ambiguity-side-channel.md](docs/explanation/detection/ambiguity-side-channel.md)
  documents structured metadata for validator failures and ambiguity records.
- [docs/explanation/mcp/mcp-runtime.md](docs/explanation/mcp/mcp-runtime.md)
  describes the MCP chokepoint, sealed tool context, tiers, and rmcp sink.
- [docs/explanation/safety-net/safety-nets.md](docs/explanation/safety-net/safety-nets.md)
  defines observer-only SafetyNet behavior, subprocess hardening, and audit.
- [docs/reference/metrics.md](docs/reference/metrics.md) catalogs every observable surface
  (audit-row columns, conflict tiers, SafetyNet benchmark snapshot fields,
  recognizer registry, pipeline observability, `BundleReport`, MCP `ToolCtx`,
  and CLI exit codes) with file-line pointers and stability guarantees.
- `docs/explanation/proxy/proxy-runtime.md` is the deep dive for the user-to-model
  proxy runtime and provider-driver pattern (shipped in v0.8).

Related companion docs:
[docs/reference/crates.md](docs/reference/crates.md),
[docs/explanation/document/document-extension.md](docs/explanation/document/document-extension.md),
[docs/explanation/detection/feedback-loop.md](docs/explanation/detection/feedback-loop.md),
[docs/explanation/policy/locale-chain.md](docs/explanation/policy/locale-chain.md), and
[docs/explanation/contributing/xtask-gates.md](docs/explanation/contributing/xtask-gates.md).

## Non-Goals

- Per-recognizer rule documentation belongs in [docs/reference/policy.md](docs/reference/policy.md).
- Release notes and chronology belong in [CHANGELOG.md](CHANGELOG.md).
- Adopter quickstart material belongs in [README.md](README.md).
- Version-to-version migration instructions belong in [UPGRADE.md](UPGRADE.md)
  when present.
- This document is not a substitute for source review before changing a
  correctness-sensitive path.
