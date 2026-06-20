# Add a custom recognizer for tenant-specific PII

Gaze's bundled rulepacks cover common PII (emails, names, locations, organizations). For
data specific to your tenant — order IDs, song names, artist names, internal account
numbers — you add a **custom recognizer** in `policy.toml`. This guide shows the smallest
working example. For the complete schema see the [policy reference](../../reference/policy.md);
for an end-to-end run see the [Getting Started tutorial](../../tutorials/getting-started.md).

## 1. Declare the recognizer and a rule

A recognizer finds the spans; a rule says what to do with the class it emits. Add both to
your `policy.toml`:

```toml
[[policy.custom_recognizers]]
kind = "regex"
name = "order-id"
class = "custom:order_id"        # lowercase; no Custom(...) syntax
pattern = '\bORD-\d{6,}\b'

[[rule]]
kind = "class"
class = "custom:order_id"
action = "tokenize"              # tokenize keeps it restorable
```

Notes:

- The class name is a lowercase `custom:<name>` string — not the Rust `Custom(...)` form.
- `action = "tokenize"` emits a restorable token. Use `redact` or `generalize` only when you
  do *not* need to restore the value — those are one-way (see the
  [restore boundary](../../explanation/core/restore-boundary.md)).

## 2. Load the policy and build the pipeline

```rust
use std::collections::HashMap;
use std::path::Path;

use gaze::{Context, LocaleChain, Policy};

let policy = Policy::load(Path::new("policy.toml"))?;
let context = Context {
    dictionaries: HashMap::new(),
    class_map: HashMap::new(),
    fields: Default::default(),
};
let rulepacks = Vec::new();
let active_locales = LocaleChain::merge_policy_and_cli(None, None);

let pipeline = gaze_assembly::build_pipeline(
    &policy,
    &context,
    &rulepacks,
    &active_locales,
    None,
)?;
```

Now `pipeline.redact(...)` tokenizes `ORD-789012` alongside the bundled classes, and
`Session` restore reconstructs the original value byte-for-byte.

## Beyond regex

- **Validators and normalizers** — constrain or canonicalize a match (for example, checksum a
  number) without breaking restore. A normalizer must preserve the original byte span; see
  [recognizer normalizers preserve the original span](../../explanation/detection/recognizer-normalizer-spans.md).
- **Dictionaries and locale gating** — match against a tenant word list, or restrict a
  recognizer to specific locales. See the [policy reference](../../reference/policy.md) and
  the [locale chain](../../explanation/policy/locale-chain.md).
