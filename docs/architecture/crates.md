# Crates

Gaze is a Rust workspace split around the boundary between the reversible
pseudonymization core, recognizer backends, CLI assembly, and internal tooling.
The root [`Cargo.toml`](../../Cargo.toml) is the workspace source of truth.

## Workspace map

| Crate | Role | Key types / entry points | Depends on | Depended on by | When to use |
|-------|------|--------------------------|------------|----------------|-------------|
| [`gaze-types`](../../crates/gaze-types) | Shared value contracts for Gaze crates and adopters. | `Recognizer`, `Detector`, `Detection`, `Candidate`, `DetectContext`, `PiiClass`, `Action`, `RedactionEntry`, `RedactionLogger`, `RedactionLogError`, `LocaleChain`, `DictionaryBundle`, `RawDocument`, `CleanDocument`. | `serde` only. | `gaze`, `gaze-audit`, `gaze-recognizers`, and consumers that need contract types without runtime or recognizer dependencies. | Use when an adapter, restore-side crate, or lint needs the public value contract without pulling SQLite, policy loading, ONNX, tokenizers, or built-in recognizers. |
| [`gaze`](../../crates/gaze) | Core reversible pseudonymization library. Owns policies, sessions, token restore, rulepacks, registries, and redaction-log dispatch. Re-exports shared contract types from `gaze-types` for backwards compatibility. | `Pipeline`, `PipelineBuilder`, `Session`, `Policy`, `RecognizerRegistry`, `Rulepack`, `SensitiveSnapshot`; re-exported `PiiClass`, `Action`, `LocaleChain`, `RawDocument`, `CleanDocument`, `RedactionLogger`. | `gaze-types` plus external runtime dependencies. `gaze-recognizers` is optional behind `bundled-recognizers` and enabled by default. No normal dependency on `gaze-audit`. | `gaze-assembly`, `gaze-cli`, and downstream consumer projects. | Use from adapters or applications that want to construct pipelines directly and keep control over recognizer registration, session lifetime, restore, and audit behavior. |
| [`gaze-audit`](../../crates/gaze-audit) | Passive SQLite audit sink and query surface. | `SqliteLogger`, `AuditFilter`, `AuditLogRow`, `build_audit_query_sql`, `AUDIT_RESTRICTED_COLUMNS`; implements `gaze_types::RedactionLogger` directly. | `gaze-types`, `rusqlite`. | `gaze-cli`, compatibility tests, and adopters that want a SQLite audit sink. | Use when an application wants the concrete SQLite redaction-log sink or audit-query API. |
| [`gaze-assembly`](../../crates/gaze-assembly) | Policy-to-pipeline builder. Converts loaded policy, context, rulepacks, active locales, and NER threshold into a core `Pipeline`. | `build_pipeline(policy, context, rulepacks, active_locales, ner_threshold)`, `BuildError`. | `gaze`, `gaze-recognizers`. | `gaze-cli`. | Use when you want the same policy/rulepack assembly path as the CLI without copying CLI code. This crate exists so `gaze` does not depend on built-in recognizers. |
| [`gaze-recognizers`](../../crates/gaze-recognizers) | Built-in recognizer backends, embedded rulepacks, and the v0.6 safety-net adapter. | `RegexDetector`, `DictionaryRecognizer`, `NerRecognizer`, `NerDetector`, `NerOptions`, `NormalizerKind`, `ValidatorKind`, `embedded(name)`; v0.6 adds `OpenAiFilterSafetyNet`, `OpenAiFilterBackend`, `SubprocessOpenAiFilterConfig`, and `class_map` (gated by `safety-net-openai`). | `gaze-types` plus backend dependencies such as `regex`, `aho-corasick`, `ort`, and `tokenizers`; `gaze` only as a dev-dependency for tests. | `gaze-assembly`, `gaze-cli`, and downstream consumer projects; `gaze` can include it via the default `bundled-recognizers` feature. | Use when an adopter wants the shipped regex, dictionary, or ONNX NER recognizers, or the v0.6 OpenAI Privacy Filter safety net, instead of implementing `gaze::Recognizer` or `gaze_types::SafetyNet` directly. |
| [`gaze-cli`](../../crates/gaze-cli) | Published `gaze` binary for pipe-mode integrations. | Binary `gaze`; subcommands `clean`, `restore`, `audit query`, `audit export`, `audit purge`, `audit safety-net query`; flags such as `--policy`, `--locale`, `--context-json`, `--audit-db`, `--restore-mode`, `--safety-net`, `--openai-filter-command`, `--openai-filter-checkpoint`, `--safety-net-mode`. | `gaze`, `gaze-assembly`, `gaze-recognizers`, `gaze-audit`. | External host adapters and shell integrations. | Use from language adapters or scripts that need a stable process boundary rather than linking Rust. |
| [`gaze-mcp-core`](../../crates/gaze-mcp-core) | Transport-free MCP-shaped chokepoint runtime. New in v0.7.0. | `Tool` trait, sealed `ToolCtx`, `ToolRegistry`, `PiiEnvelope::dispatch`, `Frontend`, `DispatchHost`, `ManifestStore`, `AuthHook`, `SessionIdPolicy`. | `gaze`, `gaze-types`, `gaze-recognizers`, `gaze-assembly`, `gaze-audit`. | `gaze-mcp-rmcp`, adopters writing custom MCP transports. | Use to build an MCP-protocol tool host where every tool call passes through the Gaze pseudonymization chokepoint, independent of transport. See [`mcp-runtime.md`](mcp-runtime.md). |
| [`gaze-mcp-rmcp`](../../crates/gaze-mcp-rmcp) | rmcp transport sink for `gaze-mcp-core`. New in v0.7.0. | `RmcpFrontend`, stdio default transport, opt-in streamable HTTP transport, adopter-supplied `PrincipalResolver`. | `gaze-mcp-core`, `rmcp`. | Adopters wiring `gaze-mcp-core` into the rmcp protocol crate. | Use when the host should speak the MCP protocol over rmcp transports without re-implementing message framing. |
| [`gaze-document`](../../crates/gaze-document) | Published SafeBundle document-ingestion crate. New in v0.7.1. | `write_bundle`, `SafeBundle`, `BundleReport`, Tesseract OCR adapter, optional PDF rasterization, and opt-in MCP document tools. See [`document-extension.md`](document-extension.md). | `gaze`, `gaze-types`, `gaze-recognizers`; optional `gaze-mcp-core`. | `gaze-cli` behind the `document` feature and adopters ingesting PNG/JPG/PDF documents. | Use when PNG/JPG/PDF input needs to become an agent-safe `clean.md` plus owner-only manifest and versioned report. |
| [`xtask`](../../crates/xtask) | Internal gate runner. Not published. | Binary `xtask`; gates `symmetric-potemkin`, `class-map-override-safety` (active since v0.4.4 and extended for the safety-net OPF label allowlist in v0.6), `recognizer-composition-validator`, `no-tenant-knowledge` (added v0.4.3), `safety-net-sanity` (v0.6). | `anyhow`, `clap`; shells out to `cargo test` or scans production source. | CI and maintainers. | Use when adding or verifying regression gates that must run real behavioral tests. |

## Dependency direction

```text
gaze-types
  ^     ^
  |     |
gaze  gaze-recognizers
  ^          ^
  |          |
  +---- gaze-assembly
             ^
             |
          gaze-cli

gaze + gaze-assembly + gaze-recognizers + gaze-audit + gaze-types
                       ^
                       |
                 gaze-mcp-core
                       ^
                       |
                 gaze-mcp-rmcp -> rmcp

xtask -> cargo test subprocesses
```

The important boundary is that `gaze-recognizers` depends on `gaze-types`, not
on `gaze`. The shared contract crate defines recognizer traits and value
types; `gaze` owns policy, rulepacks, registries, session store, token grammar,
and restore path. The recognizer crate implements concrete backends against the
shared contract surface.
`gaze-assembly` is the joining layer for CLI-style policy execution.

## Published vs internal

Published crates:

- `gaze`
- `gaze-types`
- `gaze-audit`
- `gaze-assembly`
- `gaze-recognizers`
- `gaze-cli`
- `gaze-mcp-core` (v0.7.0+)
- `gaze-mcp-rmcp` (v0.7.0+)

Internal crates:

- `xtask`

Internal crates can depend on the published crates, but published crates should
not grow dependencies on internal tooling. If a feature needs to be reusable by
adopters, put the shared value contract in `gaze-types`, core runtime behavior
in `gaze`, the built-in backend in `gaze-recognizers`, and the policy assembly
glue in `gaze-assembly`.

## Choosing a crate for new work

Put code in `gaze-types` when it is a shared value contract or trait that
adopters and internal crates need without pulling runtime, SQLite, or recognizer
dependencies.

Put code in `gaze` when it is part of the core runtime: reversible sessions,
restore, policy parsing, rule evaluation, rulepack loading, registry
implementation, validation contracts, sandbox contracts, or redaction-log
storage.

Put code in `gaze-recognizers` when it implements a concrete detector backend
or bundled rulepack data. Built-in regex, dictionary, and NER behavior belongs
there because those backends depend on implementation crates that the core
contract should not force onto every adopter.

Put code in `gaze-assembly` when it wires `Policy`, `Context`, `Rulepack`,
`LocaleChain`, and built-in recognizers into a `Pipeline`. This is where
CLI-equivalent policy execution should live.

Put code in `gaze-cli` when it is process-boundary behavior: argv parsing,
stdin/stdout JSON, sanitized stderr, exit codes, policy-file loading,
context-file loading, audit database path handling, or restore-mode handling.

Put code in `xtask` when it is a repository gate. Gates must prove behavior by
running named tests; see [xtask](xtask.md).

Put code in `gaze-mcp-core` when it is transport-free runtime behavior for the
MCP-protocol chokepoint: tool registration, envelope dispatch, manifest store,
auth hooks, session-id policy. Anything that names a specific transport (stdio
framing, HTTP streaming) does not belong here. See [`mcp-runtime.md`](mcp-runtime.md).

Put code in `gaze-mcp-rmcp` when it bridges `gaze-mcp-core` to the rmcp
protocol crate: transport selection, principal resolution, server boot. Adopters
that need a non-rmcp transport implement their own sink against
`gaze-mcp-core::Frontend` and do not depend on this crate.

The MCP debug consumer formerly housed in `debug-proxy` now lives in a
separate downstream project (private repo at time of writing). Keep
consumer-specific database/log adapter work there unless the change belongs
in Gaze's reusable runtime, recognizers, assembly layer, CLI, or gates.

## Safety-net feature gates (v0.6+)

The OpenAI Privacy Filter safety net is gated off by default so existing
clean/restore consumers see no dependency-graph change.

| Crate | Feature | What it activates |
|-------|---------|-------------------|
| `gaze` | `safety-net` | `Pipeline::with_safety_net`, `clean_with_safety_net_detect_context`, the `SafetyNet` re-exports from `gaze-types`. |
| `gaze-recognizers` | `safety-net` | The trait surface plus the `MockSafetyNet` test helper. |
| `gaze-recognizers` | `safety-net-openai` | The OPF subprocess adapter (`OpenAiFilterSafetyNet`, `SubprocessOpenAiFilterConfig`, `class_map`). Implies `safety-net`. |
| `gaze-cli` | `safety-net-openai` | The `--safety-net=openai-filter` flag set, `audit safety-net query` subcommand, and exit-code mapping for `SafetyNetError`. |

The contract architecture, observer-only invariants, OPF adapter boundary,
stderr discipline, replay hash, and `safety_net_log` schema are documented in
[`safety-nets.md`](safety-nets.md).
