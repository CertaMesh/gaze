# gaze-recognizers

Built-in recognizer backends and bundled rulepacks for Gaze.

This crate depends on `gaze` and implements concrete `gaze::Recognizer`
backends. Keeping it separate lets the core crate expose a small, stable
contract without forcing every adopter to compile regex, dictionary, ONNX, and
tokenizer dependencies.

## Cargo

```toml
[dependencies]
gaze = "0.4.1"
gaze-recognizers = "0.4.1"
```

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
- `NormalizerKind::EmailCanonical`

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

The expected production model family is Davlan mBERT NER, configured through
policy `[ner]` and loaded by `gaze-assembly` when `model_dir` is present.
Loading failures are policy configuration failures in the CLI path.

## Embedded rulepacks

`embedded(name)` returns bundled TOML contents for known rulepack names:

| Name | File | Purpose |
|------|------|---------|
| `core` | [`embedded/core.toml`](embedded/core.toml) | Global core recognizers, including email address and email-header name detection. |
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
