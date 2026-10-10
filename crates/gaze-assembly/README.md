# gaze-assembly

[![Crates.io](https://img.shields.io/crates/v/gaze-assembly.svg)](https://crates.io/crates/gaze-assembly)
[![docs.rs](https://docs.rs/gaze-assembly/badge.svg)](https://docs.rs/gaze-assembly)
[![License](https://img.shields.io/crates/l/gaze-assembly.svg)](https://github.com/CertaMesh/gaze#license)

Builds a `gaze::Pipeline` from policy, context, rulepacks, locales, and built-in recognizers. Core contracts stay in `gaze`; detector implementations stay in `gaze-recognizers`.

## Cargo

```toml
[dependencies]
gaze-pii = "0.16.0"
gaze-assembly = "0.16.0"
gaze-recognizers = "0.16.0"
serde_json = "1"
```

## Minimal flow

```rust
use std::collections::HashMap;

use gaze::{Context, LocaleChain, Policy, Rulepack};

let policy: Policy = Policy::load_for_cli(policy_path)?;
let context = Context {
    dictionaries: HashMap::new(),
    class_map: HashMap::new(),
    fields: serde_json::Map::new(),
    record_match_kinds: Default::default(),
    record_value_rejections: Default::default(),
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

Consumers that need CLI-equivalent behavior call `resolve_policy_inputs`
first; it loads bundled/path rulepacks, builds the `DictionaryBundle`, and
resolves locale precedence. They still choose a session. See
`crates/gaze-cli/src/pipeline/build.rs` for the CLI's use of both calls.

## Public entry points

[`src/lib.rs`](src/lib.rs) exposes, among others:

- `CorePipelineConfig` / `CorePipeline`: the bundled-default pipeline
  (`core` rulepack plus locale-aware recognizers) for the common case
- `resolve_policy_inputs` / `ResolvedPolicyInputs`: resolves the rulepacks,
  dictionaries, locales, and NER threshold that `gaze clean --policy` passes
  to assembly
- `build_pipeline(policy, context, rulepacks, active_locales, ner_threshold)`
  and `build_pipeline_builder` (same arguments, returns the builder so a caller
  can add recognizers or a safety net)
- `attach_nym_safety_net` (feature `safety-net-nym`)
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

Assembly wires regex and dictionary detectors, locale templates, context-only dictionaries, policy rules, and optional NER. Construction errors return `BuildError`; assembly fails closed.

## Class-map safety

Context `class_map` entries may override a dictionary recognizer's class. The
assembly layer only accepts that override when the resulting class is covered
by a tokenize-or-stricter rule (`Tokenize`, `Redact`, `FormatPreserve`, or
`Generalize`). Otherwise assembly fails closed with
`RulepackError::ClassMapOverrideClash`.

## Locale template lowering

Rulepack regex recognizers may use supported pattern-template placeholders.
`gaze-assembly` lowers those placeholders after the active locale chain is
known. Generic placeholders use `{locale.<bucket>}` and lower from loaded
rulepack locale metadata such as `[locale.salutations] names = [...]`.
`{locale_email_headers}` remains a deprecated v0.4.2 compatibility alias for
`{locale.email_headers}`.

Unknown placeholders fail closed with `RulepackError`; unknown locale buckets
fail closed with `PolicyError::UnknownLocaleBucket`.

## What belongs here

Add glue that combines `Policy`, `Context`, `Rulepack`, `LocaleChain`, and `gaze-recognizers`. Keep core contracts and detector implementations in their own crates.
