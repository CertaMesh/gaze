# gaze-recognizers

[![Crates.io](https://img.shields.io/crates/v/gaze-recognizers.svg)](https://crates.io/crates/gaze-recognizers)
[![docs.rs](https://docs.rs/gaze-recognizers/badge.svg)](https://docs.rs/gaze-recognizers)
[![License](https://img.shields.io/crates/l/gaze-recognizers.svg)](https://github.com/EmpireTwo/gaze#license)

Built-in recognizers for Gaze

Part of the [Gaze](https://github.com/EmpireTwo/gaze) workspace — a reversible PII pseudonymization runtime for agentic LLM workflows.

This crate depends on `gaze` and implements concrete `gaze::Recognizer`
backends. Keeping it separate lets the core crate expose a small, stable
contract without forcing every adopter to compile regex, dictionary, ONNX, and
tokenizer dependencies.

## Cargo

```toml
[dependencies]
gaze-pii = "0.6.5"
gaze-recognizers = "0.6.5"
```

Library users that need parser-backed E.164 phone validation must opt in to the
`phone-parser` feature:

```toml
[dependencies]
gaze-recognizers = { version = "0.6.5", features = ["phone-parser"] }
```

`gaze-cli` enables `phone-parser` by default. Without the feature, the
rulepack loader rejects `e164_phone` at load time with
`RulepackError::UnsupportedValidator`, preserving the axis-1 fail-closed
posture rather than silently degrading to shape-only matching.

Inside the workspace:

```toml
[dependencies]
gaze = { path = "../gaze" }
gaze-recognizers = { path = "../gaze-recognizers" }
```

## Public entry points

The public surface is re-exported from [`src/lib.rs`](src/lib.rs).

| Backend | Public types |
|---------|--------------|
| Regex | `RegexDetector`, `NormalizerKind`, `ValidatorKind` |
| Dictionary | `DictionaryRecognizer` |
| Anchored match | `AnchoredMatchRecognizer`, `AnchoredBoundary`, `NameShape`, `CuePosition` |
| NER | `NerRecognizer`, `NerDetector`, `NerOptions`, `NerLoadError`, `NerBackendKind`, `LabelMap`, `VerifiedArtifacts` |
| Rulepacks | `embedded(name)` |

## Regex backend

`RegexDetector` can be constructed directly:

```rust
use gaze::PiiClass;
use gaze_recognizers::RegexDetector;

let recognizer = RegexDetector::new(
    r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b",
    PiiClass::Email,
)?;
```

Use `RegexDetector::emails()` for the built-in email recognizer. Rulepack
assembly uses `RegexDetector::with_rulepack_fields` so locale tags, scores,
priorities, token families, capture groups, exclusions, validators, and
normalizers can flow from TOML rulepacks into the registry.

Current validator and normalizer enums:

- `ValidatorKind::EmailRfc`
- `ValidatorKind::E164Phone` (requires the `phone-parser` feature)
- `ValidatorKind::Luhn` (Mod 10 checksum, used by `card.structural`)
- `ValidatorKind::IbanMod97` (ISO 7064 mod-97 IBAN checksum, used by `iban.structural`)
- `NormalizerKind::EmailCanonical`
- `NormalizerKind::IbanCanonical` (uppercase + whitespace strip, paired with `iban_mod97`)

`E164Phone` is implemented via the `phonenumber` crate. It preserves valid E.164
matches such as synthetic non-reachable `+49-30-0000-0000` (not a real number)
while rejecting regex-passing but unassigned shapes such as `+99999999`. Audit notes live in
[`docs/research/v0.4.4-phonenumber-audit.md`](../../docs/research/v0.4.4-phonenumber-audit.md).

## Anchored match backend

`AnchoredMatchRecognizer` is the v0.6 structural-context backend for
cue-anchored person-name detection. Bundled `core` uses it for
`name.forward_marker`, `name.agent_recipient`, and `name.auto_footer`; the
`locale-de` and `locale-en` bundles provide the `forward_markers`,
`agent_recipient_cues`, and `footer_cues` cue buckets.

Use it when a name appears near deterministic prompt or email structure, not
as a replacement for NER over general prose. Known limits and adopter migration
notes live in
[`docs/policy.md#known-limits---ner-and-prompt-shape`](../../docs/policy.md#known-limits---ner-and-prompt-shape).

## Dictionary backend

`DictionaryRecognizer` detects tenant or rulepack dictionaries supplied through
`gaze::DictionaryBundle` and `gaze::DetectContext`.

Use it for bounded adopter-specific PII such as order IDs, account handles,
internal project names, song titles, or artist names. The recognizer stores a
dictionary name and reads the actual terms from runtime context, policy, or
rulepack assembly.

Use adopter custom recognizers instead when the detector needs external
services, private model code, or domain-specific scoring that should not ship
as a built-in backend.

## NER backend

The NER backend is optional at runtime. `NerRecognizer` loads a verified ONNX
model bundle with `NerOptions`; `NerDetector` is part of the public surface for
the backend implementation.

Current dependencies include:

- `ort` for ONNX Runtime
- `tokenizers` for tokenizer execution
- `ndarray` for model tensors
- `phonenumber` (gated behind the `phone-parser` Cargo feature) for the
  parser-backed `E164Phone` validator

The expected production model family is Davlan mBERT NER, configured through
policy `[ner]` and loaded by `gaze-assembly` when `model_dir` is present.
Loading failures are policy configuration failures in the CLI path.

## Embedded rulepacks

`embedded(name)` returns bundled TOML contents for known rulepack names:

| Name | File | Purpose |
|------|------|---------|
| `core` | [`embedded/core.toml`](embedded/core.toml) | Global core recognizers, including email address, email-header display-name detection, and v0.6 anchored structural Name detection. |
| `core-extended` | [`embedded/core-extended.toml`](embedded/core-extended.toml) | Opt-in extension. Phase 1 (v0.4.2): shape-only E.164 phone numbers, IPv4/IPv6 addresses, `de-DE`/`en-US` postal codes. Phase 2 (v0.4.3): validator-backed IBAN (`iban.structural`, `iban_mod97` + `iban_canonical`) and credit card (`card.structural`, `luhn`). Phase 3 (v0.4.4): `e164_phone` parser-backed validator extends the existing `phone.structural` recognizer. Default `[[rule]]` entries ship in the rulepack so `--rulepack-bundled core,core-extended` tokenizes the new classes out of the box. |
| `locale-de` | [`embedded/locale-de.toml`](embedded/locale-de.toml) | DACH locale metadata such as German email headers. |
| `locale-en` | [`embedded/locale-en.toml`](embedded/locale-en.toml) | English locale metadata such as English email headers. |

The loader returns `None` for unknown names. Policy/CLI callers should treat
unknown bundled names as configuration errors.

## Adding recognizers here

Add a recognizer to this crate when it is a built-in backend Gaze should ship
for many adopters. The recognizer should implement `gaze::Recognizer` and
provide deterministic metadata:

- stable `id`
- supported `PiiClass`
- locale eligibility
- score and priority
- token family
- canonical form when a validator proves one
- source labels suitable for audit logs

Add adopter-specific recognizers outside this crate when the behavior is tied
to one tenant, one private schema, or one proprietary data source.

## Test support

The crate has a `test-support` feature for tests that need additional support
surface without making it part of the default public runtime.
