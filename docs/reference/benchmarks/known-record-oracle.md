# Known-record oracle arm

This is a **separate upper-bound experiment**, not the Gaze benchmark headline.
The harness gives Gaze each document's selected gold values before detection,
as if an adopter already knew the customer or order record. Real apps may know
less. The ordinary no-record benchmark continues to use its existing request
shape and is measured separately under both scored-label contracts.

The preregistered prediction was that record context would protect 90–100% of
baseline leaked bytes within eligible exact-value gold spans. The reviewed
prototype at measured source commit `1703143b` protects **99.40%** overall:
8,147 eligible leaked bytes fall to 49 under both scored-label contracts.
Layer A now protects **100%** of its eligible leaked bytes. The ordinary
benchmark, with no record supplied, is byte-identical to branch base
`97e863ac` in every layer under both contracts. Its strict gain command exits
`1` only because it requires an improvement, which this opt-in arm cannot
produce without a record. Both sides used the same setup policy, corpus and
seed.

| Layer | Eligible leaked bytes, base → record | Headline leaked bytes v2, base → record | Headline leaked bytes v1, base → record | False-positive bytes, base → record |
| --- | ---: | ---: | ---: | ---: |
| C, primary corpus | 5,041 → 49 | 9,256 → 4,258 | 15,443 → 10,420 | 27,282 → 27,389 |
| A, generated identifiers | 2,878 → 0 | 5,738 → 2,860 | 5,738 → 2,860 | 882 → 786 |
| D, benign lookalikes | none | 0 → 0 | 0 → 0 | 6,570 → 6,933 |
| R, repeated values | 228 → 0 | 234 → 6 | 234 → 6 | 419 → 493 |

No document was refused. All 6,416 documents restored exactly and had valid
manifests in each arm and contract. Total false-positive bytes rise by 448;
total headline leaked bytes fall by 8,104 under v2 and 8,129 under v1. The
lookalike layer includes 256 explicit benign record counterweights.

The first full prototype run (candidate `d6636bc6`) protected 95.37% of its
eligible leaked bytes combined, but generated identifiers protected only
**89.38%** and missed the prediction's lower bound; 342 residual bytes were
`TAXNUM`. That run included name-order and email-case variants. Its exact-only
and variant stages scored identically in aggregate, so those variants were
removed after review. The final parser also refuses noncanonical spacing,
weak values and common-word single-token names. These guards reduced the
eligible baseline from 8,772 to 8,147 leaked bytes; the improved percentage
therefore applies to a smaller declared set. Main-corpus headline leaked bytes
with a record rose from 4,026 in the pilot to 4,258 after these guards.

The full arm uses the primary document corpus, generated identifiers,
lookalikes and repeated-value documents. It pairs record values with negative
documents and adds explicit benign counterweights for homonyms, nearby digit
strings and partial address text. The harness stores only aggregate counts;
neither context values nor document text are committed. It records leaked and
false-positive UTF-8 bytes, refusals, exact restore and valid-manifest counts
under both scored-label contracts. The reproducible harness is
[`known_record_arm.py`](../../../scripts/bench/known_record_arm.py); the full
aggregate results are [`v2`](known-record-oracle-v2.json) and
[`v1`](known-record-oracle-v1.json). The later test-fixture and aggregate-only
commits do not change detection code from the measured source commit.
