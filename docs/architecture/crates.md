# Crates

Gaze is a Rust workspace split around the boundary between the reversible
pseudonymization core, recognizer backends, CLI assembly, and internal tooling.
The root [`Cargo.toml`](../../Cargo.toml) is the workspace source of truth.

## Workspace map

| Crate | Role | Key types / entry points | Depends on | Depended on by | When to use |
|-------|------|--------------------------|------------|----------------|-------------|
| [`gaze`](../../crates/gaze) | Core reversible pseudonymization library. Owns policies, sessions, token restore, locale chains, rulepacks, registries, and redaction logging. | `Pipeline`, `PipelineBuilder`, `Session`, `Policy`, `RecognizerRegistry`, `LocaleChain`, `Rulepack`, `PiiClass`, `Action`, `RawDocument`, `CleanDocument`, `SensitiveSnapshot`. | External crates only. `gaze-recognizers` is a dev-dependency for tests. | `gaze-assembly`, `gaze-recognizers`, `gaze-cli`, and external consumers such as [piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens). | Use from adapters or applications that want to construct pipelines directly and keep control over recognizer registration, session lifetime, restore, and audit behavior. |
| [`gaze-assembly`](../../crates/gaze-assembly) | Policy-to-pipeline builder. Converts loaded policy, context, rulepacks, active locales, and NER threshold into a core `Pipeline`. | `build_pipeline(policy, context, rulepacks, active_locales, ner_threshold)`, `BuildError`. | `gaze`, `gaze-recognizers`. | `gaze-cli`. | Use when you want the same policy/rulepack assembly path as the CLI without copying CLI code. This crate exists so `gaze` does not depend on built-in recognizers. |
| [`gaze-recognizers`](../../crates/gaze-recognizers) | Built-in recognizer backends and embedded rulepacks. | `RegexDetector`, `DictionaryRecognizer`, `NerRecognizer`, `NerDetector`, `NerOptions`, `NormalizerKind`, `ValidatorKind`, `embedded(name)`. | `gaze` plus backend dependencies such as `regex`, `aho-corasick`, `ort`, and `tokenizers`. | `gaze-assembly`, `gaze-cli`, and external consumers such as [piinuts/gaze-lens](https://github.com/PIInuts/gaze-lens); `gaze` tests use it as a dev-dependency. | Use when an adopter wants the shipped regex, dictionary, or ONNX NER recognizers instead of implementing `gaze::Recognizer` directly. |
| [`gaze-cli`](../../crates/gaze-cli) | Published `gaze` binary for pipe-mode integrations. | Binary `gaze`; subcommands `clean` and `restore`; flags such as `--policy`, `--locale`, `--context-json`, `--audit-db`, `--restore-mode`. | `gaze`, `gaze-assembly`, `gaze-recognizers`. | External host adapters and shell integrations. | Use from language adapters or scripts that need a stable process boundary rather than linking Rust. |
| [`xtask`](../../crates/xtask) | Internal gate runner. Not published. | Binary `xtask`; gates `symmetric-potemkin`, `class-map-override-safety` (active since v0.4.4), `recognizer-composition-validator`, `no-tenant-knowledge` (added v0.4.3). | `anyhow`, `clap`; shells out to `cargo test` or scans production source. | CI and maintainers. | Use when adding or verifying regression gates that must run real behavioral tests. |

## Dependency direction

```text
gaze
  ^  ^
  |  |
  |  +-- gaze-recognizers
  |         ^
  |         |
  +-- gaze-assembly
          ^
          |
       gaze-cli

xtask -> cargo test subprocesses
```

The important boundary is that `gaze` does not depend on
`gaze-recognizers`. The core crate defines the recognizer trait, registry,
policy model, rulepack schema, session store, token grammar, and restore path.
The recognizer crate implements concrete backends against that surface.
`gaze-assembly` is the joining layer for CLI-style policy execution.

## Published vs internal

Published crates:

- `gaze`
- `gaze-assembly`
- `gaze-recognizers`
- `gaze-cli`

Internal crates:

- `xtask`

Internal crates can depend on the published crates, but published crates should
not grow dependencies on internal tooling. If a feature needs to be reusable by
adopters, put the contract in `gaze`, the built-in backend in
`gaze-recognizers`, and the policy assembly glue in `gaze-assembly`.

## Choosing a crate for new work

Put code in `gaze` when it is part of the core contract: reversible sessions,
restore, policy parsing, rule evaluation, locale resolution, rulepack loading,
recognizer traits, validation contracts, sandbox contracts, or redaction-log
interfaces.

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
