# Labelled CRM/HR value boundaries

Generator v19 extends the [field-ownership cells](record-id-value-ownership.md)
with independently authored customer, employee and person/contact/member
identifier boundaries. All values are synthetic. Both partitions have distinct
seeds, keys and values; gold comes from the producer's field values rather than
detector output.

| Boundary | Positive and repeat coverage | Public counterweights |
| --- | --- | --- |
| Prose punctuation | A final sentence period or colon is outside the value; internal dots/colons remain inside it; quoted values retain final punctuation | Identical values behind inventory or artifact labels |
| Adjacent cues | Separate same-class and different-class values after colon, equals, is, ist, lautet and whitespace connectors | Public version fields after the same connectors; labels and public values are non-gold |
| Serialized JSON | Direct JSON and 1, 2, 3, 8 and 9 payload-string wrappers; literal and escaped Unicode/combining marks, quotes, backslashes and tabs | Identical scalars behind public keys plus a public sibling column |
| Opening line breaks | LF and CRLF immediately after an opening ASCII quote, with closed and malformed scalar tails | Public-key tails and explicitly recorded public suffix values |
| Tab groups | Literal Unicode and combining-mark identifier groups; personal fields followed by public columns; ambiguous untyped public columns | Public TSV rows, schema/header rows and alphabetic placeholders |

Each shape appears in A, R and matched D cells for all three families and both
partitions. A and R also record nearby public fields as decoys, so swallowing
them adds false-positive bytes. Tests check exact UTF-8 gold boundaries,
serialization round trips, presence of each connector/depth/line-break variant,
disjoint partitions and canonical scorer sensitivity to each failure class.

[Committed pre-repair probes](record-id-boundary-pre-fix-sensitivity.json) use
the exact rules-only binary and policy from the earlier bound #773 measurement.
The reproducible `check_record_id_boundaries.py` runner checks their receipt
hashes and runtime-tree equivalence before running 54 canonical model-free
probes. All five failures are visible in every family; these probes establish
coverage sensitivity and do not claim benchmark gain.

Unquoted sentence punctuation is not part of a repeat seed. Dotted examples
include `AB.12.34.` followed by `AB.12.34`: the first occurrence's sentence
period is excluded, while both internal periods remain gold. This mixed
alphanumeric value exercises the existing sweep floor without changing it.
Longer generated dotted values use independently seeded digit groups.

Serialized gold is the full raw encoded scalar content on its surface, with
surrounding delimiter syntax excluded. R repeats that exact source spelling.
The JSON is parsed through every wrapper in the coverage tests; no detector's
seed or escape parser determines the gold span.

Conservative protection has a measurable cost. A malformed quoted tail still
contains a producer-declared public suffix, and a depth-9 payload still has
public sibling fields and closing syntax. They remain non-gold even if a
fail-closed scanner protects the remainder. An ambiguous untyped TSV column
also remains public according to the producer, even when conservative grouping
protects it. Report these false-positive byte classes explicitly alongside
leaks, refusals, exact restores and valid manifests; a net total cannot explain
the cost on its own.

Known limitations: this extension does not infer person linkage from CSV/TSV
headers, add typographic value quotes, lower the short digit-run repeat floor,
or promise a public boundary for ambiguous untyped columns. The earlier
generators and contracts remain reconstructible by their committed pins.
The four displayed releases are freshly measured with the committed v19 harness
and their own tagged code and setup policies. The [measurement ledger](agentic-adjacency-v19-history.json)
and [canonical replay](record-id-boundaries-v19-contract-replay.json) retain
leaks, public-byte costs, refusals, exact restores and manifest validity.

## Historical public-byte cost controls

These are false-positive bytes on the three cost-control groups in released
recognizers, shown as A / D / R. They establish the baseline for the later
conservative boundary repair; its added public-byte protection must be measured
on these same cells.

| Release and arm | Depth above eight wrappers FP B | Malformed quoted tail FP B | Ambiguous tab column FP B |
| --- | ---: | ---: | ---: |
| `v0.16.0` `policy-file` | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| `v0.15.1` `policy-file` | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| `v0.15.0` `policy-file` | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| `v0.14.0` `full-stack-kiji-resolve` | 0 / 0 / 0 | 140 / 0 / 170 | 67 / 0 / 65 |
| `v0.14.0` `pass2-ner` | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
