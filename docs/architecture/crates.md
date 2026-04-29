# Crates

Gaze is a Rust workspace split around the boundary between the reversible
pseudonymization core, recognizer backends, CLI assembly, and internal tooling.
The root [`Cargo.toml`](../../Cargo.toml) is the workspace source of truth.

## Workspace map

| Crate | Role | Key types / entry points | Depends on | Depended on by | When to use |
|-------|------|--------------------------|------------|----------------|-------------|
| [`gaze-types`](../../crates/gaze-types) | Shared value contracts for Gaze crates and adopters. | `Recognizer`, `Detector`, `Detection`, `Candidate`, `DetectContext`, `PiiClass`, `Action`, `RedactionEntry`, `LocaleChain`, `DictionaryBundle`, `RawDocument`, `CleanDocument`. | `serde` only. | `gaze`, `gaze-recognizers`, and consumers that need contract types without runtime or recognizer dependencies. | Use when an adapter, restore-side crate, or lint needs the public value contract without pulling SQLite, policy loading, ONNX, tokenizers, or built-in recognizers. |
| [`gaze`](../../crates/gaze) | Core reversible pseudonymization library. Owns policies, sessions, token restore, rulepacks, registries, and redaction logging. Re-exports shared contract types from `gaze-types` for backwards compatibility. | `Pipeline`, `PipelineBuilder`, `Session`, `Policy`, `RecognizerRegistry`, `Rulepack`, `SensitiveSnapshot`; re-exported `PiiClass`, `Action`, `LocaleChain`, `RawDocument`, `CleanDocument`. | `gaze-types` plus external runtime dependencies. `gaze-recognizers` is optional behind `bundled-recognizers` and enabled by default. | `gaze-assembly`, `gaze-cli`, and external consumers such as [piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens). | Use from adapters or applications that want to construct pipelines directly and keep control over recognizer registration, session lifetime, restore, and audit behavior. |
| [`gaze-assembly`](../../crates/gaze-assembly) | Policy-to-pipeline builder. Converts loaded policy, context, rulepacks, active locales, and NER threshold into a core `Pipeline`. | `build_pipeline(policy, context, rulepacks, active_locales, ner_threshold)`, `BuildError`. | `gaze`, `gaze-recognizers`. | `gaze-cli`. | Use when you want the same policy/rulepack assembly path as the CLI without copying CLI code. This crate exists so `gaze` does not depend on built-in recognizers. |
| [`gaze-recognizers`](../../crates/gaze-recognizers) | Built-in recognizer backends, embedded rulepacks, and the v0.6 safety-net adapter. | `RegexDetector`, `DictionaryRecognizer`, `NerRecognizer`, `NerDetector`, `NerOptions`, `NormalizerKind`, `ValidatorKind`, `embedded(name)`; v0.6 adds `OpenAiFilterSafetyNet`, `OpenAiFilterBackend`, `SubprocessOpenAiFilterConfig`, and `class_map` (gated by `safety-net-openai`). | `gaze-types` plus backend dependencies such as `regex`, `aho-corasick`, `ort`, and `tokenizers`; `gaze` only as a dev-dependency for tests. | `gaze-assembly`, `gaze-cli`, and external consumers such as [piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens); `gaze` can include it via the default `bundled-recognizers` feature. | Use when an adopter wants the shipped regex, dictionary, or ONNX NER recognizers, or the v0.6 OpenAI Privacy Filter safety net, instead of implementing `gaze::Recognizer` or `gaze_types::SafetyNet` directly. |
| [`gaze-cli`](../../crates/gaze-cli) | Published `gaze` binary for pipe-mode integrations. | Binary `gaze`; subcommands `clean`, `restore`, `audit query`, `audit export`, `audit purge`, `audit safety-net query`; flags such as `--policy`, `--locale`, `--context-json`, `--audit-db`, `--restore-mode`, `--safety-net`, `--openai-filter-command`, `--openai-filter-checkpoint`, `--safety-net-mode`. | `gaze`, `gaze-assembly`, `gaze-recognizers`, `gaze-audit`. | External host adapters and shell integrations. | Use from language adapters or scripts that need a stable process boundary rather than linking Rust. |
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
- `gaze-assembly`
- `gaze-recognizers`
- `gaze-cli`

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

The MCP debug consumer formerly housed in `debug-proxy` now lives in
[piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens). Keep consumer-specific
database/log adapter work there unless the change belongs in Gaze's reusable
runtime, recognizers, assembly layer, CLI, or gates.

## v0.6 safety-net feature gates

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
