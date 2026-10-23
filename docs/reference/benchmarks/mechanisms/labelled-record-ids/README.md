# Labelled record ID measurements

The v26 evidence below is historical. The enclosing-string and record-end repair
and generator v27 require fresh captures; those measurements are pending.

All six historical source gates pass on generator v26, under contracts v2, v1 and v4.
Full policy removes 199,535 gated leaked bytes with +239 net FP bytes;
rules only removes 211,133 with +307 net FP bytes.
No layer's gated or all-processed leaks or refusals rise. Every attempted document
restores exactly and has a valid manifest: C 2,910, A 10,307, D 7,792, R 6,751.
All refusal, restore-failure and invalid-manifest counts are zero.

Full policy: 325 gross FP bytes added, 86 removed. All 300 added generated-layer FP bytes come from ambiguous unquoted tab-group columns. The 540 new private-suffix/repeat documents have zero leaked or FP bytes; their 270 public-key counterweights have 272 FP bytes, unchanged from base.

Rules only: 325 gross FP bytes added, 18 removed. All 300 added generated-layer FP bytes come from ambiguous unquoted tab-group columns. The 540 new private-suffix/repeat documents have zero leaked or FP bytes; their 270 public-key counterweights have 0 FP bytes, unchanged from base.

The bound all-gold interval comparison finds zero newly exposed bytes in every layer under every contract.

Detector baseline: main `9cb43a703c729140315d5951304bd5734e1e96f7`.
The clean baseline carrier `26ab741bedee005a57fbf1c99f694e0e93b6d362`
contains only the committed generator extension and its tests over that main;
its crates and Cargo inputs match main exactly. It is published on
`bench/labelled-suffix-base`. Candidate capture:
`1e10fcb6ddf44240feefe8d7e63fb568cf1e5d15`.
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
| A (v2, v1, v4) | 75,194 → 4,426 | 4,953 → 5,035 | 0 → 0 | 76,396 → 5,628 |
| D (v2, v1, v4) | 0 → 0 | 14,952 → 14,952 | 0 → 0 | 0 → 0 |
| R (v2, v1, v4) | 129,564 → 862 | 1,385 → 1,535 | 0 → 0 | 129,564 → 862 |

## Rules only

| Layer (contract) | Gated leaked bytes | FP bytes | Refused | Leaked-on-all-processed bytes |
| --- | ---: | ---: | ---: | ---: |
| C (v2) | 78,770 → 78,705 | 3,629 → 3,636 | 0 → 0 | 78,770 → 78,705 |
| C (v1, v4) | 82,348 → 82,283 | 3,646 → 3,653 | 0 → 0 | 82,348 → 82,283 |
| A (v2, v1, v4) | 85,430 → 9,506 | 96 → 246 | 0 → 0 | 86,635 → 10,711 |
| D (v2, v1, v4) | 0 → 0 | 9,515 → 9,515 | 0 → 0 | 0 → 0 |
| R (v2, v1, v4) | 138,294 → 3,150 | 443 → 593 | 0 → 0 | 138,294 → 3,150 |

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

The current repair retains raw bytes through nested JSON escaping. An
unterminated value ends before the nearest enclosing serialized-string close
(with escape depth preserved), at an unescaped LF/CRLF/CR record boundary, or
at input end. Key-looking content and container punctuation stay private;
escaped line breaks remain content. Generator v27 adds key-looking private
content and separate real public-field counterweights. Historical v25 and v26
corpora remain byte-identical and rebuildable under their original contracts.

Conservative malformed suffixes and unquoted tab grouping can protect public
bytes. The tables report the measured precision cost. The source gain gate
separately checks per-layer leaks, refusals, exact restore and manifest validity;
false positives cannot compensate for a leaked-byte regression. Existing repeat
length floors and common-word guards remain unchanged. Typographic quotes and
CSV header linkage remain uncharacterized. The benchmark does not replace
independent review of real-text recall and precision.
