# Password hashes after serialized separators

Fresh main and same-binary mechanism comparisons pass contracts v2, v1, v4 and v3. Under v2 the whole credential change removes 855,588 leaked UTF-8 bytes and adds 2,874 net raw false-positive bytes. No latency or throughput comparison is claimed.

Base: `5ac6c0628a62b4d85456e9941117306a0c4b3350`. Candidate and generator: `86989b2e2bd16dbf770eb25935159a51e87bc53c`. Each main side uses its own Rust 1.96.0 release binary and its own setup policy. [Source and binary identities](source-identities.json), [commands and exits](commands.json), [hardware](hardware.json).

Generator v25, test corpus SHA-256 `133f8452d01fd517c77087e539ba9dc253e30a1886668bfab3e1631cc9ff6c32`, seed 20260710. All four native captures run serially under one owned machine lease. Each contract is re-scored from the same observation record for that side. The standard setup retains Davlan NER and Nym.

This comparison measures the whole credential change against current main. The prefix cells isolate the newly added serialized-prefix and within-string cases. Earlier [v24](../json-separators/README.md), [v23](../source-escapes/README.md) and [v22](../README.md) evidence retains its recorded corpus identities. Generator v24 reconstructs byte-for-byte. This page introduces no past-release rows.

After capture, main advanced to `29ac3b2d2cc4f2b2b1241b8dd0ad97e4d0f51a88` and was merged as `8544bd349d4cfefaebc98f79ad977f1a01a4a172`. Rebuilding both benchmark binaries and regenerating both setup policies produces exactly the captured SHA-256 values. Detector libraries, generator and scorer stay unchanged. The [source identities](source-identities.json) record this proof. Merged workspace and publication checks pass, and all three evidence commands replay from separate committed empty checkouts.

The [equivalence receipt](mechanism-main-equivalence.json) records identical per-layer outcomes for main and mechanism under every contract.

## Main contract v4 and mechanism

All byte pairs are base → candidate. Raw false positives precede gold-gap credit.

| Layer | Gated leakage | All-gold leakage | Raw false positives | Refusals |
| --- | ---: | ---: | ---: | ---: |
| C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 |
| A | 321,878 → 71,244 | 323,080 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 13,750 → 14,680 | 0 → 0 |
| R | 725,898 → 120,944 | 725,898 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v4.json), [mechanism gate](mechanism-gate-v4.json).

## Main contract v2 and mechanism

All byte pairs are base → candidate. Raw false positives precede gold-gap credit.

| Layer | Gated leakage | All-gold leakage | Raw false positives | Refusals |
| --- | ---: | ---: | ---: | ---: |
| C | 6,652 → 6,652 | 6,652 → 6,652 | 27,004 → 27,004 | 0 → 0 |
| A | 321,878 → 71,244 | 323,080 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 13,750 → 14,680 | 0 → 0 |
| R | 725,898 → 120,944 | 725,898 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v2.json), [mechanism gate](mechanism-gate-v2.json).

## Main contract v1 and mechanism

All byte pairs are base → candidate. Raw false positives precede gold-gap credit.

| Layer | Gated leakage | All-gold leakage | Raw false positives | Refusals |
| --- | ---: | ---: | ---: | ---: |
| C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 |
| A | 321,878 → 71,244 | 323,080 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 13,750 → 14,680 | 0 → 0 |
| R | 725,898 → 120,944 | 725,898 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v1.json), [mechanism gate](mechanism-gate-v1.json).

## Main contract v3 and mechanism

All byte pairs are base → candidate. Raw false positives precede gold-gap credit.

| Layer | Gated leakage | All-gold leakage | Raw false positives | Refusals |
| --- | ---: | ---: | ---: | ---: |
| C | 6,652 → 6,652 | 6,652 → 6,652 | 27,004 → 27,004 | 0 → 0 |
| A | 321,878 → 71,244 | 323,080 → 72,446 | 3,003 → 4,953 | 0 → 0 |
| D | 0 → 0 | 0 → 0 | 13,750 → 14,680 | 0 → 0 |
| R | 725,898 → 120,944 | 725,898 → 120,944 | 1,391 → 1,385 | 0 → 0 |

[Main gate](main-gate-v3.json), [mechanism gate](mechanism-gate-v3.json).

## Source spans and restoration

| Layer | Documents per arm | Exact restores | Valid manifests | Restore failures | Invalid manifests |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 2,910 | 2,910 → 2,910 | 2,910 → 2,910 | 0 → 0 | 0 → 0 |
| A | 10,037 | 10,037 → 10,037 | 10,037 → 10,037 | 0 → 0 | 0 → 0 |
| D | 7,522 | 7,522 → 7,522 | 7,522 → 7,522 | 0 → 0 | 0 → 0 |
| R | 6,481 | 6,481 → 6,481 | 6,481 → 6,481 | 0 → 0 | 0 → 0 |

## New prefix cells

Each bcrypt, Argon2id and SHA512crypt encoding is tested literally and with escaped slashes. Layer A places a hash after a serialized heading and separator. Layer R repeats each of two distinct consecutive hashes twice inside the same JSON stdout string. Separators include newline, tab, CR, quote, backslash, Unicode LF, Unicode slash, mixed-case Unicode hex, backspace and formfeed. Layer D has malformed escapes and identifier-glued prefixes. Only the complete source hash is gold.

| New v25 cells | Leakage | Raw false positives |
| --- | ---: | ---: |
| A | 51,725 → 0 | 0 → 0 |
| D | 0 → 0 | 1,690 → 1,690 |
| R | 206,688 → 0 | 0 → 0 |

[Prefix totals](source-prefix-cells.json), [retained separator totals](source-separator-cells.json), [source-escape totals](source-escape-cells.json). The [B6-only mutation](mutations.json) reverts only the left boundary in a separate committed-source copy. Both new reproductions fail with exit 101 while the earlier salt, slash and right-boundary expressions remain present. [Mutation log](B6-mutation.log).

## False positives and limits

The [trace attribution](false-positive-attribution.json) accounts for changed non-gold bytes by recognizer and source context. Protecting benign password-labelled metadata costs useful context despite exact restoration. Generic token keys remain excluded, so some literal credentials under that ambiguous key can remain clear. Hash recognition establishes encoding shape, not cryptographic validity. These supported source escapes do not implement a general source-language decoder. Arbitrary positional plaintext SQL ownership remains outside the contract. The left-boundary repair weakens no correctness axis.

| Layer | Source | Context | False-positive byte change |
| --- | --- | --- | ---: |
| A | `nym-small-int8` | SQL keyword VALUES | +1,950 |
| D | `password.code` | other synthetic context | +930 |
| R | `nym-small-int8` | SQL keyword VALUES | -6 |

## Public replay

From a committed checkout with no target directory, run:

```bash
uv run --project scripts/bench --python 3.13 python docs/reference/benchmarks/password-code-credentials/json-prefixes/verify.py
```

The model-free replay reproduces 16 scorecards and eight gates from four value-free archives. It verifies exact execution-policy snapshots against both runner and parameter hashes, then materializes only the mechanism policies required by the unchanged gate. Existing policy files must match. Models, private corpus and pre-existing policies are unnecessary. [Policy manifest](policies.json), [verification](verification.json), [empty-checkout proof](empty-checkout-verification.json), [CI input audit](ci-input-audit.json).

The [document identity and gain decomposition](measurement-identity-and-gain.json) checks identical document arrays across all four captures. The whole PR gain includes earlier credential cells. Residual leakage bounds the result to the committed corpus. No complete-detection claim is made for arbitrary text.
