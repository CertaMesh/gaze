# gaze-assembly

[![Crates.io](https://img.shields.io/crates/v/gaze-assembly.svg)](https://crates.io/crates/gaze-assembly)
[![docs.rs](https://docs.rs/gaze-assembly/badge.svg)](https://docs.rs/gaze-assembly)
[![License](https://img.shields.io/crates/l/gaze-assembly.svg)](https://github.com/EmpireTwo/gaze#license)

Policy-to-pipeline assembly for Gaze

Part of the [Gaze](https://github.com/EmpireTwo/gaze) workspace — a reversible PII pseudonymization runtime for agentic LLM workflows.

This crate joins the core `gaze` policy model with the built-in recognizers
from `gaze-recognizers`. It exists to keep the dependency direction clean:
`gaze` defines the core contracts, `gaze-recognizers` implements shipped
backends, and `gaze-assembly` wires them together for CLI-style policy
execution.

Without this crate, either `gaze` would need to depend on
`gaze-recognizers`, creating an unnecessary backend dependency for every core
adopter, or every consumer would need to duplicate policy assembly logic.

## Cargo

```toml
[dependencies]
gaze-pii = "0.9.0"
gaze-assembly = "0.9.0"
gaze-recognizers = "0.9.0"
serde_json = "1"
```

Inside the workspace:

```toml
[dependencies]
gaze = { path = "../gaze" }
gaze-assembly = { path = "../gaze-assembly" }
gaze-recognizers = { path = "../gaze-recognizers" }
serde_json = "1"
```

## Public entry points

[`src/lib.rs`](src/lib.rs) exposes:

- `build_pipeline(policy, context, rulepacks, active_locales, ner_threshold)`
- `BuildError`

`build_pipeline` accepts:

| Argument | Type | Purpose |
|----------|------|---------|
| `policy` | `&gaze::Policy` | Parsed policy with detector specs, rule specs, rulepack config, and optional NER config. |
| `context` | `&gaze::Context` | Runtime dictionaries, class-map overrides, and fields. |
| `rulepacks` | `&[gaze::Rulepack]` | Loaded bundled or path rulepacks. |
| `active_locales` | `&gaze::LocaleChain` | Locale chain used to lower locale templates and constrain recognizers. |
| `ner_threshold` | `Option<f32>` | Caller override for policy NER threshold. |

It returns a fully built `gaze::Pipeline`.

## What it assembles

`build_pipeline` currently wires:

- policy regex detectors into `gaze_recognizers::RegexDetector`
- policy dictionary detectors into `gaze_recognizers::DictionaryRecognizer`
- rulepack regex recognizers, including locale pattern-template lowering
- rulepack dictionary recognizers
- context-only dictionaries that are not already registered by policy or
  rulepack recognizers
- policy `RuleSpec` values into `ClassRule`, `ColumnRule`, and `DefaultRule`
- optional NER model loading through `gaze_recognizers::NerRecognizer`

The function fails closed with `BuildError` when policy, rulepack, recognizer,
or pipeline construction fails.

## Minimal flow

```rust
use std::collections::HashMap;

use gaze::{Context, LocaleChain, Policy, Rulepack};

let policy: Policy = Policy::load_for_cli(policy_path)?;
let context = Context {
    dictionaries: HashMap::new(),
    class_map: HashMap::new(),
    fields: serde_json::Map::new(),
};
let rulepacks: Vec<Rulepack> = Vec::new();
let active_locales = LocaleChain::merge_policy_and_cli(None, None);

let pipeline = gaze_assembly::build_pipeline(
    &policy,
    &context,
    &rulepacks,
    &active_locales,
    None,
)?;
```

Consumers that need CLI-equivalent behavior must still load bundled/path
rulepacks, build `DictionaryBundle` values, resolve locale precedence, and
choose a session. See `crates/gaze-cli/src/main.rs` for that process-boundary
work.

## Class-map safety

Context `class_map` entries may override a dictionary recognizer's class. The
assembly layer only accepts that override when the resulting class is covered
by a tokenize-or-stricter rule (`Tokenize`, `Redact`, `FormatPreserve`, or
`Generalize`). Otherwise assembly fails closed with
`RulepackError::ClassMapOverrideClash`.

This check belongs here because it depends on the final policy rules and the
runtime context together.

## Locale template lowering

Rulepack regex recognizers may use supported pattern-template placeholders.
`gaze-assembly` lowers those placeholders after the active locale chain is
known. Generic placeholders use `{locale.<bucket>}` and lower from loaded
rulepack locale metadata such as `[locale.salutations] names = [...]`.
`{locale_email_headers}` remains a v0.4.2 compatibility alias for
`{locale.email_headers}` and is deprecated for removal in the v0.5 cycle.

Unknown placeholders fail closed with `RulepackError`; unknown locale buckets
fail closed with `PolicyError::UnknownLocaleBucket`.

## What belongs here

Put code in this crate when it is assembly glue between:

- `gaze::Policy`
- `gaze::Context`
- `gaze::Rulepack`
- `gaze::LocaleChain`
- recognizers from `gaze-recognizers`

Do not put core contracts here. Those belong in `gaze`. Do not put backend
implementation here. Those belong in `gaze-recognizers`.
