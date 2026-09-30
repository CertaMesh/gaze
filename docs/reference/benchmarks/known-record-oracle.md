# Known-record oracle arm

This is a **separate upper-bound experiment**, not the Gaze benchmark headline.
The harness gives Gaze each document's selected gold values before detection,
as if an adopter already knew the customer or order record. Real apps may know
less. The ordinary no-record benchmark continues to use its existing request
shape and is measured separately under both scored-label contracts.

## v5 attribution, before the shipping-default gate

The full v5 producer and layer K measured each class and match kind at source
`52049550`, with the oracle kinds deliberately enabled. Both scored-label
contracts used policy SHA-256 `f909a23a`, corpus SHA-256 `916c6379`, and the
same machine. These are **all-on attribution results**, not the final product
defaults. The committed aggregate scorecards are
[`v2`](known-record-attribution-v5-v2.json),
[`v1`](known-record-attribution-v5-v1.json), and the
[class × kind gain table](known-record-attribution-v5-gain.json). No raw record
values or document text are stored in these files.

| Layer | v2 leaked bytes, no record → all-on | v1 leaked bytes, no record → all-on | False-positive byte change |
| --- | ---: | ---: | ---: |
| C | 7,475 → 4,297 | 13,423 → 10,251 | +142 |
| A | 4,119 → 1,488 | 4,119 → 1,488 | −96 |
| D | 0 → 0 | 0 → 0 | +264 |
| R | 234 → 6 | 234 → 6 | +98 |
| K | 484 → 0 | 484 → 0 | +23 |

Total leaked bytes fall by 6,521 under v2 and 6,515 under v1. Total false
positives rise by 431 bytes. Eligible leaked bytes fall from 6,117 to 86.
No document is refused; all 6,854 documents restore exactly and have valid
manifests in each arm and contract. The name-multi rows had 1,399 exact,
24 case-folded, 14 combined whitespace/case, and five flexible-whitespace
positive inputs. Exact, case-folded and combined whitespace/case full names
show positive net gains under both contracts; flexible whitespace recovers no
gold bytes. The 14 and 24 positive counts limit confidence in the two variant
estimates.

The defaults turn exact address parts and exact single names off because their
layer D benign counterweights rise by 29 and 45 bytes. The all-on arm recovered
337 and 123 gold bytes from those kinds, respectively, so turning them off
leaves those leaks for ordinary detectors or an explicit adopter opt-in.
Exact declared phone and credit-card values remain on by user decision, despite
69 and 99 added layer D benign bytes. This exception applies only to the
caller-known-record oracle; ordinary recognizer rules keep the usual D veto.
The final gate reruns both contracts with the **product's default kinds** and
compares each layer's leaks, false positives, refusals, exact restores and
manifest validity. Its aggregate results will be linked here before merge.

## Earlier runs

**Historical diagnostic only:** the runs below used a benchmark producer that
registered placeholder record slots once. It did not apply each request's
`record_match_kinds`, and it gave multi-token names single-name defaults.
These numbers do not measure the shipped product configuration and cannot
support a merge decision. A fresh current-main v5 run with layer K will replace
them. Gaze normalizes NBSP and NNBSP to ordinary spaces before detection, so
those characters in text are exact matches after normalization, not evidence
for `whitespace_flexible`.

The preregistered prediction was that record context would protect 90–100% of
baseline leaked bytes within eligible exact-value gold spans. Round 2 at
measured source commit `7d72b6d1` protects **99.16%** overall: 6,513 eligible
leaked bytes fall to 55 under both contracts. C protects 3,599 of 3,648
(98.66%), A protects all 2,631, and R protects 228 of 234 (97.44%). D has no
eligible gold and includes 210 explicit benign record counterweights.

| Layer | Eligible leaked bytes, base → record | Headline leaked bytes v2, base → record | Headline leaked bytes v1, base → record | False-positive bytes, base → record |
| --- | ---: | ---: | ---: | ---: |
| C, primary corpus | 3,648 → 49 | 8,602 → 4,987 | 14,789 → 11,185 | 27,316 → 27,425 |
| A, generated identifiers | 2,631 → 0 | 4,119 → 1,488 | 4,119 → 1,488 | 881 → 785 |
| D, benign lookalikes | none | 0 → 0 | 0 → 0 | 5,201 → 5,510 |
| R, repeated values | 234 → 6 | 234 → 6 | 234 → 6 | 419 → 433 |

No document was refused. All 6,370 documents restored exactly and had valid
manifests in each arm and contract. Total false-positive bytes rise by 336:
C +109, A -96, D +309, R +14. Total headline leaked bytes fall by 6,474
under v2 and 6,463 under v1. The D rise is a precision cost to review before
shipping. The oracle counted common-word record values and raw text hits per
layer: C 33/25, A 7/7, D 5/0, R 13/45. A hit count is not a tokenization
count; corroboration can leave an occurrence raw.

The ordinary no-record scorecards are byte-identical to the fresh base in
every layer under v2 and v1: leaked and false-positive bytes, refusals, exact
restores, and valid manifests all match. The strict gain command exits `1`
because it requires an improvement, which this opt-in arm cannot produce
without a record. The base is #712 head `4d9007d2`, whose tree is identical
to merged main `d2c7c0f4`. Both sides generated setup policy SHA-256
`f909a23a` and used corpus SHA-256 `916c6379`, seed `20260710`, the same
host, and separate worktrees and build targets. Other CPU and benchmark work
overlapped this run, so its latency is not an isolated measurement.

The round-1 prototype at `1703143b` protected 99.40% of 8,147 eligible
leaked bytes. Round 2 starts from #712's stronger baseline, leaving 6,513
eligible leaked bytes, so these percentages should not be compared as a
like-for-like change in recall. The first full prototype at `d6636bc6`
protected 95.37% overall but missed the prediction's lower bound in A
(89.38%, with 342 residual `TAXNUM` bytes). The round-1 parser refused
noncanonical spacing, weak values, and common-word single-token names.
Round 2 canonicalizes spacing and accepts common-word names with
corroboration. The remaining 55 eligible leaked bytes are a known gap, not a
claim of complete protection.

The full arm uses the primary document corpus, generated identifiers,
lookalikes, and repeated-value documents. It pairs record values with
negative documents and adds explicit benign counterweights for homonyms,
nearby digit strings, and partial address text. The harness stores only
aggregate counts; neither context values nor document text are committed.
It records leaked and false-positive UTF-8 bytes, refusals, exact restore,
valid-manifest counts, and common-word hit counts under both scored-label
contracts. The reproducible harness is
[`known_record_arm.py`](../../../scripts/bench/known_record_arm.py); the full
aggregate results are [`v2`](known-record-oracle-v2.json) and
[`v1`](known-record-oracle-v1.json). Later documentation and aggregate-only
commits do not change detection code from the measured source commit.
