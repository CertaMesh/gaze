# Protection outcomes and field evidence

Gaze declares four typed outcomes in `gaze_types::outcome` so sensitivity and
replacement safety can be evaluated separately:

- `Protected` describes a sensitive value that can be substituted reversibly.
- `NonSensitive(reason)` requires evidence that the position is not sensitive.
- `PossiblySensitive(reason)` records uncertainty about a value or its binding.
- `ConfirmedUntransformable(reason)` identifies confirmed sensitivity without a
  safe reversible rendering. Privacy-enforcing consumers must refuse it.

These types are dormant contracts. No recognizer produces these outcomes yet,
and this addition does not change detection, rendering, audit storage, or refusal
policy. `Candidate.outcome == None` denotes the legacy protected-candidate
contract. It does not prove that an input contains no other sensitive values.

`FieldProvenance` carries a field path, its original JSON scalar type, and the
source of that context. Caller-declared context and validated parsing provide
the strongest evidence. A fence language, filename extension, content type, or
tool name is only a hint. A hint alone can never justify `Protected` or
`NonSensitive`.

`FieldContext::WholeInput` describes one scalar input. `FieldContext::Spans`
describes scalar fields by half-open UTF-8 byte ranges in the detection input.
Producers must translate ranges when normalizing that input. `DetectContext::new`
sets no field context; `with_fields` attaches it explicitly.

Reasons contain closed variants rather than input values. Field paths store
unescaped JSON-pointer segments without scalar values. Object keys can contain
PII, so path Debug and Display hide their text. Path serialization retains the
segments for trusted structural consumers and requires a separate disclosure
policy at untrusted boundaries. `ProtectionError::Untransformable` is declared
but has no producer.
