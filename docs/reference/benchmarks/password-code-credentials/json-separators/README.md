# Password hashes before JSON separators: fresh detection evidence

All eight main and mechanism gates pass under contracts v2, v1, v4 and v3. The main v2 comparison removes 597,175 leaked UTF-8 bytes for 2,874 additional raw false-positive bytes. This is a detection-byte evaluation; no latency, throughput or performance improvement is claimed.

Main base: `eed3903f84ebeffc4d16b88914243f5eeb663c4d`. Measured candidate and harness: `3188916e61972b651f1de7e3f99ed421c2320098`. Each main side uses its own Rust 1.96.0 release binary and its own `gaze setup` policy. Binary digests differ; setup-policy digests match. [Source identities](source-identities.json), [hardware](hardware.json), [executed commands and exits](commands.json).

Generator v24, test corpus SHA-256 `37f80cfb8b5f9ac67cdf7a0eba14fb501ead0ba53644867dfceb891412d6278b`, seed 20260710. Four full-corpus native captures ran serially under one machine lease, once per main/mechanism side. Every scored-label contract is a replay of that side's single record. There are no performance repeats. Model-backed setup retains Davlan NER and Nym. Models, corpus documents, contracts, action mode and seeds are checked by the existing gate.

This page compares the whole credential change with current main. The separator subset below isolates the new boundary surfaces. It does not describe the entire gain as a separator-only improvement. Frozen [v23 evidence](../source-escapes/README.md) and [v22 evidence](../README.md) retain their own corpus identities. Generator v24 reconstructs v23 byte-for-byte. No past-release comparison rows are introduced here.

The [equivalence receipt](mechanism-main-equivalence.json) verifies identical per-layer outcomes for main and mechanism under every contract.

## Main contract v4 (also mechanism)

All pairs are base → candidate, in UTF-8 bytes. Raw FP precedes gold-gap credit.

| Layer | Gated leakage | All-gold leakage | Raw FP | Refusals |
| --- | ---: | ---: | ---: | ---: |
| C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 |
| A | 270,153 → 71,244 | 271,355 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 12,060 → 12,990 | 0 → 0 |
| R | 519,210 → 120,944 | 519,210 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v4.json), [mechanism gate](mechanism-gate-v4.json).

## Main contract v2 (also mechanism)

All pairs are base → candidate, in UTF-8 bytes. Raw FP precedes gold-gap credit.

| Layer | Gated leakage | All-gold leakage | Raw FP | Refusals |
| --- | ---: | ---: | ---: | ---: |
| C | 6,652 → 6,652 | 6,652 → 6,652 | 27,004 → 27,004 | 0 → 0 |
| A | 270,153 → 71,244 | 271,355 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 12,060 → 12,990 | 0 → 0 |
| R | 519,210 → 120,944 | 519,210 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v2.json), [mechanism gate](mechanism-gate-v2.json).

## Main contract v1 (also mechanism)

All pairs are base → candidate, in UTF-8 bytes. Raw FP precedes gold-gap credit.

| Layer | Gated leakage | All-gold leakage | Raw FP | Refusals |
| --- | ---: | ---: | ---: | ---: |
| C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 |
| A | 270,153 → 71,244 | 271,355 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 12,060 → 12,990 | 0 → 0 |
| R | 519,210 → 120,944 | 519,210 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v1.json), [mechanism gate](mechanism-gate-v1.json).

Contract v3: [main gate](main-gate-v3.json), [mechanism gate](mechanism-gate-v3.json). Frozen v2/v3 primary contracts exclude credential labels in C; v1/v4 include them. Generated layers use the credential-inclusive contract throughout.

## Complete spans and restoration

| Layer | Documents per arm | Exact restores | Valid manifests | Restore failures | Invalid manifests |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 2,910 | 2,910 → 2,910 | 2,910 → 2,910 | 0 → 0 | 0 → 0 |
| A | 9,437 | 9,437 → 9,437 | 9,437 → 9,437 | 0 → 0 | 0 → 0 |
| D | 7,042 | 7,042 → 7,042 | 7,042 → 7,042 | 0 → 0 | 0 → 0 |
| R | 5,881 | 5,881 → 5,881 | 5,881 → 5,881 | 0 → 0 | 0 → 0 |

## New separator cells

Each bcrypt, Argon2id and SHA512crypt hash is tested both literally and with escaped-slash alphabet units immediately before every separator: newline, tab, CR, quote, backslash, Unicode LF, Unicode slash, mixed-case Unicode hex, backspace and formfeed. The source hash alone is gold; separators remain outside it. Repeated source hashes are separately gold. Dangling backslashes at end of input, unknown escapes, incomplete/invalid Unicode escapes and identifier tails remain benign counterweights.

| New v24 cells | Leakage | Raw FP |
| --- | ---: | ---: |
| A | 51,680 → 0 | 0 → 0 |
| D | 0 → 0 | 1,060 → 1,060 |
| R | 103,360 → 0 | 0 → 0 |

[Re-derived separator totals](source-separator-cells.json); [retained v23 source-escape totals](source-escape-cells.json). The base must leak on these positives for gate visibility. The new Rust regression also fails on the reviewed boundary and passes on the repair. [Committed-source B3-only mutation](mutations.json) reverts only the boundary and fails the intended test with exit 101; B1/B2 stay present. [Failure log](B3-mutation.log).

## False positives and practical limits

The [trace attribution](false-positive-attribution.json) accounts for every changed generated-cell FP byte by recognizer, class and synthetic context. These byte costs can hide useful public context from an agent even when restore succeeds.

| Layer | Source | Context | FP-byte change |
| --- | --- | --- | ---: |
| A | `nym-small-int8` | SQL keyword VALUES | +1,950 |
| D | `password.code` | other synthetic context | +930 |
| R | `nym-small-int8` | SQL keyword VALUES | -6 |

Hash detection checks encoding shape, not cryptographic validity. Only escaped slash continues an alphabet; the supported complete JSON escapes terminate a hash. This is not a general source-language decoder. Generic `token` code keys remain excluded; password-labelled metadata remains fail-closed and may be protected. Arbitrary positional plaintext SQL ownership is outside this contract. No correctness axis is weakened by the boundary repair.

The mechanism runs the same candidate binary on both sides. Its base retains the previous two secret rules; its candidate appends the three credential rules through the [committed delta](../../../../../scripts/bench/mechanisms/password-code-credentials.toml), preserving all other bundles and model settings. [Mechanism ledger](../../mechanism-arms.json).

## Verification and public replay

Formatting, workspace all-feature/all-target Clippy, focused recognizers, all-feature CLI and workspace tests, cargo deny, required xtask gates, integrated CI contracts and generator checks pass. The official scorer/comparator lane passes 1,626 tests and 4,177 subtests, with six skips. Native capture reuse and policy/CLI equivalence also pass. [Commands and exits](commands.json), [unchanged implementation proof](final-implementation-equivalence.json). The composed regression asserts complete source spans, exact strict restore and `password.hash` audit provenance; malformed tails and fixed-width counterweights remain covered.

From an empty checkout with an empty `target/`, run:

```bash
uv run --project scripts/bench --python 3.13 python docs/reference/benchmarks/password-code-credentials/json-separators/verify.py
```

The model-free replay verifies all four archives and reproduces 16 scorecards, eight gates, source-cell totals and FP attribution. Exact policy snapshots are committed under `policies/`; their SHA-256 must match both `runner_provenance.policy.sha256` and `parameters.policy_sha256` for every scorecard. Mechanism snapshots are materialized at their recorded repository-relative paths, then the unchanged gate rechecks provenance. Existing files must match exactly. Captured model and rulepack locations inside snapshots are execution metadata; replay does not load models or private artifacts. [Policy manifest](policies.json), [verification receipt](verification.json). Export receipts preserve every observation row.

## What the byte comparison measures

The [document identity and gain receipt](measurement-identity-and-gain.json) proves that all four record headers contain the same document arrays, not only the same counts or corpus label. Every native side is captured once. Re-scoring changes the label contract without executing a detector again. Refusals, invalid manifests and restore failures remain visible in the gate, so a rejected document cannot conceal a leak gain.

| Generated family | Leaked bytes removed |
| --- | ---: |
| `other` | 120 |
| `credentials_` | 192,872 |
| `credentials_review_` | 210,183 |
| `credentials_escapes_` | 38,960 |
| `credentials_separators_` | 155,040 |
| `primary_C` | 0 |

The sum equals the main v2 gate's leaked-byte reduction. Existing credential families account for part of the gain, while the new separator family measures the boundary repair. Residual leakage in other document shapes bounds this result; this is not a claim of complete detection on arbitrary text. The claim applies to these committed generated cells and the checksum-pinned primary corpus. No latency or throughput was compared.

The [empty-checkout receipt](empty-checkout-verification.json) records both documented commands on separate committed checkouts with absent targets. The [final CI input audit](ci-input-audit.json) reviews every newly registered path and records the integrated contract check.
