# Getting Started with Gaze

Working PII pseudonymization in about 10 minutes. You will clean a document, store the
restore key, send only safe text to an LLM, and restore original values from the response.

## Prerequisites

- Rust toolchain at MSRV `1.89` or newer (matches the workspace `rust-version`).
- For the CLI: `cargo install gaze-cli` or build from source.

## 1. Add dependencies

```toml
[dependencies]
gaze = "0.6"
gaze-assembly = "0.6"
```

`gaze-assembly` provides `CorePipelineConfig` -- bundled defaults (core rulepack:
emails, names, locations, organizations, plus optional locale-aware recognizers)
without manually wiring recognizers.

## 2. Clean a document

```rust
use gaze::{CleanDocument, RawDocument, Scope, Session};
use gaze_assembly::CorePipelineConfig;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build once; share across requests in long-running apps.
    let core = CorePipelineConfig::new().build()?;
    let pipeline = core.pipeline();

    // One Session per conversation -- it owns the token map.
    let session = Session::new(Scope::Conversation("conv-abc".into()))?;

    let cleaned = pipeline.redact(
        &session,
        RawDocument::Text("Hi, alice@example.invalid called about ORD-789012.".into()),
    )?;

    // CleanDocument is an enum: Text(String) or Structured(...). Destructure.
    let CleanDocument::Text(clean_text) = cleaned else {
        unreachable!("Text input produces Text output");
    };
    println!("{}", clean_text);
    // "Hi, <hex:Email_1> called about ORD-789012."
    // ORD-789012 needs a custom recognizer -- see Step 5.

    Ok(())
}
```

## 3. Export the restore key before calling the LLM

```rust
use gaze::{Scope, Session};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = Session::new(Scope::Conversation("conv-abc".into()))?;
    let clean_text = "Hi, <hex:Email_1> called about ORD-789012.";

    // Do this BEFORE sending clean text to the LLM.
    let snapshot = session.export()?;
    let blob: Vec<u8> = snapshot.into_bytes();

    // Store `blob` encrypted at rest, bound to this conversation/user.
    // NEVER send `blob` to the LLM, analytics, or logs.
    // Send only `clean_text` to the LLM.
    let _ = (blob, clean_text);

    Ok(())
}
```

## 4. Restore after the LLM responds

There is no `Pipeline::restore_text`. Scan the response for tokens with
`gaze::token_shape::pattern()` and call `Session::restore_strict` per token:

```rust,no_run
use gaze::{token_shape, SensitiveSnapshot, Session};

fn restore_text(session: &Session, text: &str) -> Result<String, gaze::Error> {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in token_shape::pattern().find_iter(text) {
        out.push_str(&text[last..m.start()]);
        out.push_str(&session.restore_strict(m.as_str())?);
        last = m.end();
    }
    out.push_str(&text[last..]);
    Ok(out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let blob: Vec<u8> = load_encrypted_snapshot_from_storage();

    let snapshot = SensitiveSnapshot::from(blob);
    let restored_session = Session::import(snapshot)?;
    let llm_response = "Thanks <hex:Email_1>, I have updated your record.";
    let restored = restore_text(&restored_session, llm_response)?;
    println!("{restored}");
    // "Thanks alice@example.invalid, I have updated your record."

    Ok(())
}

fn load_encrypted_snapshot_from_storage() -> Vec<u8> {
    Vec::new()
}
```

For a tolerant variant that leaves unknown tokens in place, use `Session::restore`
(returns `Option<String>`) or catch the error from `restore_strict`.

## 5. Add a policy for tenant-specific PII

```toml
# policy.toml
[session]
scope = "conversation"

[policy.rulepacks]
bundled = []

[[policy.custom_recognizers]]
kind = "regex"
name = "order-id"
class = "custom:order_id"        # lowercase; no Custom(...) syntax
pattern = '\bORD-\d{6,}\b'

[[rule]]
kind = "class"
class = "custom:order_id"
action = "tokenize"
```

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

## Common errors

| Error | Cause | Fix |
|-------|-------|-----|
| `PolicyError` (unknown field) | Typo in `policy.toml` | Check [docs/policy.md](policy.md) |
| `Error::BlobExpired { .. }` | Snapshot TTL elapsed | Increase `ttl` (a `Duration`) or refresh before expiry |
| Export errors on `Ephemeral` | Cannot restore from ephemeral sessions | Use `Scope::Conversation` |
| `RulepackError::UnsupportedValidator` | Unknown validator name | See valid names in [docs/policy.md](policy.md#validators) |
| Tokens not restored | Wrong session blob | The blob must come from the exact session that produced the clean output |

## Next steps

- [Policy reference](policy.md) -- full TOML schema, all recognizers, locale chain
- [CLI adapter contract](cli-adapter-contract.md) -- shell-out protocol for framework adapters
- [Security review](security-review.md) -- invariants, threat boundaries, audit isolation
- [Exit codes](../crates/gaze-cli/README.md#exit-codes) -- canonical exit code reference
- `cargo doc --open -p gaze` -- full API reference
