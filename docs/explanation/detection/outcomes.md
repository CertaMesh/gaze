# Protection outcomes and field evidence

Gaze declares four typed outcomes in `gaze_types::outcome` so sensitivity and
replacement safety can be evaluated separately:

- `Protected` describes a sensitive value that can be substituted reversibly.
- `NonSensitive(reason)` requires evidence that the position is not sensitive.
- `PossiblySensitive(reason)` records uncertainty about a value or its binding.
- `ConfirmedUntransformable(reason)` identifies confirmed sensitivity without a
  safe reversible rendering. Privacy-enforcing consumers must refuse it.

The bundled `field_labelled` recognizers (`customer_id.field`,
`employee_id.field`, `record_id.field`) produce them; see
[Field-labelled record IDs](#field-labelled-record-ids). Caller record
admission separately refuses unsafe short values at enforcing boundaries with
the closed `caller_value_too_short` reason. Owner-side eval instead warns and
measures. `Candidate.outcome == None` denotes the legacy protected-candidate
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
`field_labelled` recognizers read this context; no other recognizer does.

Reasons contain closed variants rather than input values. Field paths store
unescaped JSON-pointer segments without scalar values. Object keys can contain
PII, so path Debug and Display hide their text. Path serialization retains the
segments for trusted structural consumers and requires a separate disclosure
policy at untrusted boundaries. `ProtectionError::Untransformable` names the
first refused field and lists every further one in `others`, in document order.

## Field-labelled record IDs

A `field_labelled` recognizer binds a scalar by its structured label. It emits
on whole parsed leaves only; a fragment of a leaf never establishes the value.

| Leaf | Evidence | Outcome |
|---|---|---|
| string, digit-bearing, at most 40 bytes and four groups | validated parse or caller | `Protected`, whole leaf |
| any other string | validated parse or caller | `PossiblySensitive(label_value_shape_unclear)` |
| any | hint only | `PossiblySensitive(label_binding_unproven)` |
| JSON number | validated parse or caller | `ConfirmedUntransformable(json_number_no_text_only_path)` |
| boolean or null | any | none |

The pipeline enforces outcomes on the raw candidate pool, after validator veto
and before conflict resolution, token allocation or output, so no overlapping
candidate can discard a refusal and nothing is staged first. A confirmed
untransformable candidate refuses the input. `[outcomes] possibly_sensitive`
protects (default), refuses with `no_safe_rendering`, or passes with an audit
row; a possibly sensitive JSON number refuses under `protect`. A class the
policy preserves has nothing to refuse. Possible and confirmed protection
stay distinct in the audit row and the final protection trace.

A field-bound value enters the session with rule evidence, so the repeat-value
sweep copies it elsewhere in the document and in later turns. A copy may not
become a token inside a JSON number or object key: an exact whole-literal
repeat there refuses, typed (`json_number_no_text_only_path` for a number,
`no_safe_rendering` for a key), before any token is minted. A copy that only
overlaps such a literal is a different value and is dropped rather than
splitting the literal. The sweep's length floors still apply, so short values
never trigger it.

Boundaries apply the same contract. Anthropic `tool_use.input` numbers pass
typed scalar admission: each number is scanned as a whole leaf with its path,
so an unlabelled number nothing detects is forwarded unchanged and a labelled
one refuses. Gemini and OpenAI legacy routes scan every forwarded number the
same way. MCP keeps refusing undeclared numbers with `UndeclaredNumber`. JSON
encoded inside a string (an OpenAI `arguments` string, a tool result string)
carries no field context, so these rules do not bind there.

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
The pipeline writes them on every field-bound winner, refusal and
`pass_audited` row.

The restricted audit projection includes the four value-free columns.
`gaze audit query` and `gaze audit export` accept `--outcome` and
`--outcome-reason`. Filters use canonical snake_case labels and also accept
kebab-case spellings. Filters combine with AND and omit rows with NULL labels.
