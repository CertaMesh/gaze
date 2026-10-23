# Labelled record ID measurements

All six source gates pass on generator v27, under contracts v2, v1 and v4.
Full policy removes 277,768 gated leaked bytes with +219 net FP bytes;
rules only removes 293,834 with +307 net FP bytes.
No layer's gated or all-processed leaks or refusals rise. Every attempted document
restores exactly and has a valid manifest: C 2,910, A 10,928, D 8,413, R 7,372.
All refusal, restore-failure and invalid-manifest counts are zero.

Full policy: 325 gross FP bytes added, 106 removed. 300 added FP bytes occur in generated layers, including 300 from ambiguous unquoted tab-group columns. The 1,902 current malformed/private-key and repeat documents have zero leaked or FP bytes; their 951 public-key counterweights have 2,506 FP bytes, unchanged from base.

Rules only: 325 gross FP bytes added, 18 removed. 300 added FP bytes occur in generated layers, including 300 from ambiguous unquoted tab-group columns. The 1,902 current malformed/private-key and repeat documents have zero leaked or FP bytes; their 951 public-key counterweights have 0 FP bytes, unchanged from base.

The bound all-gold interval comparison finds zero newly exposed bytes in every layer under every contract.

Detector baseline: main `9cb43a703c729140315d5951304bd5734e1e96f7`.
The clean baseline carrier `14c5331968d2e3c6e16bdf410f76d197b8dcb899`
contains the committed benchmark corpus, tests and export helper over that main;
its crates and Cargo inputs match main exactly. It is published on
`bench/labelled-enclosing-base`. Candidate capture:
`e73e53a2a5a7b1df6e0445c68b3b3ceae8cd72a6`.
All captures use the identical runner, scorer, generator and contract inputs.

All four native captures ran fresh with cache reuse disabled, on the same
machine and corpus. Sampling seed: `20260710`; generated test seed: `2026092602`.
Each checkout uses its own fresh `gaze setup` policy; full policies are byte-identical.
Rules only removes exactly the NER and safety-net tables, including their children;
all other parsed values remain unchanged. Native exit 4 records residual leaks
against the absolute release-readiness threshold. These are relative source gain
comparisons, with no latency claim.

## Full policy

| Layer (contract) | Gated leaked bytes | FP bytes | Refused | Leaked-on-all-processed bytes |
| --- | ---: | ---: | ---: | ---: |
| C (v2) | 6,652 → 6,587 | 27,004 → 27,011 | 0 → 0 | 6,652 → 6,587 |
| C (v1, v4) | 9,717 → 9,652 | 27,021 → 27,028 | 0 → 0 | 9,717 → 9,652 |
| A (v2, v1, v4) | 100,498 → 4,426 | 4,973 → 5,035 | 0 → 0 | 101,700 → 5,628 |
| D (v2, v1, v4) | 0 → 0 | 17,186 → 17,186 | 0 → 0 | 0 → 0 |
| R (v2, v1, v4) | 182,493 → 862 | 1,385 → 1,535 | 0 → 0 | 182,493 → 862 |

## Rules only

| Layer (contract) | Gated leaked bytes | FP bytes | Refused | Leaked-on-all-processed bytes |
| --- | ---: | ---: | ---: | ---: |
| C (v2) | 78,770 → 78,705 | 3,629 → 3,636 | 0 → 0 | 78,770 → 78,705 |
| C (v1, v4) | 82,348 → 82,283 | 3,646 → 3,653 | 0 → 0 | 82,348 → 82,283 |
| A (v2, v1, v4) | 112,997 → 9,506 | 96 → 246 | 0 → 0 | 114,202 → 10,711 |
| D (v2, v1, v4) | 0 → 0 | 9,515 → 9,515 | 0 → 0 | 0 → 0 |
| R (v2, v1, v4) | 193,428 → 3,150 | 443 → 593 | 0 → 0 | 193,428 → 3,150 |

Contracts share a row only when all displayed counts are identical. FP counts
use raw gate accounting. Leaked-on-all-processed bytes include all scored gold.

Receipts pin source/crates trees, native binaries, policies, observations,
original execution evidence and re-scored cards. Reconstruct original captures
as described by each receipt's `capture_reconstruction` field. Original policy
hashes differ from publication copies whose model-directory home prefixes use `~`.

- [Full-policy receipt](full-policy/receipt.json)
- [Rules-only receipt](rules-only-source/receipt.json)
- [Full-policy FP attribution](full-policy/precision-costs.json)
- [Rules-only FP attribution](rules-only-source/precision-costs.json)

## Ownership and precision limits

Quoted scalars retain raw bytes through nested JSON escaping. An unterminated
value ends before the nearest enclosing serialized-string close, at an unescaped
LF/CRLF/CR record boundary, or at input end. Key-looking content and container
punctuation stay private; escaped line breaks remain content. Enclosing JSON
keeps its closing quotes and remains parseable. Generator v27 adds 1,863 documents
with key-looking private content and separate real public fields, and corrects
current malformed-newline/public counterweights to follow the record-end rule.
The v25 and v26 corpora reconstruct byte-for-byte under their original contracts.

An unescaped line break ends the unterminated value's record. Content on the next
line requires its own detection evidence. Ambiguous malformed suffixes and
unquoted tab grouping can protect public bytes. The tables report the measured
precision cost. The source gain gate checks per-layer leaks, refusals, exact
restore and manifest validity; false positives cannot compensate for new leaked
bytes. Existing repeat length floors and common-word guards remain unchanged.
Typographic quotes and CSV header linkage remain uncharacterized. The benchmark
does not replace independent review of real-text recall and precision.
