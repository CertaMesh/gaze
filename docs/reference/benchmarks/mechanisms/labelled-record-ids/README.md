# Labelled record ID measurements

All six source gates pass on generator v27, under contracts v2, v1 and v4.
Full policy removes 482,182 gated leaked bytes with +212 net FP bytes;
rules only removes 502,112 with +307 net FP bytes.
No layer's gated or all-processed leaks or refusals rise. Every attempted document
restores exactly and has a valid manifest: C 2,910, A 11,840, D 9,379, R 8,284.
All refusal, restore-failure and invalid-manifest counts are zero.

Full policy: 325 gross FP bytes added, 113 removed. 300 added FP bytes occur in generated layers, including 300 from ambiguous unquoted tab-group columns. The 3,726 current private-boundary and repeat documents have zero leaked or FP bytes; their 1,917 public counterweights have 4,562 FP bytes, unchanged from base.

Rules only: 325 gross FP bytes added, 18 removed. 300 added FP bytes occur in generated layers, including 300 from ambiguous unquoted tab-group columns. The 3,726 current private-boundary and repeat documents have zero leaked or FP bytes; their 1,917 public counterweights have 0 FP bytes, unchanged from base.

Full policy, quote-dialect subset: all 1,824 private and repeat documents have zero leaked bytes; their 966 real public counterweights have 2,056 FP bytes, unchanged from base. The subset includes 54 genuinely absent-value public notes.

Rules only, quote-dialect subset: all 1,824 private and repeat documents have zero leaked bytes; their 966 real public counterweights have 0 FP bytes, unchanged from base. The subset includes 54 genuinely absent-value public notes.

The bound all-gold interval comparison finds zero newly exposed bytes in every layer under every contract.

Detector baseline: main `9cb43a703c729140315d5951304bd5734e1e96f7`.
The clean baseline carrier `c4ab57de271812acd509699c2e9d4e40998f3694`
contains the committed benchmark corpus, tests and export helper over that main;
its crates and Cargo inputs match main exactly. It is published on
`bench/labelled-quote-base`. Candidate capture:
`20ff2b01e621383ff903989ce5f40e8300871290`.
All captures use the identical runner, scorer, generator and contract inputs.

All four native captures ran fresh with cache reuse disabled, on the same
machine and corpus. Sampling seed: `20260710`; generated test seed: `2026092602`.
Each checkout uses its own fresh `gaze setup` policy; full policies are byte-identical.
Rules only removes exactly the NER and safety-net tables, including their children;
all other parsed values remain unchanged. Native exit 4 records residual leaks
against the absolute release-readiness threshold. These are relative source gain
comparisons, with no latency claim. The full-policy candidate capture overlapped
other local jobs and host memory pressure. Its wall-clock time is not a controlled
performance comparison.

## Full policy

| Layer (contract) | Gated leaked bytes | FP bytes | Refused | Leaked-on-all-processed bytes |
| --- | ---: | ---: | ---: | ---: |
| C (v2) | 6,652 → 6,587 | 27,004 → 27,011 | 0 → 0 | 6,652 → 6,587 |
| C (v1, v4) | 9,717 → 9,652 | 27,021 → 27,028 | 0 → 0 | 9,717 → 9,652 |
| A (v2, v1, v4) | 168,388 → 4,426 | 4,980 → 5,035 | 0 → 0 | 169,590 → 5,628 |
| D (v2, v1, v4) | 0 → 0 | 19,242 → 19,242 | 0 → 0 | 0 → 0 |
| R (v2, v1, v4) | 319,017 → 862 | 1,385 → 1,535 | 0 → 0 | 319,017 → 862 |

## Rules only

| Layer (contract) | Gated leaked bytes | FP bytes | Refused | Leaked-on-all-processed bytes |
| --- | ---: | ---: | ---: | ---: |
| C (v2) | 78,770 → 78,705 | 3,629 → 3,636 | 0 → 0 | 78,770 → 78,705 |
| C (v1, v4) | 82,348 → 82,283 | 3,646 → 3,653 | 0 → 0 | 82,348 → 82,283 |
| A (v2, v1, v4) | 182,423 → 9,506 | 96 → 246 | 0 → 0 | 183,628 → 10,711 |
| D (v2, v1, v4) | 0 → 0 | 9,515 → 9,515 | 0 → 0 | 0 → 0 |
| R (v2, v1, v4) | 332,280 → 3,150 | 443 → 593 | 0 → 0 | 332,280 → 3,150 |

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

Single and triple ASCII delimiters preserve doubled quotes and complete multiline
values. Every closing search stops before its enclosing serialized string's close.
Unclassified quoting owns through the nearest enclosing close (exclusive), raw
unescaped LF/CRLF/CR record end, or input end, retaining nonempty ownership.
Key-looking private content never proves a public boundary. Enclosing JSON stays
parseable through serialization depths 1 through 9; exact signed-session restore retains
all original bytes. Proven empty JSON note values leave public siblings intact;
opaque nested text cannot establish an absent value.

The current unreleased generator v27 folds in 2,790 YAML, TOML, CSV-style, SQL and
nested-JSON quote-dialect documents per partition, including real public sibling
and reference fields and 54 absent-value counterweights. Together with the
unfinished and key-looking boundary cells, the audit covers 5,643 documents.
Application-declared raw ranges define gold independently of detector output.
The committed Rust fixture has 4,986 rows. Earlier v25 and v26 corpora reconstruct
byte-for-byte under their original contracts; historical pins remain unchanged.

An unescaped line break ends an unterminated value's record. Content on the next
line needs its own evidence. Ambiguous unquoted tab grouping can protect public
bytes; the tables disclose that precision cost. No leaked private byte is traded
for a false-positive improvement. Existing repeat length floors and common-word
guards remain unchanged. Typographic quote dialects and general CSV header
linkage remain uncharacterized. Independent real-text review remains necessary.
