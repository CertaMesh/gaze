# Caller-known record context

An app often knows which customer or order an agent is working on before it
sends text to the agent. The app can pass a small, explicitly typed record to
`gaze clean --context-json`. Gaze uses those values as call-scoped detection
evidence, replaces matches with tokens, and restores the exact matched bytes
from the session manifest. The record is not included in the cleaned text.

```json
{
  "record": {"customer": {"name": "[customer name]", "email": "[customer email]"}},
  "field_map": {"/customer/name": "Name", "/customer/email": "Email"}
}
```

The bracketed values stand for data supplied by the trusted app, not literal
values to send to an agent.

The caller, rather than Gaze, supplies the class for every string leaf.
Unmapped, unknown and ambiguous values reject the call. A record dictionary
still runs through the usual recognizer and first-match class action rules.
Only reversible `tokenize` and `format_preserve` actions are accepted. A
nonreversible column action rejects record mode because it could leave a
matching value raw in that column. Audit source IDs describe a class and slot,
never the supplied value.

This mode matches a full value. Names also match Unicode case changes; the
original casing is kept in the manifest for exact restore. It does not infer
name order, email case changes, abbreviations, fragments or typos. A short
value or a common single-word name can collide with unrelated prose, so the
parser refuses those values. Longer homonyms remain a false-positive risk.
The app must keep its context file private and avoid logging it.

The [policy reference](../../reference/policy.md#caller-known-record-context-prototype)
defines the JSON shape, limits and error behavior. The separate
[known-record oracle arm](../../reference/benchmarks/known-record-oracle.md)
assumes an adopter already knows gold values; its result is an upper bound,
not context-free detection performance.
