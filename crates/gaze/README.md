# gaze-pii

[![Crates.io](https://img.shields.io/crates/v/gaze-pii.svg)](https://crates.io/crates/gaze-pii)
[![docs.rs](https://docs.rs/gaze-pii/badge.svg)](https://docs.rs/gaze-pii)
[![License](https://img.shields.io/crates/l/gaze-pii.svg)](https://github.com/EmpireTwo/gaze#license)

Reversible PII pseudonymization runtime for agentic workflows

Part of the [Gaze](https://github.com/EmpireTwo/gaze) workspace — a reversible PII pseudonymization runtime for agentic LLM workflows.

`gaze-pii` is the runtime crate for [Gaze](https://github.com/EmpireTwo/gaze). It owns the contracts that must stay stable for adopters: `Pipeline`, `Session`, `Policy`, `RecognizerRegistry`, the rulepack schema, token shape, and the signed restore manifest.

The crate is published as `gaze-pii`; the import path remains `use gaze::...` because `[lib].name = "gaze"` is preserved.

## Install

```toml
[dependencies]
gaze-pii = "0.7"
gaze-assembly = "0.7"
gaze-recognizers = "0.7"
```

`gaze-assembly` provides `CorePipelineConfig` — bundled defaults so you don't hand-wire the `core` rulepack (emails, names, locations, organizations, locale cues).

## Minimal example

```rust
use gaze::{CleanDocument, Scope, Session};
use gaze_assembly::CorePipelineConfig;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let core = CorePipelineConfig::new().build()?;
    let session = Session::new(Scope::Conversation("conv-42".into()))?;

    let CleanDocument::Text(clean) = core.redact_text(
        &session,
        "Email alice@example.invalid about ORD-789012.",
    )? else { unreachable!() };
    // "Email <{session_hex}:Email_1> about ORD-789012."

    // Persist `session.export()?` server-side, encrypted, keyed to the
    // conversation. Send only `clean` to the LLM.
    let _snapshot = session.export()?;

    Ok(())
}
```

For restore, scan the LLM response with `gaze::token_shape::pattern()` and call `session.restore_strict(token)` per match. Full walk-through: [`docs/getting-started.md`](https://github.com/EmpireTwo/gaze/blob/main/docs/getting-started.md).

## What this crate owns

| Area | Types |
|------|-------|
| Pipeline execution | `Pipeline`, `PipelineBuilder`, `Error`, `Result` |
| Sessions and restore | `Session`, `Scope`, `SensitiveSnapshot` |
| Policy model | `Policy`, `PolicyError`, `RuleSpec`, `RulepackPolicy`, `NerPolicy`, `SessionPolicy`, `SessionScope` |
| Recognizer API | `Recognizer`, `RecognizerRegistry`, `DetectContext`, `Candidate`, `Validator`, `Canonicalizer` |
| Locale chain | `LocaleChain`, `LocaleTag`, `LocaleError` |
| Rulepacks | `Rulepack`, `RulepackSource`, `RulepackError`, `RecognizerSpec`, `TokenSpec`, `LocaleData` |
| Rules and classes | `PiiClass`, `Action`, `ClassRule`, `ColumnRule`, `DefaultRule`, `Rule` |
| Documents | `RawDocument`, `CleanDocument`, `Value` |
| Audit logging | `RedactionLogger`, `RedactionEntry`, `ConflictTier`, `DocumentKind` (concrete SQLite sink: `gaze-audit`) |

The full re-export list lives in [`src/lib.rs`](src/lib.rs).

## What this crate does not own

- **Concrete recognizers.** Regex/dictionary/NER backends and bundled rulepacks live in `gaze-recognizers`.
- **Policy-to-pipeline assembly.** `CorePipelineConfig` and `build_pipeline` live in `gaze-assembly`.
- **SQLite audit sink.** `SqliteLogger` and the read-side audit query API live in `gaze-audit`. `gaze-pii` carries no `rusqlite` dependency in any feature graph.
- **CLI.** The `gaze` binary lives in `gaze-cli`.

## Guarantees

- **Fail closed** on unknown rulepack validators or normalizers — typed errors at load, no silent degradation.
- **Reversible by design.** Tokens are session-scoped and counted by class; restore goes through the signed snapshot, not string substitution.
- **Deterministic detection** as the floor. NER and the OpenAI-filter SafetyNet are opt-in observers and cannot mutate the manifest.
- **Auditable.** Every emitted token traces to a recognizer + rule. Conflict losers are logged with `decided_by: ConflictTier`.

Full project north star + five-axis contract: [AGENTS.md](https://github.com/EmpireTwo/gaze/blob/main/AGENTS.md#project-north-star).

## Features

| Feature | Default | Effect |
|---------|---------|--------|
| `bundled-recognizers` | on | Re-exports `gaze-recognizers` so adopters can register built-in detectors without an extra dependency. Disable for a recognizer-trait-only build. |
| `safety-net` | off | Compiles the Pass-3 SafetyNet observer surface. Activation also requires `gaze-cli`'s `safety-net-openai` feature for the OpenAI-filter subprocess device. |

## License

Dual-licensed under either of [Apache-2.0](https://github.com/EmpireTwo/gaze/blob/main/LICENSE-APACHE) or [MIT](https://github.com/EmpireTwo/gaze/blob/main/LICENSE-MIT), at your option.
