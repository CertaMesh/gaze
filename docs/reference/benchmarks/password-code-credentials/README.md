# Password hashes and code credentials: gain evidence

This is the retained **generator-v22** measurement. The fresh [source-escape report](source-escapes/README.md) measures the repaired head on generator v23 and supplies the latest gain-gate evidence. The tables and records below keep their original identities.

The v4 headline and both required gain gates (v2 and v1) pass: **403,175 fewer leaked bytes, 2,520 additional raw false-positive bytes, net 400,655 bytes**. The isolated mechanism passes the same contracts. These are byte-quality results; no latency or throughput improvement is claimed.

Measured main base: `4c15ca80f85bbd3258eb856ac71ea424b1cc6bb5`. Measured candidate: `2068418bb89db000adcf880307974357ff716afe`. A later signed merge of main `3aa5c95fe5d996d29a774aa99dc827c0ec7cddfb` produced `0a25d8458465cfd4cf529b90f3eb1aca8fe2d296`. The [merge proof](post-capture-merge-proof.json) lists every changed file and confirms that detection runtime and benchmark implementations, dependencies, toolchain, corpus and contracts are unchanged. CI-equivalent checks ran again after that merge.

The subsequent user-directed [TLS merge](tls-merge-proof.json) updates only `rustls-webpki` 0.103.15 → 0.103.16 and `webpki-roots` 1.0.7 → 1.0.10 in the dependency lockfile, plus the changelog. Runtime and benchmark source blobs remain identical. The retained captures and binary digests predate these TLS patches; no post-TLS capture or binary-identity equality is claimed. Advisory checks, formatting, clippy and TLS-consumer tests ran after the merge; complete CI verifies the final review head.

Each side was built with Rust 1.96.0 and measured in its own checkout, using its own `gaze setup` policy with Davlan NER and Nym enabled. The primary test corpus has 2,910 documents. Generated benchmark **specification v22**, corpus `ee25d9eefd2af48c99214002e5cdfc909222e6e1efa72e5bd98874246acb82ac`, has A 8,657 / D 6,122 / R 5,101 documents. Sampling seed is 20260710; the generated partition retains its own pinned seed 2026092602.

One v2 observation record per side supplies v1, v3 and v4 by re-scoring. An independent native v1 pair had already finished before the one-capture decision; it is retained only as a cross-check. Its per-layer byte metrics, refusals, restore/manifest counts and per-label recall equal the v1 replays. No further contract-specific captures were started. Each mechanism side was captured once, under v2, and re-scored for the other contracts.

Historical release remeasurement was explicitly deferred to a separate backfill job. The release-history tables remain bound to their recorded generator v20 corpus and must not be compared directly with these v22 values. No new historical release row or inferred historical score is published here.

## Per-layer results

All pairs below are base → candidate, in UTF-8 bytes. Raw FP is the gain-gate metric, before the separate headline gold-gap credit. “All gold” includes validator-failed gold that has no gate credit; “all processed” retains refused documents in the denominator. With zero refusals, its leaked-byte count equals all-gold leaked bytes.

### Contract v4 (latest headline)

| Layer | Gated leaked B | All-gold leaked B | Raw FP B | Refused | Leaked on all processed B |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 | 9,717 → 9,717 |
| A | 205,469 → 71,244 | 206,671 → 72,446 | 2,993 → 4,589 | 0 → 0 | 206,671 → 72,446 |
| D | 0 → 0 | 0 → 0 | 10,650 → 11,580 | 0 → 0 | 0 → 0 |
| R | 389,894 → 120,944 | 389,894 → 120,944 | 1,347 → 1,341 | 0 → 0 | 389,894 → 120,944 |

[Machine-readable verdict](gate-v4.json): **PASS**.

### Contract v2

| Layer | Gated leaked B | All-gold leaked B | Raw FP B | Refused | Leaked on all processed B |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 6,652 → 6,652 | 6,652 → 6,652 | 27,004 → 27,004 | 0 → 0 | 6,652 → 6,652 |
| A | 205,469 → 71,244 | 206,671 → 72,446 | 2,993 → 4,589 | 0 → 0 | 206,671 → 72,446 |
| D | 0 → 0 | 0 → 0 | 10,650 → 11,580 | 0 → 0 | 0 → 0 |
| R | 389,894 → 120,944 | 389,894 → 120,944 | 1,347 → 1,341 | 0 → 0 | 389,894 → 120,944 |

[Machine-readable verdict](gate-v2.json): **PASS**.

### Contract v1

| Layer | Gated leaked B | All-gold leaked B | Raw FP B | Refused | Leaked on all processed B |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 | 9,717 → 9,717 |
| A | 205,469 → 71,244 | 206,671 → 72,446 | 2,993 → 4,589 | 0 → 0 | 206,671 → 72,446 |
| D | 0 → 0 | 0 → 0 | 10,650 → 11,580 | 0 → 0 | 0 → 0 |
| R | 389,894 → 120,944 | 389,894 → 120,944 | 1,347 → 1,341 | 0 → 0 | 389,894 → 120,944 |

[Machine-readable verdict](gate-v1.json): **PASS**.

### Contract v3

| Layer | Gated leaked B | All-gold leaked B | Raw FP B | Refused | Leaked on all processed B |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 6,652 → 6,652 | 6,652 → 6,652 | 27,004 → 27,004 | 0 → 0 | 6,652 → 6,652 |
| A | 205,469 → 71,244 | 206,671 → 72,446 | 2,993 → 4,589 | 0 → 0 | 206,671 → 72,446 |
| D | 0 → 0 | 0 → 0 | 10,650 → 11,580 | 0 → 0 | 0 → 0 |
| R | 389,894 → 120,944 | 389,894 → 120,944 | 1,347 → 1,341 | 0 → 0 | 389,894 → 120,944 |

[Machine-readable verdict](gate-v3.json): **PASS**.

Frozen primary contracts v2 and v3 exclude PASSWORD and SECURITYTOKEN from layer C; primary v1 and v4 include them. Each generated layer uses its own credential-inclusive contract, so its new credential gold is scored under every primary-contract run. No frozen contract was widened.

| Layer | Processed documents, each side | Exact restores | Valid manifests | Restore failures | Invalid manifests |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 2,910 | 2,910 → 2,910 | 2,910 → 2,910 | 0 → 0 | 0 → 0 |
| A | 8,657 | 8,657 → 8,657 | 8,657 → 8,657 | 0 → 0 | 0 → 0 |
| D | 6,122 | 6,122 → 6,122 | 6,122 → 6,122 | 0 → 0 | 0 → 0 |
| R | 5,101 | 5,101 → 5,101 | 5,101 → 5,101 | 0 → 0 | 0 → 0 |

All four contracts have these same document, refusal, restore and manifest counts. Both sides process all 22,790 documents without a refusal, restore failure or invalid manifest.

## Isolated mechanism

Both policies use the measured candidate binary. The base retains the previous secret recognizers. The candidate adds exactly `password.hash`, `password.code` and `security_token.code` through the [committed policy delta](../../../../scripts/bench/mechanisms/password-code-credentials.toml); NER, Nym and every other policy setting stay identical. The [mechanism ledger](../mechanism-arms.json) retains both value-free records and re-derives every contract. The [pair-equivalence receipt](mechanism-main-equivalence.json) verifies that its metrics, per-label recall, refusals, restore and manifest counts equal the main comparison on both sides in every layer. This is an unreleased measurement for the upcoming v0.18 release.

| Contract | Leaked bytes removed | Raw FP bytes added | Net bytes | Verdict |
| --- | ---: | ---: | ---: | --- |
| v4 | 403,175 | 2,520 | 400,655 | [PASS](mechanism-gate-v4.json) |
| v2 | 403,175 | 2,520 | 400,655 | [PASS](mechanism-gate-v2.json) |
| v1 | 403,175 | 2,520 | 400,655 | [PASS](mechanism-gate-v1.json) |
| v3 | 403,175 | 2,520 | 400,655 | [PASS](mechanism-gate-v3.json) |

## Scope and costs

The [trace attribution](false-positive-attribution.json) accounts for every changed-cell FP byte. In layer A, Nym labels the SQL keyword `VALUES` as a name, adding 1,596 FP bytes across the changed SQL cells. The same keyword loses 6 FP bytes in one layer R cell. This can hide useful SQL syntax from the agent, even though strict restore succeeds. The new `password.code` recognizer adds all 930 layer D FP bytes: translations 416, validation rules 320, masked values 130 and schema examples 64. These costs remain in the gate. The governed checksum-label counterweights do not regress.

Generic `token` keys are excluded from the code-token recognizer; non-JWT secrets stored under that key can remain clear. Stored-hash matching establishes an encoding shape, not cryptographic validity. PHP expression evaluation and arbitrary plaintext SQL positional ownership remain outside the contract. The 120 compact/nested PHP rows per partition share expression shapes despite separate rendered contexts and value pools, so the result does not establish generalization to unseen PHP syntax. See [the detection contract](../../../../docs/explanation/detection/password-credentials.md).

## Evidence and reproduction

- [Executed commands and exits](commands.json), [hardware](hardware.json), [toolchain selection](toolchain.json), [binary digests](binary-identities.json), and [measured source identities](source-identities.json).
- Native v2 scorecards: [base](base-v2.json.gz), [candidate](candidate-v2.json.gz); latest v4 replays: [base](base-v4.json.gz), [candidate](candidate-v4.json.gz). Scorecards name their retained observation archives and digests.
- Native v1 cross-checks: [base proof](base-native-v1-replay-proof.json), [candidate proof](candidate-native-v1-replay-proof.json), [gate](gate-native-v1.json).
- [Canonical replay verification](verification.json). Export receipts prove observation rows are unchanged; publication only normalizes a policy-path home prefix when needed. Gzip file and decompressed-content digests are distinguished explicitly.

Run `python3 docs/reference/benchmarks/password-code-credentials/verify.py` to reproduce all scorecards, gates and FP trace attribution against the committed receipts, without models or the private corpus. Re-score the retained v2 records with `scripts/bench/scorecard_record.py` and the selected `scored-labels-vN.json` (omit the option for v1). Run `scripts/bench/agentic_layers.py gate` on the resulting base/candidate scorecards. `scripts/bench/mechanism_arms.py check` re-derives the mechanism ledger without a model, executable or private corpus. The retained canonical captures and reported verification commands ran one at a time on the recorded machine. The benchmark lease was released after captures so another lane could use it; no additional captures were started.
