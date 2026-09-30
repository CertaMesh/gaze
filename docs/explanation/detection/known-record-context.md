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

This mode trims record values and collapses whitespace runs. By default it
matches exact and flexible-whitespace variants of credit cards, IBANs,
national IDs and Steuer IDs; exact passports and phones; exact, case-folded
and combined whitespace/case variants of full names; and case-folded or
corroborated single names. Address parts, exact single names, flexible-
whitespace full names, email and other unlisted pairs are off. The adopter
can replace a group's defaults with `record_match_kinds` in the context JSON.
Whitespace-flexible matching collapses whitespace runs; it does not add or
remove separators, so pass the value in the form the document uses.
The recognizer can match text whitespace runs of up to 32 characters and full
Unicode name case folds when enabled. The original source bytes are kept in
the manifest for exact restore.
Single-token names in the [version 1 common-word dictionary](../../../crates/gaze-recognizers/assets/record-common-names-v1.txt),
including `Will`, require person-model evidence, an adjacent record name,
or a full record name elsewhere in the document with a name-position cue.
Unlisted names such as `Maren` match changed-case copies by default.
Same-case exact copies require opt-in or another detector. Unsafe short values
are skipped individually and reported by path and typed reason in the Rust
context. A valid IBAN passes even when its country code is its only two letters.
Apps should inspect `record_value_rejections` for refused values. The list
contains refusals only; accepted values in off-by-default groups are inert and
do not appear there. This mode does not infer name order, email case
changes, abbreviations, fragments or typos.
Homonyms with corroborating context remain a false-positive risk. The oracle
found that turning exact address parts and single names off leaves 337 and
123 additional leaked gold bytes compared with its all-on arm. Exact declared
phone and credit-card values stay on despite 69 and 99 added layer D benign
bytes. The combined-whitespace/case full-name kind had only 14 positive
inputs, and case-folded full names had 24, so their precision estimates are
thin. Unmeasured kinds stay off because their gain is unknown. A record field
whose group is off adds no record-context detection. Ordinary detectors still
run on the document.
The current default Nym operating point has no person label; model
corroboration currently uses NER person spans.
The app must keep its context file private and avoid logging it.

The [policy reference](../../reference/policy.md#caller-known-record-context)
defines the JSON shape, limits and error behavior. The separate
[known-record oracle arm](../../reference/benchmarks/known-record-oracle.md)
assumes an adopter already knows gold values; its result is an upper bound,
not context-free detection performance.
