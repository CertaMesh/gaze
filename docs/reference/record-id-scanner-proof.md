# Record ID scanner proof

JSON label keys and array label cells own scalar values in independently
validated JSON regions. Complete records remain detectable after truncated
containers or an unclosed quote on an earlier log line. Trailing label-key
whitespace can use literal spaces, JSON escapes, or Unicode whitespace escapes.

An object or array under a label key owns its first ID-like child. A child is
ID-like when its key is `id`, `value`, `number`, `no`, or `nr`, or its string
value contains a digit. Nested array numbers and ordinary schema words do not
become container-owned identifiers. Object keys never become private values.

The structure generator checks 34,560 cases across label keys, scalar and
container values, wrappers, broken prefixes, suffixes, and serialization depths
0–4. Each case checks covered identifier bytes and parses the replacement with
a quoted placeholder in number positions. Parsed results retain the same object
keys, containers, array lengths, and JSON keywords. The existing 4,900-case leak
generator remains a separate check. Unicode label tails add 35 cases.

Repeat patterns require a digit in the labelled value. A repeat cannot replace
an object key, JSON syntax, or an unquoted scalar. These restrictions also apply
inside serialized JSON strings and across session turns. The 40-byte pattern
limit applies after trimming each derived pattern. Space, NBSP, and escaped-tab
padding therefore preserve repeat protection without creating oversized matcher
patterns. Pipeline tests cover 45 repeat/schema cases and 20 padded-value cases;
a daemon test checks three padded values on a second turn. Restore remains exact.

Direct numeric JSON IDs retain the existing plain-text token format, which may
make cleaned JSON invalid before restore. Quoted numeric emission and labels
with escaped letters remain follow-ups. The structure invariant tests replace
numeric tokens with a quoted placeholder; they do not claim quoted emission.

## Differential counts

The frozen scanner comparison checks 29,390 captures across generated
serializations, existing fixtures, mixed prefixes, and sibling/scaling inputs.
All retain identical results.

The seeded random comparison checks 359,152 captures. It compares fresh and
shared indexes and asserts every class count:

| Class | Captures |
| --- | ---: |
| Frozen scanner panics, repaired | 5,134 |
| Frozen reversed/rejected spans, repaired | 3,260 |
| Recovered empty JSON values | 8,344 |
| Recovered ambiguous values kept owned | 50 |
| Real successor values among those 8,344 empty cases | 0 |
| Previously valid rejections now owning the next value | 2,696 |
| Previously valid spans corrected around JSON syntax | 380 |
| Other valid frozen results, unchanged | 347,682 |

The 8,344 cases are a subset of the panic and reversed-span repairs, not an
additional class. The new JSON ownership changes affect 3,076 captures outside
that subset. The independent oracle uses Serde token parsing and container
context, without the production frame index. Every relocation preserves all
previously protected value bytes; removed bytes are wrappers or JSON syntax.

Set `GAZE_SCALAR_DIFFERENTIAL_REPORT=1` to print one `SPAN_CHANGE` JSON record
for every difference from the frozen scanner, including the earlier repairs.
The generated inputs and seed live in the committed tests.

## Quote work and mutations

Doubled-apostrophe lines reuse the prefix cursor and one quote-event index
per delimiter grammar. Quote events retain run parity, escape depth,
next-character checks, and clipped limits. A separate test compares shared
quote queries with the frozen grammar. Scaling tests use 128, 512, and 2,048
lines at four serialization depths. Each fourfold input increase must stay
within fivefold scanner work and 60 steps per source byte. Encoded ambiguous
strings can own the entire scalar, so their recognizer scan count is one.

The named `record-id-focused` Anvil check runs recognizer and core unit tests,
the CLI record-ID tests, and the daemon padding regression with two build jobs
and nice 10. Workspace checks retain the shared build lock.

The `record-id-mutations` check runs
`scripts/verify_record_id_scanner_mutations.py` from a clean checkout with a
signed HEAD. Each mutant must compile and fail its named regression test. The
script restores each source byte-for-byte and writes logs and hash receipts
under `target/scanner-mutations/`. It never creates commits or uses a stash.

The committed [mutation receipt](record-id-scanner-mutations.json) pins the
implementation commit and records the compiled mutants, named failures, and
matching source hashes before and after restoration. The probes cover quote
cache reuse, JSON region recovery, object-key and container ownership, escaped
whitespace, repeat eligibility, JSON sweep boundaries, derived pattern bounds,
padded values, and session provenance.

These tests do not replace full-corpus benchmark certification. Detection
changes still require both scored-label contracts and mechanism evidence for
the exact integrated candidate before merge.
