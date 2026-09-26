# Repeat-value sweep

Once Gaze tokenizes a value that a rule found, every other copy of that value
in the same document, and in later documents of the same session, is
tokenized too. Before the sweep a copy was protected only when a recognizer
fired at that exact spot, so a name caught in an email header shipped raw in
the body, in a different case, or in the next turn (solo todo 3849).

```text
input   From: Maria Schneider <maria.schneider@example.invalid>
        hi, this is maria schneider again. Thanks, Maria
before  From: <a13e:Name_1> <<a13e:Email_1>>
        hi, this is maria schneider again. Thanks, Maria
after   From: <a13e:Name_1> <<a13e:Email_1>>
        hi, this is <a13e:Name_2> again. Thanks, <a13e:Name_3>
```

## Where it runs

After resolve and before the safety net. The sweep collects its values from
two places: this document's resolved winners and the session manifest. It
looks for copies that no winner covers. Each copy joins the candidate pool,
and the pool is resolved again, so the resolver's usual rungs settle any
overlap. A copy that encloses a weaker same-class span, such as an NER
fragment that left `<Name_1>a`, wins and covers the whole copy. A document
with no uncovered copy keeps its first resolution; the only extra cost is one
linear scan.

The proxy and the daemon clean whole request bodies, so no copy is cut by a
chunk boundary. A future streaming ingress must hold back
(longest value − 1) bytes before it emits.

## What propagates

Only values found by deterministic evidence: a validator, an anchored or
structural cue, or a plain regex or dictionary rule. Values found by the NER
model or by a safety net never propagate. Copying model-found values spreads
the model's mistakes across whole documents: on TAB court cases, 91 % of the
new false positives came from NER-tagged words such as "Government" and
"Court". A class the policy does not protect (`preserve`) never propagates.

## Matching rules

| Value | Matches |
|---|---|
| Several words, or digits and punctuation (`Maria Schneider`, `DE44 5001`) | Any case; any whitespace run (space, tab, line break, NBSP) matches any whitespace run. At least 4 non-space characters. |
| One alphabetic word (`Schneider`, `Albrecht-Quaye`) | Title case, or as written when it starts upper-case and is not all capitals (`McDonald`). At least 4 characters. |
| A part of a multi-word `Name` value | Same as one word, at least 3 letters. |
| A collision-family value (`family:<name>`) | Byte-exact only. |

Every copy must stand on word edges under the shared
`gaze_types::is_inside_word` rule, and a copy inside a URL-shaped run
(`://` or a leading `www.`) is skipped. A single word on a closed list of
common words that are also names (months, weekdays, `Will`, `Mark`, `Rose`,
`May`, `Sonne`, ...) is never swept.

Case folding keeps a map from every folded byte back to the character it came
from, and a copy must start and end on whole characters. Turkish `İ` lowers to
two scalars; the copy still covers exactly its own bytes. Default Unicode case
mapping lowers `I` to `i`, so a dotless `ı` in a value does not match an
upper-case `I` in the copy.

**Stated leaks.** A lone lower-case part (`thanks maria`) stays raw: matching
lower-case single words would tokenize ordinary words that happen to be
names. A value no rule found (only NER, or nothing) seeds no sweep.

## Token identity and restore

A token restores to exactly one byte string, so:

- A byte-identical copy reuses the source token.
- A different spelling (`maria schneider`, `MARIA SCHNEIDER`) or a name part
  gets its own sibling token of the same class and token family.

Restore stays exact for every copy.

## Session state

Each manifest entry records the evidence tier its value was found with, and
keeps the strongest tier seen. The session caches one multi-pattern matcher
over its sweepable values and rebuilds it only when a value's sweepability
changes. The value list is the manifest the session already holds; the sweep
adds no new store of raw values.

The tier travels in the `session_blob`, which moved to envelope version 6.
Readers up to v0.15 refuse a v6 blob with `InvalidSnapshotVersion(6)` rather
than drop the field. A blob of version 5 or older still imports and restores,
but none of its values seed the sweep, because their tier is unknown.

## Audit

Every swept copy writes one audit row with `recognizer_id` and
`provenance_stage` `manifest_sweep`, `decided_by: manifest_sweep`
(`ConflictTier::ManifestSweep`), and `provenance_merged_from` naming the link:
`manifest_sweep:exact`, `manifest_sweep:variant` or `manifest_sweep:part`.
The row never carries the source token or value, per the audit contract. The
daemon overwrites `provenance_stage` with `daemon` as it does for every row;
`decided_by` still names the sweep.

## Failure

Fail closed. When the value list passes its size cap (200,000 patterns or
16 MiB of pattern bytes) or the matcher cannot be built, the request fails
with `Error::ManifestSweep`. It never ships unswept text.

## See also

- [Session contract](../core/session-contract.md)
- [How Gaze works](../how-gaze-works.md)
- Tests: `crates/gaze/tests/manifest_sweep.rs`,
  `crates/gaze-cli/tests/manifest_sweep.rs`,
  `crates/gaze-proxy/tests/manifest_sweep.rs`
