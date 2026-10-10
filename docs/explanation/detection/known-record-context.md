# Caller-known record context

Pass a trusted app's typed record through `gaze clean --context-json` to detect
known values for this call. Matches become tokens; the manifest restores their
exact source bytes. The record never enters cleaned text.

```json
{
  "record": {"customer": {"full_name": "[customer name]", "email": "[customer email]"}}
}
```

Brackets stand for private app data. Keep the context file private and out of logs.

Classes come from a versioned EN/DE/FR/NL/PT field-alias table. `field_map`
overrides classes, handles unknown keys, or marks a leaf `"ignore"`. Unmapped
unknown keys reject the call with a field path, never its value. Normal
recognizer and first-match action rules apply; only `tokenize` and
`format_preserve` are accepted. Nonreversible column actions reject record mode.
Audit source IDs contain class and slot, never values.

| Group | Default matching |
| --- | --- |
| Cards, IBANs, national IDs, Steuer IDs | Exact and flexible whitespace |
| Passports, phones | Exact |
| Full names | Exact, case-folded, combined whitespace/case |
| Single names | Case-folded or corroborated |
| Address parts, exact single names, whitespace-only full-name variants, email, unlisted pairs | Off |

`record_match_kinds` replaces a group's defaults. Values are trimmed and
whitespace runs collapsed. Flexible matching allows text whitespace runs up to
32 characters, without adding/removing separators. Enabled name folding uses
full Unicode case folds. Name order, email case changes, abbreviations,
fragments, and typos are not inferred.

[Common single-token names](../../../crates/gaze-recognizers/assets/record-common-names-v1.txt),
such as `Will`, need NER person evidence, an adjacent record name, or a full
record name elsewhere with a name-position cue. Unlisted names such as `Maren`
match changed case by default; same-case exact copies need opt-in or another
detector. Default Nym has no person label, so model corroboration uses NER.

Unsafe short values are skipped individually. Inspect Rust
`record_value_rejections` for paths and typed reasons; accepted values in disabled
groups are inert and absent from this list. A valid IBAN is accepted even when
its country code supplies its only two letters. Ordinary document detectors
still run for disabled groups.

Corroborated homonyms can still cause false positives. In the oracle, disabling
exact address parts/single names left 337/123 extra leaked gold bytes versus
all-on. Exact phone/card values stayed on despite 69/99 added layer D benign
bytes. Combined-whitespace/case and case-folded full-name estimates had only
14/24 positive inputs. Unmeasured kinds remain off.

See the [JSON shape, limits, and errors](../../reference/policy.md#caller-known-record-context).
The [oracle arm](../../reference/benchmarks/known-record-oracle.md) assumes known
gold values; it measures an upper bound, not context-free detection.
