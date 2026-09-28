# Known-record oracle arm

This is a **separate upper-bound experiment**, not the Gaze benchmark headline.
The harness gives Gaze each document's selected gold values before detection,
as if an adopter already knew the customer or order record. Real apps may know
less. The ordinary no-record benchmark continues to use its existing request
shape and is measured separately under both scored-label contracts.

The preregistered prediction was that record context would protect 90–100% of
baseline leaked bytes within eligible exact-value gold spans. The first full
prototype run (candidate `d6636bc6`) protected 95.37% combined, but the
generated-identifier layer protected **89.38%** and missed the lower bound;
342 residual bytes were `TAXNUM`. That run included name-order and email-case
variants. Its exact-only and variant stages scored identically in aggregate,
so those variants were removed from the prototype after review. The final
record parser also refuses noncanonical spacing, weak values and common-word
single-token names. The final arm must be measured again on its own commit.

The full arm uses the main synthetic document corpus, generated identifiers,
lookalikes and repeated-value documents. It pairs record values with negative
documents and adds explicit benign counterweights for homonyms, nearby digit
strings and partial address text. The harness stores only aggregate counts;
neither context values nor document text are committed. It records leaked and
false-positive UTF-8 bytes, refusals, exact restore and valid-manifest counts
under both v2 and v1 scored-label contracts. The reproducible harness is
[`known_record_arm.py`](../../../scripts/bench/known_record_arm.py).

Final-head aggregate files and results are pending the shared benchmark slot.
