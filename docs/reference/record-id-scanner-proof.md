# Record ID scanner proof

A JSON object key such as `"Customer ID:"` owns its following string or
number. A label in an array owns the next string or number cell. The scanner
preserves the JSON delimiters around those values, including through nested
JSON serialization. An ordinary object value string does not own a sibling
field. JSON `null`, `true`, and `false` retain their existing rejection rules.

The fixtures in
[`record_id_json_label_cells.json`](../../crates/gaze-recognizers/tests/fixtures/record_id_json_label_cells.json)
cover keys, array cells, nested rows, numbers, and escaped Unicode content.
Unit tests check exact source spans at serialization depths 0–3. CLI tests
use default `gaze clean` at depths 0–2 and restore the original input exactly.
String replacements preserve valid JSON. Bare numeric IDs receive the existing
plain-text token format, just like ordinary numeric fields: their clean output
may not be valid JSON. Exact restore still recovers the original numeric value. The four earlier panic fixtures remain unchanged.

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
| Previously valid spans corrected around JSON syntax | 219 |
| Other valid frozen results, unchanged | 347,843 |

The 8,344 cases are a subset of the panic and reversed-span repairs, not an
additional class. The new JSON ownership changes affect 2,915 captures outside
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

From a prepared Anvil checkout, run the focused proof:

```sh
anvil run <worktree> -- nice -n 10 env CARGO_BUILD_JOBS=2 \
  cargo test -p gaze-recognizers --all-features --lib regex::scalar_index -- --nocapture
anvil run <worktree> -- nice -n 10 env CARGO_BUILD_JOBS=2 \
  cargo test -p gaze-cli --all-features --test record_ids
```

The mutation script requires a clean checkout with a signed HEAD:

```sh
anvil run <worktree> -- nice -n 10 env CARGO_BUILD_JOBS=2 \
  python3 scripts/verify_record_id_scanner_mutations.py
```

The script removes JSON label ownership, then rebuilds the quote-event index
for each query. Each mutant must compile and fail exactly its named regression
test. The script restores each source byte-for-byte, checks the clean checkout,
and writes logs and hash receipts under `target/scanner-mutations/`. It never
creates commits or uses a stash.

The committed [mutation receipt](record-id-scanner-mutations.json) records both
compiled mutants, their named test failures, and matching source hashes before
and after restoration. The receipt pins the implementation commit; subsequent
proof-only commits do not change those source hashes.

These tests do not replace full-corpus benchmark certification. Detection
changes still require both scored-label contracts and mechanism evidence for
the exact integrated candidate before merge.
