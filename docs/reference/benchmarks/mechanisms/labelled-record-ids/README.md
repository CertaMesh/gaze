# Labelled record ID measurements

All six source gates pass on generator v25, under contracts v2, v1 and v4.
Full policy removes 186,965 gated leaked bytes with 239 added FP bytes;
rules only removes 197,363 with 307 added FP bytes.
No layer's gated or all-processed leaks or refusals rise. Every attempted document
restores exactly and has a valid manifest: C 2,910; A 10,037; D 7,522; R 6,481.
All refusal, restore-failure and invalid-manifest counts are zero. Layer D's
strict false-positive counterweights pass under every contract.

Measured source pair: main `9a70af36478ef84a6b43c9cc16b14ab302dff8ce` and
candidate `d1d03fd3baf8ce55630e55efa0b7d85302467da0`. Both trees were clean.
All four native captures ran fresh with cache reuse disabled, on the same machine
and corpus. Sampling seed: `20260710`; generated test seed: `2026092602`.
Receipts pin the generator corpus, source and crates trees, actual native binary
hashes, policies, observation archives and original execution evidence.

## Policies and source comparisons

Full policy uses each revision's own fresh `gaze setup` output. The two policy
files are byte-identical. NER and the Nym safety net remain enabled. Evidence is
in [full-policy/receipt.json](full-policy/receipt.json).

Rules only uses the two distinct source binaries with one shared policy derived
from fresh main setup: remove only the `ner` and `safety_net` tables and their
children. Parsed values outside those keys are unchanged. Candidate setup is
verified identical to main setup. Evidence and the derivation receipt are in
[rules-only-source/receipt.json](rules-only-source/receipt.json).

The ordinary layer gate admits both source comparisons. `mechanism_arms.py`
requires one source revision and therefore does not admit this source pair; its
ledger and checker are unchanged. These measurements establish byte comparisons,
with no latency claim. The absolute release-readiness threshold still reports
residual leaks; the passes above are relative gain comparisons.

## Full policy

| Layer (contract) | Gated leaked bytes | FP bytes | Refused | Leaked-on-all-processed bytes |
| --- | ---: | ---: | ---: | ---: |
| C (v2) | 6,652 → 6,587 | 27,004 → 27,011 | 0 → 0 | 6,652 → 6,587 |
| C (v1, v4) | 9,717 → 9,652 | 27,021 → 27,028 | 0 → 0 | 9,717 → 9,652 |
| A (v2, v1, v4) | 71,244 → 4,426 | 4,953 → 5,035 | 0 → 0 | 72,446 → 5,628 |
| D (v2, v1, v4) | 0 → 0 | 14,680 → 14,680 | 0 → 0 | 0 → 0 |
| R (v2, v1, v4) | 120,944 → 862 | 1,385 → 1,535 | 0 → 0 | 120,944 → 862 |

## Rules only

| Layer (contract) | Gated leaked bytes | FP bytes | Refused | Leaked-on-all-processed bytes |
| --- | ---: | ---: | ---: | ---: |
| C (v2) | 78,770 → 78,705 | 3,629 → 3,636 | 0 → 0 | 78,770 → 78,705 |
| C (v1, v4) | 82,348 → 82,283 | 3,646 → 3,653 | 0 → 0 | 82,348 → 82,283 |
| A (v2, v1, v4) | 80,840 → 9,506 | 96 → 246 | 0 → 0 | 82,045 → 10,711 |
| D (v2, v1, v4) | 0 → 0 | 9,515 → 9,515 | 0 → 0 | 0 → 0 |
| R (v2, v1, v4) | 129,114 → 3,150 | 443 → 593 | 0 → 0 | 129,114 → 3,150 |

Contracts are grouped in a row only when all displayed counts are identical.
FP bytes are the raw bytes used by the gate, before the headline gold-gap
adjustment. Leaked-on-all-processed counts retain all scored gold, including the
validator-failed gold omitted from some gated totals. Refused documents cannot
hide an improvement: the gate separately checks availability and restore facts.

## Precision costs and scalar boundaries

Quoted scalars end at their own closing delimiter through nested JSON escaping,
without a fixed wrapper-depth limit. Malformed quotes stop only at a delimiter
with proof of a following assignment, matching container close, or record end.
Ambiguous punctuation and newlines remain part of the private value. Independent synthetic tests failed before this repair and now pass;
the existing ownership corpus now requires exact public-byte preservation for
both shapes. Against the audited pre-repair candidate, all three diagnostic
gates pass: 78,012 FP bytes disappear with no leaked-byte, refusal, restore or
manifest regression. Those older captures provide diagnostic comparison only
and receive no current gain credit.

Unquoted tab grouping remains conservative to protect the complete private
value. Ambiguous producer-declared public last columns add 150 FP bytes in A
and 150 in R, split equally across customer, employee and record classes. Each
layer includes 120 public-value bytes and 30 separator bytes. This is a known
limit, retained to avoid leaking tab-containing private identifiers. Full policy
also removes 68 old A FP bytes, so A's net rise is 82; rules only has no such
removal. D is unchanged.

C adds 25 and removes 18 FP bytes (net 7). Sixteen added bytes protect explicitly
labelled customer numbers missing from or mislabelled in holdout gold; nine
protect public prepositions/separators before phone values. These counts remain
in the gate; no gold or scorer ruling was changed. No newly FP A/R bytes remain
on deep-serialization or malformed-quote cells, and none were added on public
order/invoice/build ID cells.

Unquoted alphabetic-only placeholders remain rejected. Existing repeat length
floors and common-word guards remain in force. Typographic value quotes and CSV
header linkage are not characterized. The benchmark does not replace review of
real text or the ownership and snapshot implementation.

## Reproduce and verify

Each arm retains its original bound observations and execution evidence.
`captured-scorecard-*.json.gz` preserves the original v1 capture in scorecard
schema 4. `scorecard-*-v<N>.json.gz` contains its re-scored contract view. Re-score
an observation archive with `scripts/bench/rescore.py`, or decompress a scorecard
before using `scripts/bench/agentic_layers.py gate`. Receipts include commands and
reconstruction filenames. `scorecard_sha256` hashes decompressed JSON;
`scorecard_archive_sha256` separately hashes its deterministic gzip archive.

Published full-policy copies normalize only the two model-directory home paths
to `~`. Their publication hashes are separate from original capture-policy
hashes. Generate local runtime paths with each checkout's `gaze setup`; the
original captures, scores and their hashes are unchanged.

Historical policy-ablation inputs (`base-core.toml`, `mechanism.toml` and the
append-only policy pair) retain their original source receipt. That ablation was
not measured here and receives no gain credit. Earlier source measurements are
retained in Git history; they receive no credit for this source pair.
