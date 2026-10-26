# Record ID scanner proof

JSON label keys and array label cells own scalar values in independently
validated JSON regions. Complete records remain detectable after truncated
containers, including a stray quote on the same line. Trailing label-key
whitespace accepts literal and escaped Unicode whitespace and zero-width space.

A labelled array owns every direct string element and each object element's
explicit ID children. Objects own only `id`, `value`, `number`, `no`, or `nr`
children, matched without case. Numbers require a direct label or an explicit
ID child key. A container with JSON-Schema keywords owns no values. These
restrictions intentionally leave arbitrary child fields and schema examples
unprotected rather than rewriting metadata or JSON syntax.

The structure generator checks 34,560 cases across label keys, scalar and
container values, wrappers, broken prefixes, suffixes, and serialization depths
0–4. Its projection distinguishes strings from numbers and retains object keys,
array lengths, literals, and container types. Direct numeric ID spans use a
numeric placeholder in this recognizer-only test because numeric token quoting
remains a separate limitation. The existing 4,900-case covered-value generator
remains a separate check. Unicode label tails add 85 cases.

A separate pipeline invariant checks 600 mixed-carrier cases with prose,
key-value and YAML seeds followed by JSON, same-line stray quotes, escaped JSON
in prose, and quoted payloads. It checks both one-document and second-turn
sessions. Cleaned layers must parse with unchanged keys and value types, and
private string copies must disappear. It also requires exact restore. Another
130 cases exclude every supported JSON-Schema keyword from container ownership;
40 cases verify all and only explicit container values across five depths.

Every sweep source follows the JSON structural gate, including sources labelled
in prose and prior turns. A repeat cannot replace an object key, JSON syntax,
an unquoted scalar, or part of a JSON escape. The gate indexes decoded JSON
fragments even without an enclosing JSON string. Field-owned repeats require a
digit. Their 40-byte bound applies after decoding and trimming. Eight pipeline
cases and four daemon cases check escaped seeds against plain copies and the
reverse direction, together with second-turn JSON structure and exact restore.

Fifteen cases check escape integrity, 45 check pipeline repeats and schema,
and 28 check padding. Three daemon padding cases protect second-turn copies.
Nine serialization-depth cases and one Unicode fold-expansion case check short
effective patterns despite longer source spellings. Forty more cases check
deeply escaped padding and imported session state. Derived exact patterns match
the decoded body and project each hit to verified source bytes. Duplicate hits
retain one copy. An escape cascade beyond 32 decode passes fails closed before
matcher creation; ordinary serialized inputs shrink at each layer.

The field-scalar cap preserves legacy `UncertainPrefix` eligibility and its
precision floor. Long legacy patterns use a contiguous NFA. The recognizer's
40-byte diagnostic retains primary ownership. Collision-family matching remains
case-sensitive. Session storage retains provenance. Restore remains exact.

Direct numeric JSON IDs retain the existing plain-text token format, which may
make cleaned JSON invalid before restore. Numeric token quoting, escaped-letter
labels, plural ID keys, pair-key records, and header-column mapping remain
follow-ups. The strict mixed-carrier invariant covers numeric counterweights;
it does not claim that direct numeric ID emission preserves JSON validity.

## Lexical value and carrier checks

Field ownership also requires an ID-shaped value. The decoded body contains a
digit and starts with an alphanumeric character, optionally preceded by one
`-`, `@`, or `_`. A separate base64 check retains existing digit-free values
such as `+YQ==` and `opaque+YQ==`. It validates the run, terminal padding, and
following delimiter. Digit-free base64 needs an explicit label separator or a
quoted scalar; a closing quote is accepted only for a quoted value or proven
parent scalar. Opening attribute quotes cannot terminate a preceding word.
Quoted alphabetic placeholders remain public.

Unquoted groups stop at carrier operators and record boundaries. Grouping
retains up to four single-space-separated groups within the existing diagnostic
limit. A space-only label needs a digit in its first group before grouping.
Overlong single runs retain full ownership and limit diagnostics. Dotted member
paths and template placeholders are rejected.

The existing closed wrapper vocabulary remains supported. Outside proven data
scalars, a paired wrapper contains alphanumeric-led content with both letters
and digits, without operator syntax. Numeric-only wrapped arguments stay public.
Validated JSON, CSV, and SQL scalar contents retain their established wrapper
and multiline behavior. Unterminated prose values stop at unescaped record-line
boundaries. A shared markup index distinguishes attribute syntax from data
in both primary recognition and repeat matching. Proven scalar ranges retain
original-byte mappings across JSON decoding.

Before the lexical fixes, the carrier generator failed by owning `/td` after
an HTML label. The JSON-like generator independently failed by replacing an
object key in JSONC. Both tests now assert the resulting observable structure.

The committed carrier matrix checks 328 label/carrier combinations and 192
placeholder cases against manifest raw values and exact carrier structure.
Another 32 cases preserve JSON-like keys and bare numbers across same-document
and later-session repeats, including JSONC, trailing commas, NaN, Python repr,
JavaScript objects, leading-zero numbers, and streamed containers. Forty-eight
stray-quote checks and one Unicode-space key check exercise the lexical gate
without Serde container acceptance. Thirty-six unterminated quote cases retain
next-line code, SQL, and HTML syntax. Separate checks retain whole private values
containing key-looking text, leading symbols, grouped IDs, wrapper dialects,
and padded base64 values with operator counterweights.
Eighteen positive cases retain quoted HTML attributes and embedded JSON, CSV,
and SQL data. Eighteen same-document and later-turn repeat cases protect bare
attribute syntax while retaining quoted data, including escaped JSON fragments.
Comparison operators before a label do not create markup context.
Direct unquoted attributes remain raw pending safe token emission.

The strict JSON gate continues to reject any replacement crossing real JSON
syntax or escape interiors. The additional lexical gate rejects hits contained
in quoted keys, bare number lexemes in container text, and JSON literals.
Containment allows a whole private value to include key-looking data in prose.
Both gates index decoded spellings and project checks to original source bytes.

## Minified record lookup work

URL-shaped whitespace runs are indexed once per input. Each hit queries sorted
ranges instead of rescanning the surrounding run. The work-counter test executes
all lookups on 500, 1,000, 2,000, and 4,000 minified records and requires exactly
one input-length scan. It also retains URL exclusion behavior. No timing assert
is used.

`scripts/verify_record_id_minified_scaling.py target/debug/gaze` reports diagnostic
debug CLI timings. It checks every record's keys, numeric fields, tokenized ID,
repeat copies, entry count, and exact restore. These timings do not certify the
full detection benchmark.

Diagnostic unoptimized CLI observations at `fe4af649`, one sample per case,
with concurrent replay work on the shared machine. Each clean invocation
includes process startup. Restore runs outside the timed interval and is exact.

| Records | No copy, seconds | One copy, seconds |
| --- | ---: | ---: |
| 500 | 0.256 | 0.303 |
| 1,000 | 0.313 | 0.363 |
| 2,000 | 0.412 | 0.506 |
| 4,000 | 0.635 | 0.774 |

Every row checks the expected entry count, keys, number types, and all ID copies.
These single observations do not establish a base/candidate speedup. The work
counter independently proves that the URL-run index is built once per input.

## Differential counts

The frozen scanner comparison checks 29,390 captures across generated
serializations, existing fixtures, mixed prefixes, and sibling/scaling inputs.
The comparison freezes the original capture grammar and checks scanner boundaries
before ID-shape acceptance. All retain identical boundary results.

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
| Previously valid spans corrected around JSON syntax | 395 |
| Other valid frozen results, unchanged | 347,667 |

The 8,344 cases are a subset of the panic and reversed-span repairs, not an
additional class. The new JSON ownership changes affect 3,091 captures outside
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
the CLI record-ID tests, and both daemon record-ID regressions with two build jobs
and nice 10. Workspace checks retain the shared build lock.

The `record-id-mutations` check runs
`scripts/verify_record_id_scanner_mutations.py` from a clean checkout with a
signed HEAD. Each named baseline must execute exactly one passing test before
mutation. The catalog contains 50 compiled mutants that fail their named regression tests. The
script restores each source byte-for-byte and writes logs and hash receipts
under `target/scanner-mutations/`. It never creates commits or uses a stash.

The committed [mutation receipt](record-id-scanner-mutations.json) pins the
implementation commit and records the compiled mutants, named failures, and
matching source hashes before and after restoration, plus baseline and mutant
log hashes. The probes cover quote
cache reuse, JSON region recovery, object-key and container ownership, escaped
whitespace, repeat eligibility, JSON sweep boundaries, derived pattern bounds,
padded values, session provenance, lexical carrier gates, ID-shape exceptions,
HTML attribute syntax, and URL-index work. Independent ownership assertions
exercise header and escaped-prose defenses before the value-shape floor.

These tests do not replace full-corpus benchmark certification. Detection
changes still require both scored-label contracts and mechanism evidence for
the exact integrated candidate before merge.

## Current validation boundary

The CI continuation fixes quote-containing wrapper values and the `ID number`
label suffix while retaining the ID-shape and carrier gates. Its focused
regression checks the exact private bytes and strict restore for customer,
employee, and member labels. The mutation runner accepts both unit and integration
test execution receipts.

Fourteen probes for the changed scanner and rulepack files were rerun: ten
existing probes and four new probes for wrapper quotes and each label suffix.
Their one-test baselines passed, all compiled mutants failed the named test, and
source hashes matched after restoration. The receipt pins each observation's
implementation commit; the other 36 probes retain their preceding evidence.

The original replay has 35,214 JSON cases and 176 carrier cases. Existing deferred
leaks and 375 direct-numeric JSON validity failures remain explicit limits.
Exact-head replay, Anvil gates, and CI evidence are reported in the pull request.
These checks do not certify the full corpus. Benchmark certification and merge
remain separate requirements.
