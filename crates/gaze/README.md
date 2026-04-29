# gaze

Core reversible PII pseudonymization library for Gaze.

This crate owns the contracts that must remain stable for adopters:
`Pipeline`, `Session`, `Policy`, `RecognizerRegistry`, `LocaleChain`, the
rulepack schema, token shapes, restore, and audit logging. It deliberately
does not depend on `gaze-recognizers`; concrete recognizer backends live in a
separate crate and plug into the core `Recognizer` surface.

## Cargo

```toml
[dependencies]
gaze = "0.4.4"
```

When developing inside the workspace, use the path dependency:

```toml
[dependencies]
gaze = { path = "../gaze" }
```

## Public entry points

The public surface is re-exported from [`src/lib.rs`](src/lib.rs).

| Area | Types |
|------|-------|
| Pipeline execution | `Pipeline`, `PipelineBuilder`, `Error`, `Result` |
| Sessions and restore | `Session`, `Scope`, `SensitiveSnapshot` |
| Policy model | `Policy`, `PolicyError`, `DetectorSpec`, `DetectorKind`, `RuleSpec`, `RulepackPolicy`, `NerPolicy`, `SessionPolicy`, `SessionScope`, `DEFAULT_NER_THRESHOLD` |
| Recognizer API | `Recognizer`, `RecognizerRegistry`, `RecognizerRegistryBuilder`, `DetectContext`, `Candidate`, `Validator`, `ValidationResult`, `Canonicalizer` |
| Locale chain | `LocaleChain`, `LocaleTag`, `LocaleError` |
| Rulepacks | `Rulepack`, `RulepackSource`, `RulepackError`, `RecognizerSpec`, `RawMatch`, `ContextSpec`, `TokenSpec`, `LocaleData`, `recognizer_composition_validator` |
| Rules and classes | `PiiClass`, `BUILTIN_CLASS_NAMES`, `Action`, `ClassRule`, `ColumnRule`, `DefaultRule`, `Rule`, `RuleContext` |
| Documents | `RawDocument`, `CleanDocument`, `Value` |
| Context dictionaries | `Context`, `TypedContext`, `RawContext`, `ContextDictionary`, `ContextFieldsRef`, `DictionaryBundle`, `DictionaryEntry`, `DictionarySource`, `RulepackDict` |
| Audit logging | `RedactionLogger`, `RedactionEntry` (carries `created_at` epoch ms since v0.4.4), `ConflictTier`, `DocumentKind`; concrete SQLite sinks live in `gaze-audit` |
| Sandbox contracts | `Sandbox`, `SandboxPlan`, `ExecPolicy`, `UntrustedExecRequest`, `ValidatedExecRequest`, `SandboxError` |

## Minimal library flow

```rust
use gaze::{Action, ClassRule, PiiClass, Pipeline, RawDocument, Scope, Session};
use gaze_recognizers::RegexDetector;

let pipeline = Pipeline::builder()
    .recognizer(RegexDetector::emails()?)
    .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
    .build()?;

let session = Session::new(Scope::Conversation("example-session".to_string()))?;
let clean = pipeline.redact(
    &session,
    RawDocument::Text("alice@example.invalid".to_string()),
)?;
```

The example uses `gaze-recognizers` for a built-in recognizer. The core crate
only requires an implementation of `gaze::Recognizer` or the legacy
`gaze::Detector` adapter accepted by `PipelineBuilder::detector`.

## Policy and sessions

`Policy::load_for_cli(path)` parses `policy.toml` with fail-closed validation.
`Session::from_policy(policy)` and `Session::from_policy_with_ttl_override`
construct the corresponding `Scope`.

The session owns the reversible mapping between raw values and emitted tokens.
Use `Session::export()` to create a signed `SensitiveSnapshot`, and
`Session::import(snapshot)` to restore it later. Ephemeral sessions cannot be
exported.

## Pipeline execution

Use `Pipeline::builder()` to register recognizers, rules, and optional
redaction loggers:

```rust
let pipeline = Pipeline::builder()
    .recognizer(my_recognizer)
    .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
    .redaction_logger(my_logger)
    .build()?;
```

Execution entry points:

- `Pipeline::redact(session, raw)` uses the default `global` locale chain.
- `Pipeline::redact_with_context(session, raw, locale_chain)` adds locale
  selection.
- `Pipeline::redact_with_detect_context(session, raw, locale_chain,
  dictionaries, detect_fields)` adds tenant dictionaries and structured
  context fields.

## LocaleChain

`LocaleChain` resolves recognizer eligibility from ordered locale tags. The
CLI path merges CLI, policy, rulepack default, and system default in that
precedence order. See [docs/architecture/locale-chain.md](../../docs/architecture/locale-chain.md).

## RecognizerRegistry

`RecognizerRegistry` runs recognizers through `DetectContext` and resolves
candidate conflicts before redaction. Implement `Recognizer` directly when a
backend needs locale eligibility, scores, priorities, token families, or
context dictionaries. Use `PipelineBuilder::recognizer` for the modern path.

Use `PipelineBuilder::detector` only for simple detector implementations that
emit `Detection` values without the full recognizer metadata.

## Validators and bundled rulepacks (v0.4.2+)

Validators and normalizers are closed enums declared in `gaze-recognizers`. The
core crate parses validator names from rulepack TOML and dispatches into those
enums; unknown names fail closed at rulepack load time with
`RulepackError::UnsupportedValidator` or `RulepackError::UnsupportedNormalizer`.

The rulepack schema currently accepts:

- validators: `email_rfc`, `e164_phone` (gated behind `phone-parser` feature),
  `luhn`, `iban_mod97`
- normalizers: `email_canonical`, `iban_canonical`

The shipped bundled rulepacks are `core` (always-on email + email-header
recognizers) and `core-extended` (opt-in shape-only phone, IPv4/IPv6, postal
codes plus validator-backed IBAN and credit card).

## Audit schema v2 (v0.4.4)

`RedactionEntry` carries a `created_at: i64` epoch-millisecond timestamp.
`SqliteLogger` opens with an `ALTER TABLE` migration so legacy v0.4.3 audit
databases without `created_at` remain queryable through a NULL default. Time
filtering is exposed through the CLI; see `crates/gaze-cli/README.md`.

## What belongs here

Put code in this crate when it is part of the reversible core contract:

- token grammar and restore
- session scope and snapshot validation
- policy parsing and typed policy errors
- recognizer traits and registry behavior
- locale matching
- rulepack schema and validation
- redaction-log contracts
- sandbox contracts

Concrete built-in backends belong in `gaze-recognizers`, and policy-to-pipeline
assembly that imports those backends belongs in `gaze-assembly`.
