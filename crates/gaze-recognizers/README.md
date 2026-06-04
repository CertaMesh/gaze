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
gaze-pii = "0.10.1"
gaze-recognizers = "0.10.1"
```

Library users that need parser-backed E.164 phone validation must opt in to the
`phone-parser` feature:

```toml
[dependencies]
gaze-recognizers = { version = "0.10.1", features = ["phone-parser"] }
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
[`v0.4.4-phonenumber-audit.md`](https://github.com/PIInuts/business/blob/main/research/v0.4.4-phonenumber-audit.md)
(hosted in `PIInuts/business:research/`).

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
| `core` | [`embedded/core.toml`](embedded/core.toml) | Unified bundled recognizer set. Email/name, parser-backed phone, IBAN, payment-card, IP, ETH, and postal recognizers now live in one bundle. Each recognizer declares `safety_tier = "safe_default"`, `"locale_gated"`, or `"opt_in"` to control no-policy activation. |
| `core-extended` | alias of `core` | Deprecated since v0.8.0 and scheduled for removal in v0.10.0. CLI use emits a warning and preserves v0.8.x compatibility by auto-activating locale-gated recognizers. |
| `locale-de` | [`embedded/locale-de.toml`](embedded/locale-de.toml) | DACH locale metadata such as German email headers. |
| `locale-en` | [`embedded/locale-en.toml`](embedded/locale-en.toml) | English locale metadata such as English email headers. |

The loader returns `None` for unknown names. Policy/CLI callers should treat
unknown bundled names as configuration errors.

## Recognizer-level metadata (v0.7.2)

Rulepacks loaded into this crate can carry two cross-cutting metadata blocks
that change conflict-resolution behavior without changing the underlying
backend implementation. Both are designed to fail closed: when adopters
under-specify them, Gaze emits a coarser family-level token rather than a
guess.

### Collision-family policy

Cross-class recognizer rivalries (PAN vs IBAN, postal vs phone, …) declare
collision metadata beside the recognizer definition:

```toml
[[recognizers]]
id = "iban.structural"
class = "custom:iban"

[recognizers.collision]
family = "payment-card-or-iban"
variant = "iban"
precedence = 10
```

Adopter policy uses the same shape under `[[policy.custom_recognizers]]` with
a `[policy.custom_recognizers.collision]` table. The runtime compiles these
into a `FamilyPolicyTable` queried by stable recognizer id. Validator-veto
runs first; family policy then arbitrates same-family different-variant
overlaps with `ConflictTier::CollisionPolicy`. Equal precedence between
variants emits a family-level `PiiClass::Custom("family:<name>")` token plus
`AmbiguityRecord::PrecedenceTie`. Reserved bundled family names cannot be
claimed by adopter policy. Full contract:
[`docs/architecture/collision-family.md`](../../docs/architecture/collision-family.md).

### Mandatory-anchor resolution

A collision-family recognizer can require a deterministic cue before emitting
its narrower variant:

```toml
[recognizers.collision]
family = "payment-card-or-iban"
variant = "iban"
precedence = 10
mandatory_anchor = "iban"
```

Locale rulepacks (e.g. `locale-en`, `locale-de`) supply the matching cue
bundle:

```toml
[locale.cues.iban]
names = ["IBAN", "IBAN:", "Account No."]
window_chars = 64
```

When the anchor cue is found in a bounded window around the candidate span,
the variant class flows normally. When the anchor is missing — or the locale
bundle does not provide that cue key — Gaze emits one family-level
`PiiClass::Custom("family:<name>")` token, sets the decision to
`ConflictTier::AnchoredContext`, and attaches an `AmbiguityRecord` with
`AmbiguityReason::NoAnchor`. The cleaned text receives one token and the
manifest stores one restore mapping. The bundled
`cargo run -p xtask -- locale-cue-bundle-coherence` gate fails if a bundled
recognizer declares `mandatory_anchor` without a matching bundled cue block.
Full contract:
[`docs/architecture/anchor-resolution.md`](../../docs/architecture/anchor-resolution.md).

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

The detection entry point is **fallible** (P0 #908):

```rust
fn detect(&self, input: &str, ctx: &DetectContext<'_>)
    -> Result<Vec<Candidate>, gaze_types::DetectError>;
```

A backend failure MUST surface as `DetectError::backend(self.id(), <message>)`,
never as an empty `Vec`. Returning an empty candidate list means "no PII here",
and the pipeline trusts it — so a backend that fails silently is an axis-1 leak.
The registry short-circuits on `Err` and the pipeline aborts outbound redaction
(`gaze::pipeline::Error::RecognizerDetect`) rather than emitting partially
cleaned output. Recognizers whose logic cannot fail simply return
`Ok(candidates)`. Full contract:
[`docs/architecture/p0-908-ner-failclosed.md`](../../docs/architecture/p0-908-ner-failclosed.md).

Add adopter-specific recognizers outside this crate when the behavior is tied
to one tenant, one private schema, or one proprietary data source.

The per-recognizer metadata surface (`id`, `supported_class`, `token_family`,
`validator_kind`, `locales`), the SafetyNet benchmark-snapshot fields
(strict-span leak rate, observer-residual recall, composability quad), and
the `Candidate`/`CollisionMembership` audit-row linkage are cataloged in
[`docs/metrics.md`](../../docs/metrics.md#3-safetynet-metrics-gaze-recognizers)
(SafetyNet) and
[`docs/metrics.md`](../../docs/metrics.md#4-recognizer-surface-gaze-recognizers--gaze)
(recognizer surface).

## Test support

The crate has a `test-support` feature for tests that need additional support
surface without making it part of the default public runtime.
