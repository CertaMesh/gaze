# Caller-known record context

An app often knows which customer or order an agent is working on before it
sends text to the agent. The app can pass a small, explicitly typed record to
`gaze clean --context-json`. Gaze uses those values as call-scoped detection
evidence, replaces matches with tokens, and restores the exact matched bytes
from the session manifest. The record is not included in the cleaned text.

```json
{
  "record": {"customer": {"full_name": "[customer name]", "email": "[customer email]"}}
}
```

The bracketed values stand for data supplied by the trusted app, not literal
values to send to an agent.

Gaze infers classes from a versioned EN/DE/FR/NL/PT field-name alias table.
The caller uses `field_map` to override a class, classify an unknown key, or
mark a leaf `"ignore"`. Unknown unmapped keys reject the call, with an error
that names the field path and never the value. A record dictionary
still runs through the usual recognizer and first-match class action rules.
Only reversible `tokenize` and `format_preserve` actions are accepted. A
nonreversible column action rejects record mode because it could leave a
matching value raw in that column. Audit source IDs describe a class and slot,
never the supplied value.

This mode trims record values and collapses whitespace runs, then matches
full values across text whitespace runs of up to 32 characters. Names also match full Unicode case
folds; the original source bytes are kept in the manifest for exact restore.
Single-token names in the [version 1 common-word dictionary](../../../crates/gaze-recognizers/assets/record-common-names-v1.txt),
including `Will`, require person-model evidence, an adjacent record name, or
a full record name elsewhere in the document with a name-position cue.
Unlisted names such as `Maren` match everywhere. Short letter and digit values still fail closed. It does
not infer name order, email case changes, abbreviations, fragments or typos.
Homonyms with corroborating context remain a false-positive risk.
The current default Nym operating point has no person label; model
corroboration currently uses NER person spans.
The app must keep its context file private and avoid logging it.

The [policy reference](../../reference/policy.md#caller-known-record-context-prototype)
defines the JSON shape, limits and error behavior. The separate
[known-record oracle arm](../../reference/benchmarks/known-record-oracle.md)
assumes an adopter already knows gold values; its result is an upper bound,
not context-free detection performance.
