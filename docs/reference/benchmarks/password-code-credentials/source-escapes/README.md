# Escaped password hashes: fresh gain evidence

The v4 headline and required v2/v1 main and mechanism gain gates **PASS**. The main comparison removes **442,135 leaked bytes** for **2,874 additional raw false-positive bytes**, a net 439,261 bytes. No latency or throughput improvement is claimed.

Measured main base: `55c5d32b8e6d5082f65e3d3afebf7a60a442bb62`. Measured candidate and harness: `6eb72a93e341962945e5569e0c27cf3e989ad986`. Both release binaries were built with Rust 1.96.0, each in its own checkout, using the policy its own `gaze setup` wrote. The binary SHA-256 digests differ; setup-policy digests match. [Source and binary identities](source-identities.json), [hardware](hardware.json), and [executed commands and exits](commands.json).

The setup policies enable Davlan NER and Nym and include `secrets`. The generated corpus is specification **v23**, test SHA-256 `5b9a3ad435210a3c2543989a0cd41255b60ff24a4f7efef5d21dee4db1e9713e`. Layer C has 2,910 documents, A 8,837, D 6,742 and R 5,281. Each arm processes 23,770 documents. Sampling seed is 20260710.

Each side was captured once. The same value-free observation record supplies v4, v2, v1 and v3 by re-scoring. Four native captures ran serially under one machine lease: main base/candidate and isolated mechanism base/candidate. No independent contract captures or historical release backfill ran. The previous [v22 evidence](../README.md) and release-history rows keep their recorded corpus identities. v23 reconstructs every v22 document byte-for-byte; historical backfill remains a separate approved job.

## Main and isolated-mechanism outcomes

Both comparisons have identical per-layer byte, refusal, restore and manifest counts under every contract, as the [equivalence receipt](mechanism-main-equivalence.json) verifies. The tables below apply to **both main and mechanism comparisons**.

### Main contract v4 (also the mechanism result)

All pairs are base → candidate in UTF-8 bytes. Raw FP is the gain-gate metric before headline gold-gap credit. With zero refusals, all-gold leaked bytes equal leakage on all processed documents.

| Layer | Gated leaked B | All-gold / all-processed leaked B | Raw FP B | Refused |
| --- | ---: | ---: | ---: | ---: |
| C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 |
| A | 218,473 → 71,244 | 219,675 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 11,000 → 11,930 | 0 → 0 |
| R | 415,850 → 120,944 | 415,850 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v4.json), [mechanism gate](mechanism-gate-v4.json): **PASS**.

### Main contract v2 (also the mechanism result)

All pairs are base → candidate in UTF-8 bytes. Raw FP is the gain-gate metric before headline gold-gap credit. With zero refusals, all-gold leaked bytes equal leakage on all processed documents.

| Layer | Gated leaked B | All-gold / all-processed leaked B | Raw FP B | Refused |
| --- | ---: | ---: | ---: | ---: |
| C | 6,652 → 6,652 | 6,652 → 6,652 | 27,004 → 27,004 | 0 → 0 |
| A | 218,473 → 71,244 | 219,675 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 11,000 → 11,930 | 0 → 0 |
| R | 415,850 → 120,944 | 415,850 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v2.json), [mechanism gate](mechanism-gate-v2.json): **PASS**.

### Main contract v1 (also the mechanism result)

All pairs are base → candidate in UTF-8 bytes. Raw FP is the gain-gate metric before headline gold-gap credit. With zero refusals, all-gold leaked bytes equal leakage on all processed documents.

| Layer | Gated leaked B | All-gold / all-processed leaked B | Raw FP B | Refused |
| --- | ---: | ---: | ---: | ---: |
| C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 |
| A | 218,473 → 71,244 | 219,675 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 11,000 → 11,930 | 0 → 0 |
| R | 415,850 → 120,944 | 415,850 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v1.json), [mechanism gate](mechanism-gate-v1.json): **PASS**.

Contract v3 also passes: [main](main-gate-v3.json), [mechanism](mechanism-gate-v3.json). Frozen primary contracts v2/v3 exclude credential labels in layer C; v1/v4 include them. Generated layers use their credential-inclusive contract under every primary replay.

### Restore and manifest counts

| Layer | Documents per arm | Exact restores | Valid manifests | Restore failures | Invalid manifests |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 2,910 | 2,910 → 2,910 | 2,910 → 2,910 | 0 → 0 | 0 → 0 |
| A | 8,837 | 8,837 → 8,837 | 8,837 → 8,837 | 0 → 0 | 0 → 0 |
| D | 6,742 | 6,742 → 6,742 | 6,742 → 6,742 | 0 → 0 | 0 → 0 |
| R | 5,281 | 5,281 → 5,281 | 5,281 → 5,281 | 0 → 0 | 0 → 0 |

Counts are physical observation outcomes and stay unchanged across label-contract replays. Mechanism counts are in the equivalence receipt and gate files.

## Source escapes and counterweights

The added cells include maximum-width crypt salts with SQL doubled quotes and MySQL backslash quotes, JSON escaped slashes in fixed- and variable-width hashes, and exact-source repeats. Raw and serialized over-limit salts, malformed escape tails, glued suffixes, and truncated/overlong fixed checksums remain benign counterweights.

| New v23 cells | Leaked B | Raw FP B |
| --- | ---: | ---: |
| A | 13,004 → 0 | 10 → 364 |
| D | 0 → 0 | 350 → 350 |
| R | 25,956 → 0 | 44 → 44 |

The base leaks on the added source-unit shapes, so the benchmark sees the repaired gap. Candidate source-unit gold has zero leaked bytes in A and R, and its D false-positive bytes do not rise. [Re-derived source-cell totals](source-escape-cells.json).

## False-positive costs and limits

The [trace attribution](false-positive-attribution.json) accounts for every changed generated-cell raw false-positive byte. Non-gold byte intervals are joined to committed synthetic text; no primary-corpus text is published.

| Layer | Producer | Context | FP-byte change |
| --- | --- | --- | ---: |
| A | `nym-small-int8` | SQL keyword VALUES | +1,950 |
| D | `password.code` | other synthetic context | +930 |
| R | `nym-small-int8` | SQL keyword VALUES | -6 |

These costs can hide useful SQL syntax and password-labelled metadata from an agent even when strict restoration succeeds. Generic `token` keys remain excluded from the code-token rule, so non-JWT credentials under that ambiguous key can remain clear. Password validation, translation, schema and masked-value metadata stays fail-closed and can be tokenized. Hash recognition establishes encoding shape rather than cryptographic validity. Supported quote/slash escapes are not a general source-language decoder. Arbitrary plaintext SQL positional ownership remains outside this contract.

The mechanism uses the same candidate binary on both sides. Its base retains the previous two secret recognizers; the candidate appends `password.hash`, `password.code` and `security_token.code` through the [committed policy delta](../../../../../scripts/bench/mechanisms/password-code-credentials.toml). Other policies and models stay fixed. [Mechanism ledger](../../mechanism-arms.json).

## Regression and replay proof

All 17 local Rust verification stages pass: formatting, all-feature/all-target Clippy, focused recognizers, full all-feature CLI/workspace tests, cargo deny and required repository gates. The focused suites contain 14 credential and 32 security-token tests. Generator suites pass 8 credential-generator and 251 agentic-layer tests. Full verification ran before the captures.

Both new tests failed on the original rules at the reviewed SQL and PHP JSON reproductions. After the repair, they assert one complete hash span, exact source restoration, decoded-width limits, malformed counterweights and `password.hash` audit provenance. Independent committed-source copies reverting each repair fail again with exit 101. [Mutation receipts](mutations.json). The [measured detector source](tested-source-equivalence.json) is unchanged from the tested/mutated repair head `8c86add6`.

Run the model-free public proof from a checkout of this evidence:

```bash
uv run --project scripts/bench --python 3.13 python docs/reference/benchmarks/password-code-credentials/source-escapes/verify.py
```

It re-derives all 16 scorecards and eight gates from four portable observation archives, checks archive hashes, attributes false-positive deltas and checks the new-cell totals. [Verification receipt](verification.json). Export receipts prove observation rows are unchanged; only a policy-path home prefix may be normalized. Every scorecard links its exact archive.
