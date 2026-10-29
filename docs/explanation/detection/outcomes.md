# Protection outcomes and field evidence

Gaze declares four typed outcomes in `gaze_types::outcome` so sensitivity and
replacement safety can be evaluated separately:

- `Protected` describes a sensitive value that can be substituted reversibly.
- `NonSensitive(reason)` requires evidence that the position is not sensitive.
- `PossiblySensitive(reason)` records uncertainty about a value or its binding.
- `ConfirmedUntransformable(reason)` identifies confirmed sensitivity without a
  safe reversible rendering. Privacy-enforcing consumers must refuse it.

These types are dormant contracts. No recognizer produces these outcomes yet,
and these types do not change detection, rendering, or refusal policy.
`Candidate.outcome == None` denotes the legacy protected-candidate contract.
It does not prove that an input contains no other sensitive values.

`FieldProvenance` carries a field path, its original JSON scalar type, and the
source of that context. Caller-declared context and validated parsing provide
the strongest evidence. A fence language, filename extension, content type, or
tool name is only a hint. A hint alone can never justify `Protected` or
`NonSensitive`.

`FieldContext::WholeInput` describes one scalar input. `FieldContext::Spans`
describes scalar fields by half-open UTF-8 byte ranges in the detection input.
Producers must translate ranges when normalizing that input. `DetectContext::new`
sets no field context; `with_fields` attaches it explicitly.

### Whole-document field context

Every text clean (`gaze clean`, the daemon, library `RawDocument::Text`) passes
`FieldContext::Spans` to recognizers when it can locate fields. It has two
producers:

- `ValidatedParse`: the document strictly parses as one JSON object or array.
  Every scalar leaf becomes a span. A string's span covers its literal interior,
  still in escaped source spelling. Numbers, booleans and nulls cover their
  literal.
- `CallerDeclared`: string and number leaves of the context JSON `fields` map
  are located wherever their exact spelling occurs without cutting a word.
  Object keys and array indices form the path.

The parser is `gaze_types::json_tree`, the strict parser the Anthropic codec
uses. It refuses duplicate keys, lone surrogates, trailing bytes and limit
overruns. Unlike the codec, `gaze clean` accepts non-ASCII keys. A document
that does not parse strictly gets no parse context and keeps legacy detection.
It is never marked `NonSensitive`. Spans are projected onto the normalized
detection input; a range that would split a normalized character is dropped.
No recognizer reads field context yet, so output is unchanged.

Reasons contain closed variants rather than input values. Field paths store
unescaped JSON-pointer segments without scalar values. Object keys can contain
PII, so path Debug and Display hide their text. Path serialization retains the
segments for trusted structural consumers and requires a separate disclosure
policy at untrusted boundaries. `ProtectionError::Untransformable` is declared
but has no producer.

## Audit metadata

`RedactionEntry::with_protection_outcome` sets two closed labels together:
`outcome` identifies the category and `outcome_reason` identifies the reason.
`protected` and `possibly_sensitive` remain separate even when both use a
protection action. `protection_outcome()` rebuilds the typed decision and
rejects missing or mismatched reason families. Typed SQLite reads enforce the
same pairing. Family ambiguity stays in `AmbiguityRecord`.

`with_field_provenance` copies only `field_path_depth` and `field_evidence`.
JSON object keys can contain PII, so audit entries never copy the path segments.
The depth column counts unescaped JSON-pointer segments, with zero for the root.
It leaves room for a separate path identifier under a future disclosure policy.
Evidence contains only closed variants, including the kind of an unvalidated hint.

`SqliteLogger` adds these four nullable columns to existing databases. Existing
rows retain NULL metadata, and read-only queries support databases that have not
been migrated. A NULL outcome is legacy metadata, not a confirmed decision.
New fields are omitted from serialized `RedactionEntry` when absent.
No pipeline producer populates them yet.

The restricted audit projection includes the four value-free columns.
`gaze audit query` and `gaze audit export` accept `--outcome` and
`--outcome-reason`. Filters use canonical snake_case labels and also accept
kebab-case spellings. Filters combine with AND and omit rows with NULL labels.
